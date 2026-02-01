+++
model = "claude-opus-4-5"
created = 2026-02-01
modified = 2026-02-01
driver = "Isaac Clayton"
+++

# Diamond-Types Deep Dive

## Overview

- Repository: https://github.com/josephg/diamond-types
- Language: Rust
- Author: Joseph Gentle (Seph)
- Primary innovations: B-tree based range tree, JumpRope (skip list of gap buffers), 5000x faster than Automerge

Diamond-types is a high-performance CRDT library for text editing. Joseph Gentle created it as a prototype to explore how fast a well-optimized CRDT could be. The result: processing 260,000 editing operations in 56 milliseconds natively, over 5000x faster than Automerge and 5x faster than yjs.

The library separates "space" (document content) from "time" (operation history), using distinct data structures optimized for each concern.

## Architecture

Diamond-types uses a two-tier architecture:

1. **OpLog (Operation Log)**: Stores the complete history of changes as a time DAG
2. **Branch**: Stores a snapshot of the document at some point in time

This separation allows efficient operations: the OpLog can be shared and synced without materializing document state, while Branches provide fast local editing.

```rust
pub struct ListOpLog {
    pub cg: CausalGraph,                    // Time DAG + agent assignment
    pub(crate) operation_ctx: ListOperationCtx,  // Insert/delete content
    pub(crate) operations: RleVec<KVPair<ListOpMetrics>>,
}

pub struct ListBranch {
    version: Frontier,           // Current version (set of LVs)
    content: JumpRopeBuf,        // Document content as a rope
}
```

## Data Structures

### Local Versions (LV)

Diamond-types uses local version numbers (LV) internally rather than (agent, seq) pairs. Every operation seen by a peer is assigned a monotonically increasing LV. This enables:

- Efficient storage: single usize instead of (agent_id, seq) tuple
- Fast lookups: direct array indexing
- Compact RLE: consecutive operations from the same agent compress to ranges

The mapping between LVs and (agent, seq) pairs is maintained in the `CausalGraph`:

```rust
pub type LV = usize;

pub struct CausalGraph {
    pub graph: Graph,                    // Parent relationships
    pub agent_assignment: AgentAssignment,  // LV <-> (agent, seq) mapping
    pub version: Frontier,               // Current frontier
}
```

### JumpRope: Skip List of Gap Buffers

JumpRope is a custom rope implementation that combines skip lists with gap buffers. It processes around 35-40 million edits per second.

**Skip List Structure:**

```rust
pub struct JumpRope {
    rng: RopeRng,              // For random height generation
    num_bytes: usize,          // Total UTF-8 bytes
    head: Node,                // First node (inline)
}

pub struct Node {
    str: GapBuffer<NODE_STR_SIZE>,  // 392 bytes in release
    height: u8,                      // Skip list height
    nexts: [SkipEntry; MAX_HEIGHT+1],
}

pub struct SkipEntry {
    node: *mut Node,
    skip_chars: usize,  // Characters skipped by this edge
}
```

Key properties:
- NODE_STR_SIZE = 392 bytes (10 in debug for testing)
- MAX_HEIGHT = 20 levels
- BIAS = 65/256 probability of height increment
- Each node stores both content AND skip pointers

**Gap Buffer:**

Each node contains a gap buffer for O(1) local edits:

```rust
pub struct GapBuffer<const LEN: usize> {
    data: [u8; LEN],
    gap_start_bytes: u16,
    gap_start_chars: u16,
    gap_len: u16,
    all_ascii: bool,  // Fast path for ASCII-only content
}
```

The gap buffer enables efficient in-place editing:
- Insert at gap: O(1)
- Move gap: O(gap distance) memcpy
- The `all_ascii` flag enables fast character counting when true

### ContentTree: B-tree for CRDT Items

The ContentTree stores CRDT items (CRDTSpan) in a B-tree with dual length tracking:

```rust
pub struct ContentTree<V: Content> {
    leaves: Vec<ContentLeaf<V>>,
    nodes: Vec<ContentNode>,
    height: usize,
    root: usize,
    total_len: LenPair,
    cursor: Option<(Option<LenPair>, DeltaCursor)>,
}

pub struct LenPair {
    pub cur: usize,  // Current visible length
    pub end: usize,  // End-state length (after all ops applied)
}
```

Node sizes (configurable):
- NODE_CHILDREN = 16 (release), 4 (debug)
- LEAF_CHILDREN = 32 (release), 4 (debug)

The dual-length tracking (cur/end) enables efficient time travel: `cur` tracks what is visible now, `end` tracks what will be visible after all operations are applied.

### CRDTSpan: The Item Type

```rust
pub struct CRDTSpan {
    pub id: DTRange,           // Local version range
    pub origin_left: LV,       // Left neighbor at creation
    pub origin_right: LV,      // Right neighbor at creation
    pub current_state: SpanState,  // NOT_INSERTED_YET | INSERTED | DELETED
    pub end_state_ever_deleted: bool,
}

pub struct SpanState(pub u32);  // 0=not inserted, 1=inserted, 2+=deleted n-1 times
```

Like yjs, diamond-types uses dual origins (left and right). But unlike yjs:
- Items are stored in a B-tree, not a linked list
- Run-length encoding collapses consecutive items into spans
- The `end_state_ever_deleted` flag enables efficient "final state" computation

### IndexTree: LV to Leaf Mapping

The IndexTree provides O(log n) lookup from LV to ContentTree leaf:

```rust
type Index = IndexTree<Marker>;

pub enum Marker {
    InsPtr(LeafIdx),           // Points to ContentTree leaf
    Del(DelRange),             // Delete information
}

pub struct DelRange {
    target: LV,                // What was deleted
    fwd: bool,                 // Direction of delete
}
```

## Merge Algorithm

Diamond-types implements a modified YATA algorithm (compatible with yjs). The key insight is that YATA and Fugue produce identical merge behavior for practical cases.

### Ordering Rules

From `listmerge/merge.rs`, the integrate function:

1. Find `origin_left`: the item to the left when this insert was created
2. Find `origin_right`: the item to the right when this insert was created  
3. Scan forward from origin_left, looking for the insertion point
4. Stop scanning when we reach origin_right or find our position

The conflict resolution logic (simplified):

```rust
// Scan through potential conflicts
loop {
    let other_entry = cursor.get_item();
    
    // Stop at origin_right
    if other_lv == item.origin_right { break; }
    
    // Only consider NOT_INSERTED_YET items as conflicts
    if other_entry.current_state != NOT_INSERTED_YET { break; }
    
    let other_left_cursor = get_cursor_after(other_entry.origin_left);
    
    match other_left_cursor.cmp(&left_cursor) {
        Ordering::Less => break,      // Insert here
        Ordering::Greater => {},       // Continue scanning
        Ordering::Equal => {
            if item.origin_right == other_entry.origin_right {
                // Same origins: order by agent name, then seq
                let ins_here = my_name < other_name 
                    || (my_name == other_name && my_seq < other_seq);
                if ins_here { break; }
            } else {
                // Different right origins: use right origin ordering
                let my_right_cursor = get_cursor_before(item.origin_right);
                let other_right_cursor = get_cursor_before(other_entry.origin_right);
                
                if other_right_cursor < my_right_cursor {
                    scanning = true;
                    scan_cursor = cursor.clone();
                } else {
                    scanning = false;
                }
            }
        }
    }
    
    cursor.next();
}

if scanning {
    cursor = scan_cursor;
}
```

### Time Travel: Advance/Retreat

Diamond-types supports efficient time travel through advance and retreat operations:

```rust
// From advance_retreat.rs
fn advance_by_range(&mut self, range: DTRange) {
    // Mark items in range as INSERTED
    // Update ContentTree lengths
}

fn retreat_by_range(&mut self, range: DTRange) {
    // Mark items in range as NOT_INSERTED_YET or decrement delete count
    // Update ContentTree lengths
}
```

This enables:
- Checkout at any version
- Efficient diff computation
- Incremental merge without full replay

### Transformed Operations

The `TransformedOpsIterRaw` iterator converts operations from their original positions to positions in the target document state:

```rust
pub enum TransformedResultRaw {
    FF(DTRange),                    // Fast-forward (no conflicts)
    Apply { xf_pos: usize, op: KVPair<ListOpMetrics> },
    DeleteAlreadyHappened(DTRange), // Double delete
}
```

## Optimizations

### Run-Length Encoding Everywhere

Nearly every data structure uses RLE:
- Operations: consecutive inserts/deletes from same agent
- CRDTSpans: consecutive items with same origins
- History graph: consecutive parent relationships
- Agent assignments: consecutive LV ranges

The `RleVec` type provides transparent RLE with binary search:

```rust
pub struct RleVec<T: SplitableSpan + MergableSpan> {
    vec: Vec<T>,
}

impl<T> RleVec<T> {
    fn push(&mut self, item: T) {
        if let Some(last) = self.vec.last_mut() {
            if last.can_append(&item) {
                last.append(item);
                return;
            }
        }
        self.vec.push(item);
    }
}
```

### Cursor Caching

The ContentTree maintains an optional cached cursor:

```rust
cursor: Option<(Option<LenPair>, DeltaCursor)>,
```

The `DeltaCursor` tracks pending length updates that haven't been flushed to parent nodes:

```rust
pub struct DeltaCursor(pub ContentCursor, pub LenUpdate);

pub struct LenUpdate {
    pub cur: isize,
    pub end: isize,
}
```

This batches tree updates, avoiding O(log n) parent traversals for every edit.

### Gap Buffer Fast Paths

The gap buffer includes an `all_ascii` flag:

```rust
fn count_internal_chars(&self, s: &str) -> usize {
    if self.all_ascii { s.len() } else { count_chars(s) }
}
```

For ASCII text (common in code), character counting is O(1) instead of O(n).

### Underwater Items

The ContentTree is initialized with an "underwater" item that spans a huge range:

```rust
fn new_underwater() -> CRDTSpan {
    CRDTSpan {
        id: DTRange::new(UNDERWATER_START, UNDERWATER_START * 2 - 1),
        origin_left: usize::MAX,
        origin_right: usize::MAX,
        current_state: INSERTED,
        end_state_ever_deleted: false,
    }
}
```

This simplifies insertion logic by ensuring there's always content to insert relative to.

### Memory Layout

Diamond-types uses Struct of Arrays (SoA) in several places:
- ContentTree: separate Vec for leaves and nodes
- OpLog: separate storage for content strings and operation metadata

The B-tree nodes use fixed-size arrays:

```rust
pub struct ContentLeaf<V> {
    children: [V; LEAF_CHILDREN],  // Fixed size, no heap allocation per item
    next_leaf: LeafIdx,
    parent: NodeIdx,
}
```

## Comparison with Yjs

| Aspect | Yjs | Diamond-types |
|--------|-----|---------------|
| Language | JavaScript | Rust |
| Item Storage | Doubly-linked list | B-tree |
| Position Lookup | O(1) with markers, O(n) worst | O(log n) always |
| Memory per char | ~80 bytes (JS objects) | ~40 bytes (CRDTSpan) |
| Content Storage | Inline in items | Separate JumpRope |
| RLE | Items merge adjacent | Spans + RLE everywhere |
| Search Markers | 80 cached markers | Cursor caching |
| Delete Tracking | Tombstone flags | SpanState counter |

### Key Differences

1. **Data Structure Choice**: Yjs uses a linked list with search markers for O(1) amortized access. Diamond-types uses a B-tree for guaranteed O(log n).

2. **Memory Layout**: Yjs items are JavaScript objects scattered across the heap. Diamond-types uses packed arrays in contiguous memory.

3. **Content Storage**: Yjs stores content inline in items. Diamond-types separates content into JumpRope, enabling different optimization strategies.

4. **Delete Representation**: Yjs stores deletes in an IdSet of ranges. Diamond-types tracks delete state per-span with a counter supporting double-deletes.

5. **Time Travel**: Both support it, but diamond-types makes it a first-class feature with advance/retreat operations.

## Performance Characteristics

From Joseph Gentle's blog post "CRDTs go brrr":

| Implementation | Time | RAM |
|---|---|---|
| Automerge | 291s | 880 MB |
| Reference-CRDTs | 31s | 28 MB |
| Yjs | 0.97s | 3.3 MB |
| Diamond (WASM) | 0.19s | - |
| Diamond (native) | 0.056s | 1.1 MB |

Processing rate: 4.6 million operations per second in native mode.

### Complexity Analysis

| Operation | Time Complexity |
|-----------|----------------|
| Insert | O(log n) + O(c) for conflicts |
| Delete | O(log n) |
| Merge | O(m log n) for m operations |
| Position lookup | O(log n) |
| LV to item | O(log n) |

Where:
- n = document size
- c = number of concurrent conflicting inserts (typically 0-2)
- m = number of operations to merge

## Lessons for Our Implementation

### What to Adopt

1. **Separate content from CRDT metadata**: JumpRope for content, ContentTree for CRDT items. This allows optimizing each independently.

2. **Local version numbers**: Map (agent, seq) to dense LV integers. Enables efficient storage and lookup.

3. **B-tree with dual length tracking**: LenPair (cur, end) enables efficient time travel without full replay.

4. **Run-length encoding everywhere**: Spans instead of individual items. Use RleVec as the default collection.

5. **Gap buffers in skip list nodes**: Combines O(log n) navigation with O(1) local edits.

6. **Cursor caching with delta updates**: Batch tree updates to avoid O(log n) parent traversals per edit.

7. **Underwater initialization**: Start with a sentinel item to simplify insertion logic.

### What to Consider Differently

1. **Rust-specific optimizations**: Many of diamond-types' optimizations rely on Rust's memory model (packed structs, no GC). JavaScript implementations need different strategies.

2. **Complexity**: Diamond-types is complex. For a simpler codebase, some optimizations (like the dual-tree structure) may not be worth it.

3. **Cursor caching vs search markers**: Yjs's 80 search markers might be simpler than cursor caching with delta updates. Worth benchmarking both.

4. **Content separation**: Storing content separately (JumpRope) vs inline (yjs) has tradeoffs. Inline is simpler; separate allows content-specific optimizations.

5. **Double-delete tracking**: Diamond-types tracks delete count per span. This handles malicious/unusual cases but adds complexity. Consider if it's needed.

## Code Walkthrough

### Inserting Text

```rust
// From branch.rs
pub fn insert(&mut self, oplog: &mut ListOpLog, agent: AgentId, pos: usize, content: &str) -> LV {
    apply_local_operations(oplog, self, agent, &[TextOperation::new_insert(pos, content)])
}
```

This flows through:
1. `add_operations_local`: Assign LV, push to operations RleVec
2. `merge`: Build M2Tracker, integrate the insert
3. `integrate`: Find position using origin_left/right, handle conflicts
4. Update JumpRope content

### Merging Remote Operations

```rust
// From merge.rs
pub fn walk(&mut self, graph: &Graph, aa: &AgentAssignment, 
            op_ctx: &ListOperationCtx, ops: &RleVec<KVPair<ListOpMetrics>>,
            start_at: Frontier, rev_spans: &[DTRange], 
            apply_to: Option<&mut JumpRopeBuf>) -> Frontier {
    
    let mut walker = SpanningTreeWalker::new(graph, rev_spans, start_at);

    for walk in &mut walker {
        // Move backwards in time
        for range in walk.retreat {
            self.retreat_by_range(range);
        }

        // Move forwards in time
        for range in walk.advance_rev.into_iter().rev() {
            self.advance_by_range(range);
        }

        // Apply new operations
        self.apply_range(aa, op_ctx, ops, walk.consume, apply_to.as_deref_mut());
    }

    walker.into_frontier()
}
```

The SpanningTreeWalker computes the optimal path through the time DAG, minimizing advance/retreat operations.

## Sources

- [CRDTs go brrr](https://josephg.com/blog/crdts-go-brrr/) - Joseph Gentle's blog post on optimization
- [Diamond-types GitHub](https://github.com/josephg/diamond-types) - Source code
- [JumpRope GitHub](https://github.com/josephg/jumprope-rs) - Skip list rope implementation
- [INTERNALS.md](https://github.com/josephg/diamond-types/blob/master/INTERNALS.md) - Internal documentation
- [Hacker News discussion](https://news.ycombinator.com/item?id=33903563) - Community discussion on the 5000x speedup
