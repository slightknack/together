+++
model = "claude-opus-4-5"
created = 2026-02-01
modified = 2026-02-01
driver = "Isaac Clayton"
+++

# Loro CRDT Library Deep Dive

## Overview

Loro is a Conflict-free Replicated Data Types (CRDTs) library that makes building local-first and collaborative apps easier. Available in Rust, JavaScript (via WASM), and Swift.

Repository: https://github.com/loro-dev/loro
Documentation: https://loro.dev/docs

### Key Innovations

1. Uses Fugue algorithm for text sequences (solves interleaving problem)
2. Integrates Event Graph Walker (Eg-walker) from diamond-types for efficient merging
3. Separates OpLog (history) from DocState (current state) for efficient time travel
4. Uses generic-btree library with RLE support for cache-efficient data structures
5. Implements Peritext for rich text formatting

### Supported CRDT Types

- Text editing with Fugue
- Rich text with Peritext
- Moveable trees
- Moveable lists
- Last-Write-Wins maps

## Core Algorithm: Fugue

Loro uses the Fugue algorithm for text sequences, as described in "The Art of the Fugue: Minimizing Interleaving in Collaborative Text Editing" by Matthew Weidner, Joseph Gentle, and Martin Kleppmann (2023).

### The Interleaving Problem

Most existing algorithms for replicated lists suffer from a critical problem: when two users concurrently insert text at the same position, the merged outcome may interleave the inserted passages.

Example of problematic interleaving:
```
User A types: "Hello"
User B types: "World"
Both insert at position 0

Bad merge result: "HWeolrllod"  (interleaved)
Good merge result: "HelloWorld" or "WorldHello"
```

### How Fugue Solves Interleaving

Fugue uses two key concepts:

1. `origin_left`: The ID of the character immediately to the left when this character was inserted
2. `origin_right`: The ID of the character immediately to the right when this character was inserted

The algorithm first resolves conflicts using `origin_left`. If there is still ambiguity (multiple concurrent inserts with the same left origin), it uses `origin_right` to break ties.

From `fugue_span.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub(super) struct FugueSpan {
    pub id: IdFull,
    /// Sometimes, the id of the span is just a placeholder.
    /// This field is used to track the real id of the span.
    pub real_id: Option<CompactId>,
    /// The status at the current version
    pub status: Status,
    /// The status at the `new` version (for diff calculation)
    pub diff_status: Option<Status>,
    pub origin_left: Option<CompactId>,
    pub origin_right: Option<CompactId>,
    pub content: RichtextChunk,
}
```

### Conflict Resolution Algorithm

From `crdt_rope.rs`, the core insertion logic:

```rust
// Calculate origin_left and origin_right
let origin_left = if start.cursor.offset == 0 {
    // Get left leaf node if offset == 0
    if let Some(left) = self.tree.prev_elem(start.cursor) {
        let left_node = self.tree.get_leaf(left.leaf.into());
        Some(left_node.elem().id.inc(left_node.elem().rle_len() - 1).id())
    } else {
        None
    }
} else {
    let left_node = self.tree.get_leaf(start.leaf().into());
    Some(left_node.elem().id.inc(start.offset() - 1).id())
};

// origin_right is the first non-future op between pos-1 and pos
let (origin_right, parent_right_leaf, in_between) = {
    let mut in_between = Vec::new();
    let mut origin_right = None;
    // ... scan for first non-future element
};
```

The tie-breaking for concurrent inserts with the same origins:

```rust
if content.origin_left == other_origin_left {
    if other_elem.origin_right == content.origin_right {
        // Same right parent - use peer ID for deterministic ordering
        if other_elem.id.peer > content.id.peer {
            break;  // Insert before other_elem
        } else {
            scanning = false;  // Continue scanning
        }
    } else {
        // Different right parent - compare positions
        match self.cmp_pos(other_parent_right_idx, parent_right_leaf) {
            Ordering::Less => scanning = true,
            Ordering::Equal if other_elem.id.peer > content.id.peer => break,
            _ => scanning = false,
        }
    }
}
```

### Complexity Analysis

| Operation | Time Complexity | Notes |
|-----------|-----------------|-------|
| Insert at position | O(log n) | B-tree lookup + Fugue scan |
| Delete range | O(log n + k) | k = range length |
| Merge | O(m log n) | m = operations to merge |
| Position lookup | O(log n) | Cached B-tree query |

## Data Structures

### generic-btree: The Foundation

Loro uses a custom B-tree implementation (`generic-btree`) that is designed for CRDT operations:

Repository: https://github.com/loro-dev/generic-btree

Key features:
- Pure safe Rust implementation
- Run-length encoding (RLE) support for efficient span storage
- Generic cache system for O(log n) position lookups
- Support for slicing and merging elements

From `generic-btree/src/lib.rs`:

```rust
pub trait BTreeTrait {
    /// Element type with RLE support
    type Elem: Debug + HasLength + Sliceable + Mergeable + TryInsert + CanRemove;
    /// Cache type for aggregate queries
    type Cache: Debug + Default + Clone + Eq;
    /// Cache diff for incremental updates
    type CacheDiff: Debug + Default + CanRemove;
    const USE_DIFF: bool = true;

    fn calc_cache_internal(cache: &mut Self::Cache, caches: &[Child<Self>]) -> Self::CacheDiff;
    fn apply_cache_diff(cache: &mut Self::Cache, diff: &Self::CacheDiff);
    fn get_elem_cache(elem: &Self::Elem) -> Self::Cache;
    // ...
}
```

The B-tree configuration:

```rust
const MAX_CHILDREN_NUM: usize = 12;  // B-tree branching factor
```

### RLE Traits

Elements in the tree support run-length encoding:

```rust
pub trait HasLength {
    fn rle_len(&self) -> usize;
}

pub trait Sliceable {
    fn _slice(&self, range: Range<usize>) -> Self;
    fn split(&mut self, pos: usize) -> Self;
}

pub trait Mergeable {
    fn can_merge(&self, rhs: &Self) -> bool;
    fn merge_right(&mut self, rhs: &Self);
    fn merge_left(&mut self, left: &Self);
}
```

### RichtextChunk: Compact Content Representation

From `fugue_span.rs`:

```rust
#[derive(Clone, PartialEq, Eq, Copy)]
pub(crate) struct RichtextChunk {
    start: u32,
    end: u32,
}
```

This compact 8-byte struct can represent:
- Text content (range into a string arena)
- Style anchors (start/end markers for formatting)
- Unknown placeholders (for lazy loading)
- Move anchors (for move operations)

Discriminated by magic values:

```rust
impl RichtextChunk {
    pub(crate) const UNKNOWN: u32 = u32::MAX;
    pub(crate) const START_STYLE_ANCHOR: u32 = u32::MAX - 1;
    pub(crate) const END_STYLE_ANCHOR: u32 = u32::MAX - 2;
    pub(crate) const MOVE_ANCHOR: u32 = u32::MAX - 3;
}
```

### FugueSpan: The CRDT Unit

Each span in the CRDT tree contains:

```rust
pub(super) struct FugueSpan {
    pub id: IdFull,           // (peer, counter, lamport) - 24 bytes
    pub real_id: Option<CompactId>,  // 8 bytes
    pub status: Status,       // 4 bytes (future flag + delete_times)
    pub diff_status: Option<Status>,  // 4 bytes
    pub origin_left: Option<CompactId>,  // 8 bytes
    pub origin_right: Option<CompactId>, // 8 bytes
    pub content: RichtextChunk,  // 8 bytes
}
// Total: ~64 bytes per span
```

The `Status` struct tracks visibility:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Default, Hash, Copy)]
pub(super) struct Status {
    pub future: bool,        // Is this from a future operation?
    pub delete_times: i16,   // How many times deleted (counter, not tombstone)
}

impl Status {
    pub fn is_activated(&self) -> bool {
        self.delete_times == 0 && !self.future
    }
}
```

### CrdtRope: The Main Tree Structure

From `crdt_rope.rs`:

```rust
pub(super) struct CrdtRope {
    pub(super) tree: BTree<CrdtRopeTrait>,
}
```

The tree trait implementation:

```rust
impl BTreeTrait for CrdtRopeTrait {
    type Elem = FugueSpan;
    type Cache = Cache;
    type CacheDiff = Cache;
    const USE_DIFF: bool = true;
    // ...
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Copy)]
pub(super) struct Cache {
    pub(super) len: i32,         // Active (visible) length
    pub(super) changed_num: i32, // Number of changed elements (for diff)
}
```

The cache enables O(log n) position lookups by storing aggregate lengths at each node.

### IdToCursor: Fast ID Lookups

From `id_to_cursor.rs`:

```rust
pub(super) struct IdToCursor {
    map: FxHashMap<PeerID, Vec<Fragment>>,
}

pub(super) struct Fragment {
    pub(super) counter: Counter,
    pub(super) cursor: Cursor,
}

pub(super) enum Cursor {
    Insert(InsertSet),
    Delete(IdSpan),
    Move { from: ID, to: LeafIndex },
}
```

This provides O(1) amortized lookup from operation ID to tree position, essential for efficient version switching.

The `InsertSet` handles fragmentation efficiently:

```rust
const MAX_FRAGMENT_LEN: usize = 256;
const SMALL_SET_MAX_LEN: usize = 32;

pub(crate) enum InsertSet {
    Small(SmallInsertSet),   // Linear search for small sets
    Large(LargeInsertSet),   // B-tree for large sets
}
```

## Memory Layout and Cache Efficiency

### Span Coalescing

Consecutive operations from the same peer are merged:

```rust
impl Mergeable for FugueSpan {
    fn can_merge(&self, rhs: &Self) -> bool {
        self.id.peer == rhs.id.peer
            && self.status == rhs.status
            && self.diff_status == rhs.diff_status
            && self.id.counter + self.content.len() as Counter == rhs.id.counter
            && self.id.lamport + self.content.len() as Lamport == rhs.id.lamport
            && rhs.origin_left.is_some()
            && rhs.origin_left.unwrap().peer == self.id.peer
            && rhs.origin_left.unwrap().counter.get()
                == self.id.counter + self.content.len() as Counter - 1
            && self.origin_right == rhs.origin_right
            && self.content.can_merge(&rhs.content)
            // ... real_id merging check
    }
}
```

This is critical for performance: sequential typing creates runs that merge into single spans.

### TextChunk: UTF-8 with Precomputed Lengths

From `richtext_state.rs`:

```rust
pub(crate) struct TextChunk {
    bytes: BytesSlice,    // Actual UTF-8 content
    unicode_len: i32,     // Precomputed Unicode codepoint count
    utf16_len: i32,       // Precomputed UTF-16 code unit count
    id: IdFull,           // Creation ID
}
```

This avoids repeated O(n) UTF-8 scanning for position calculations.

### B-tree Node Size

```rust
const MAX_CHILDREN_NUM: usize = 12;
```

This branching factor is chosen for:
- Good cache locality (nodes fit in cache lines)
- Reasonable tree height
- Efficient splitting/merging

### Arena Allocation

Both internal and leaf nodes use arena allocation:

```rust
pub struct BTree<B: BTreeTrait> {
    in_nodes: Arena<Node<B>>,
    leaf_nodes: Arena<LeafNode<B::Elem>>,
    root: ArenaIndex,
    root_cache: B::Cache,
}
```

Benefits:
- Stable indices (no pointer invalidation)
- Contiguous memory allocation
- Fast node access O(1)

## Index Optimization: O(log n) Lookups

### Query System

The generic B-tree supports custom queries:

```rust
pub trait Query<B: BTreeTrait> {
    type QueryArg: Clone;

    fn init(target: &Self::QueryArg) -> Self;

    fn find_node(&mut self, target: &Self::QueryArg, child_caches: &[Child<B>]) -> FindResult;

    fn confirm_elem(&mut self, q: &Self::QueryArg, elem: &B::Elem) -> (usize, bool);
}
```

### Position Queries

Two query types for position lookup:

```rust
/// Prefer left - stops at first element containing position
struct ActiveLenQueryPreferLeft { left: i32 }

/// Prefer right - continues past zero-length elements
struct ActiveLenQueryPreferRight { left: i32 }
```

Both traverse the tree using the cached `len` field, giving O(log n) position lookups.

### Cursor Caching

From `richtext_state.rs`:

```rust
#[derive(Clone, Debug)]
pub(super) struct CachedCursor {
    leaf: LeafIndex,
    index: FxHashMap<PosType, usize>,
}

impl RichtextState {
    pub(super) fn try_get_cache_or_clean(
        &mut self,
        index: usize,
        pos_type: PosType,
    ) -> Option<Cursor> {
        // Returns cached cursor if valid for query
    }
}
```

This optimizes sequential access patterns common in text editing.

## Event Graph Walker Integration

Loro integrates the Event Graph Walker algorithm from diamond-types for efficient merging.

### Architecture: OpLog vs DocState

```
OpLog (History)           DocState (Current State)
+----------------+        +-------------------+
| All operations |        | Current document  |
| in DAG order   |   -->  | No history info   |
+----------------+        +-------------------+
        |                         ^
        | DiffCalculator          |
        +-------------------------+
```

From the README:
> OpLog is dedicated to recording history, while DocState only records the current document state and does not include historical operation information.

### DiffMode Enumeration

From `diff_calc.rs`:

```rust
pub(crate) enum DiffMode {
    /// General mode - uses ContainerHistoryCache
    Checkout,
    /// Import new updates (concurrent possible)
    Import,
    /// Import updates greater than current version
    ImportGreaterUpdates,
    /// Linear history - fastest mode
    Linear,
}
```

### Tracker: Version Switching

From `tracker.rs`:

```rust
pub(crate) struct Tracker {
    applied_vv: VersionVector,   // All operations seen
    current_vv: VersionVector,   // Current checkout version
    rope: CrdtRope,              // The document state
    id_to_cursor: IdToCursor,    // ID -> position mapping
}
```

The tracker maintains an "unknown" placeholder that allows lazy materialization:

```rust
impl Tracker {
    pub fn new_with_unknown() -> Self {
        let mut this = Self { /* ... */ };
        this.rope.tree.push(FugueSpan {
            content: RichtextChunk::new_unknown(u32::MAX / 4),
            id: IdFull::new(UNKNOWN_PEER_ID, 0, 0),
            // ...
        });
        // ...
    }
}
```

### Checkout Operation

Version switching is done by:

1. Computing the diff between current and target version vectors
2. "Retreating" operations not in target (marking as future, undoing deletes)
3. "Forwarding" operations in target (unmarking future, applying deletes)

```rust
fn _checkout(&mut self, vv: &VersionVector, on_diff_status: bool) {
    let current_vv = std::mem::take(&mut self.current_vv);
    let (retreat, forward) = current_vv.diff_iter(vv);
    let mut updates = Vec::new();
    
    for span in retreat {
        // Mark inserts as future, undo deletes
        for c in self.id_to_cursor.iter(span) {
            match c {
                IterCursor::Insert { leaf, id_span } => {
                    updates.push(LeafUpdate {
                        leaf,
                        id_span,
                        set_future: Some(true),
                        delete_times_diff: 0,
                    });
                }
                IterCursor::Delete(span) => {
                    // Decrement delete_times for affected spans
                }
            }
        }
    }
    
    for span in forward {
        self.forward(span, &mut updates);
    }
    
    self.batch_update(updates, on_diff_status);
}
```

### Complexity: Time Travel

| Operation | Complexity |
|-----------|------------|
| Checkout to adjacent version | O(k log n) where k = operations changed |
| Checkout to distant version | O(m log n) where m = total operations between versions |
| Diff calculation | O(m log n) |

## Conflict Resolution Details

### Total Ordering of Concurrent Inserts

The ordering algorithm (from Fugue):

1. Compare `origin_left`:
   - If different, the one with leftward origin goes first
   
2. If same `origin_left`, compare `origin_right`:
   - If same, use peer ID for deterministic ordering
   - If different, compare positions of the origins
   
3. For concurrent operations with no common ancestor:
   - Use peer ID as final tiebreaker

### Delete Semantics

Loro uses delete counters instead of tombstones:

```rust
pub(super) struct Status {
    pub future: bool,
    pub delete_times: i16,  // Can go negative during version switching!
}
```

An element is visible when:
```rust
pub fn is_activated(&self) -> bool {
    self.delete_times == 0 && !self.future
}
```

This allows:
- Multiple deletes of the same element (idempotent)
- Undo/redo by adjusting counters
- Efficient version switching without tombstone cleanup

## Unique Innovations vs Other CRDTs

### vs Yjs (YATA)

| Aspect | Yjs | Loro |
|--------|-----|------|
| Algorithm | YATA | Fugue (improved YATA) |
| Interleaving | Can interleave | Minimizes interleaving |
| Rich text | Separate implementation | Integrated Peritext |
| Memory | Per-character tombstones | Span coalescing + delete counters |
| Time travel | Not built-in | First-class support |

### vs diamond-types

| Aspect | diamond-types | Loro |
|--------|---------------|------|
| Algorithm | Eg-walker | Fugue on Eg-walker |
| Data types | Text only | Text, Map, List, Tree |
| Rich text | Not supported | Peritext integration |
| API | Rust-focused | Multi-language (Rust, JS, Swift) |

### vs Cola

| Aspect | Cola | Loro |
|--------|------|------|
| Algorithm | Custom RGA variant | Fugue |
| Focus | Minimal implementation | Full-featured library |
| Rich text | Basic | Full Peritext |
| Move operations | Not supported | Supported |

### vs json-joy

| Aspect | json-joy | Loro |
|--------|----------|------|
| Language | TypeScript | Rust (WASM for JS) |
| Data structure | Splay tree | B-tree |
| Indexing | Dual index | Generic cache system |
| Performance | Good | Excellent (native) |

## Cursor Handling

### Sequential Access Optimization

From `richtext_state.rs`:

```rust
pub(super) fn record_cache(
    &mut self,
    leaf: LeafIndex,
    pos: usize,
    pos_type: PosType,
    entity_offset: usize,
    entity_index: Option<usize>,
) {
    // Cache the cursor position for future queries
    match &mut self.cached_cursor {
        Some(c) if c.leaf == leaf => {
            c.index.insert(pos_type, pos);
        }
        _ => {
            self.cached_cursor = Some(CachedCursor {
                leaf,
                index: FxHashMap::default(),
            });
            // ...
        }
    }
}
```

### Position Types

```rust
pub(crate) enum PosType {
    Entity,   // CRDT entity index
    Event,    // User-facing event index
    Unicode,  // Unicode codepoint index
    Utf16,    // UTF-16 code unit index
    Bytes,    // Byte offset
}
```

The system maintains cached conversions between these representations.

## State Representation

### Two-Level Architecture

1. **Container State** (e.g., `RichtextState`):
   - User-facing content
   - Style information (for rich text)
   - No CRDT metadata

2. **Tracker** (in `DiffCalculator`):
   - CRDT structure
   - Version vectors
   - ID-to-position mappings

### StyleRangeMap: Efficient Style Tracking

From `style_range_map.rs`:

```rust
pub(super) struct StyleRangeMap {
    pub(super) tree: BTree<RangeNumMapTrait>,
    has_style: bool,
}

pub(crate) struct Elem {
    pub(crate) styles: Styles,
    pub(crate) len: usize,
}

pub(crate) struct Styles {
    pub(crate) styles: FxHashMap<StyleKey, StyleValue>,
}
```

Styles are stored as ranges in a separate B-tree, with efficient:
- Range queries
- Intersection computation (for insertions between styled regions)
- Annotation updates

## Key Takeaways for Implementation

### 1. Use Fugue for Text Ordering

The Fugue algorithm with `origin_left` and `origin_right` provides:
- Maximal non-interleaving
- Deterministic ordering
- Efficient conflict resolution

### 2. Separate History from State

The OpLog/DocState separation enables:
- Fast time travel
- Efficient storage (state without full history)
- Incremental synchronization

### 3. Generic B-tree with RLE

The `generic-btree` pattern:
- Customizable cache for O(log n) queries
- RLE support for span coalescing
- Arena allocation for stability

### 4. Delete Counters, Not Tombstones

Using `delete_times` counter instead of tombstones:
- Allows efficient version switching
- Handles concurrent deletes correctly
- Enables undo/redo

### 5. Cache Sequential Access

Cursor caching for sequential access:
- Avoids redundant tree traversals
- Optimizes common editing patterns
- Multiple position type support

### 6. Lazy Materialization

The "unknown" placeholder pattern:
- Allows partial document loading
- Efficient for large documents
- Supports sparse collaboration

## Performance Characteristics

### Memory Usage

- ~64 bytes per CRDT span (can represent many characters)
- Efficient span coalescing reduces count
- Arena allocation reduces fragmentation

### Time Complexity Summary

| Operation | Complexity |
|-----------|------------|
| Insert character | O(log n) |
| Delete character | O(log n) |
| Insert at position | O(log n) |
| Position lookup | O(log n) amortized (O(1) with cache hit) |
| Merge operations | O(m log n) |
| Version checkout | O(k log n) |

### Space Complexity

| Structure | Space |
|-----------|-------|
| Document state | O(spans) where spans << characters |
| Operation log | O(operations) |
| ID-to-cursor map | O(operations) |
| Version vector | O(peers) |

## References

1. Weidner, M., Gentle, J., & Kleppmann, M. (2023). "The Art of the Fugue: Minimizing Interleaving in Collaborative Text Editing." https://arxiv.org/abs/2305.00583

2. Gentle, J., & Kleppmann, M. (2024). "Collaborative Text Editing with Eg-walker: Better, Faster, Smaller." https://arxiv.org/abs/2409.14252

3. Litt, G., et al. "Peritext: A CRDT for Rich Text Collaboration." https://www.inkandswitch.com/peritext/

4. Loro Documentation: https://loro.dev/docs

5. generic-btree repository: https://github.com/loro-dev/generic-btree

6. Loro source code: https://github.com/loro-dev/loro
