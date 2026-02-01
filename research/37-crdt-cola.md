+++
model = "claude-opus-4-5"
created = 2026-02-01
modified = 2026-02-01
driver = "Isaac Clayton"
+++

# Cola Deep Dive

## Overview

- Repository: https://github.com/nomad/cola
- Language: Rust
- Author: Riccardo Mazzarini (noib3)
- Published: September 2, 2023
- Primary innovations: Gtree (grow-only B-tree with vector storage), Anchor-based positioning, decoupled CRDT/buffer architecture

Cola is a text CRDT for real-time collaborative editing of plain text documents. It prioritizes practical performance while maintaining theoretical correctness. The key design decision is that cola does not store text content itself. It operates exclusively on positional metadata (Anchors, timestamps, lengths), allowing users to manage actual text content independently with any buffer implementation they choose.

## Data Structure

### Core Types

Cola's architecture consists of several interlocking components:

```rust
pub struct Replica {
    id: ReplicaId,                    // Unique peer identifier
    run_tree: RunTree,                // Primary data structure (Gtree of EditRuns)
    lamport_clock: LamportClock,      // Logical clock for ordering
    run_clock: RunClock,              // Local clock for insertion runs
    version_map: VersionMap,          // Last seen timestamps per replica
    deletion_map: DeletionMap,        // Deletion timestamps per replica
    backlog: Backlog,                 // Pending out-of-order operations
}
```

**ReplicaId**: A 64-bit identifier assigned to each peer. Unlike yjs (which uses random 53-bit client IDs) or diamond-types (which maps agent names to dense LVs), cola uses the ReplicaId directly throughout.

**Text**: Represents inserted content without storing the actual string:

```rust
pub struct Text {
    inserted_by: ReplicaId,
    range: Range<Length>,  // Temporal range, not spatial offset
}
```

This is a critical design choice. The `range` field refers to the character clock of the inserting replica before and after insertion. It has nothing to do with where the text was inserted spatially.

**EditRun**: The fundamental unit of document structure:

```rust
pub(crate) struct EditRun {
    text: Text,           // Who inserted and temporal range
    run_ts: RunTs,        // Insertion run timestamp
    lamport_ts: LamportTs, // Lamport timestamp for ordering
    is_deleted: bool,     // Tombstone flag
}
```

Consecutive insertions from the same peer with sequential temporal offsets compress into a single EditRun. This is similar to yjs's Item merging and diamond-types's span coalescing.

### The Gtree

The Gtree is cola's primary indexing structure. It is a grow-only tree that combines B-tree balancing with vector-based storage:

```rust
pub(crate) struct Gtree<const ARITY: usize, L: Leaf> {
    inodes: Vec<Inode<ARITY, L>>,  // Internal nodes
    lnodes: Vec<Lnode<L>>,          // Leaf nodes
    root_idx: InodeIdx,             // Root pointer
    cursor: Option<Cursor<L>>,      // Cached edit position
}

pub(crate) struct Inode<const ARITY: usize, L> {
    tot_len: Length,                    // Total length of subtree
    parent: InodeIdx,                   // Parent pointer
    num_children: usize,                // Current child count
    children: [NodeIdx; ARITY],         // Child indices
    has_leaves: bool,                   // Leaf vs internal children
}
```

Key properties:
- ARITY = 32 (branching factor)
- Average occupancy: approximately 20 children per inode
- All nodes stored in contiguous vectors, indices replace pointers
- Bidirectional traversal via parent pointers
- Grow-only: nodes are never removed, only marked deleted

The vector-based storage solves Rust's ownership constraints without unsafe code. As the author explains: "If you tried to implement this in safe Rust you'd quickly end up with a bunch of `Rc<RefCell<_>>`s everywhere." By using indices instead of pointers, the vector owns all nodes and navigation remains safe.

### Anchor System

Anchors provide stable position references that survive concurrent edits:

```rust
pub(crate) struct InnerAnchor {
    replica_id: ReplicaId,    // Who created the anchored content
    contained_in: RunTs,      // Which EditRun contains this position
    offset: Length,           // Temporal offset within that run
}

pub struct Anchor {
    inner: InnerAnchor,
    bias: AnchorBias,  // Left or Right
}
```

Anchors are conceptually similar to yjs's `origin` and `rightOrigin` but with a different structure. While yjs stores the left/right neighbors at insertion time, cola stores a reference into a specific EditRun. The `run_ts` field identifies which insertion run contains the anchor, enabling O(log f) lookup where f is the number of fragments that run has been split into.

### RunIndices: Secondary Index

The RunIndices structure provides fast anchor-to-leaf resolution:

```rust
pub(crate) struct RunIndices {
    map: ReplicaIdMap<ReplicaIndices>,
}

pub(crate) struct ReplicaIndices {
    vec: Vec<(Fragments, Length)>,  // Indexed by RunTs
}

pub(crate) enum Fragments<const INLINE: usize> {
    Array(Array<INLINE>),           // First 8 fragments inline
    Gtree(Gtree<INLINE, Fragment>), // Falls back to tree
}
```

This structure maps (ReplicaId, RunTs, offset) to the LeafIdx of the containing EditRun. The key insight is using RunTs as a direct array index, avoiding binary search for run identification. When an EditRun is split (by concurrent inserts or deletions), new Fragments are added to track each piece.

### Memory Layout

Cola uses a flat memory model with indices:

```
inodes: [Inode0, Inode1, Inode2, ...]
lnodes: [Lnode0, Lnode1, Lnode2, ...]

Each Inode contains child indices (not pointers) into either inodes or lnodes.
Each Lnode contains parent index back to its parent Inode.
```

This layout provides:
- Cache-friendly sequential access during traversal
- Simple serialization (just write the vectors)
- Safe Rust without RefCell overhead
- Stable indices that remain valid as the tree grows

The tradeoff: deletions cannot reclaim memory. Deleted content is tombstoned but never removed.

## Merge Algorithm

### Ordering Rules

Cola uses Lamport timestamps as the primary ordering mechanism for concurrent insertions:

```rust
impl Ord for EditRun {
    fn cmp(&self, other: &Self) -> Ordering {
        // Primary: descending Lamport timestamp (later insertions first)
        // Secondary: ascending ReplicaId (arbitrary but deterministic)
        self.lamport_ts
            .cmp(&other.lamport_ts)
            .reverse()
            .then(self.replica_id().cmp(&other.replica_id()))
    }
}
```

This differs from yjs and diamond-types, which use the YATA algorithm with dual origins. Cola's approach is simpler: all insertions at the same anchor are ordered purely by their Lamport timestamp and ReplicaId.

When integrating a remote insertion:

```rust
// From run_tree.rs merge_insertion
let run = EditRun::from_insertion(insertion);

if insertion.anchor().is_zero() {
    return self.insert_run_at_zero(run);
}

let anchor_idx = self.run_indices.idx_at_anchor(insertion.anchor(), AnchorBias::Left);
let anchor = self.gtree.leaf(anchor_idx);

// If anchor is in the middle of a run, split and insert
if insertion.anchor().offset() < anchor.end() {
    let insert_at = insertion.anchor().offset() - anchor.start();
    return self.split_run_with_another(run, anchor_idx, insert_at);
}

// Scan through potential conflicts, ordering by (lamport_ts desc, replica_id asc)
for (idx, leaf) in self.gtree.leaves::<false>(anchor_idx) {
    if run > *leaf {
        prev_idx = idx;
    } else {
        return self.insert_run_after_another(run, prev_idx);
    }
}
```

### Conflict Resolution

Cola's conflict resolution is simpler than YATA:

1. Find the anchor point (the position in the document where the insertion should go)
2. Scan forward from the anchor, comparing Lamport timestamps
3. Insert when we find a run with lower priority (lower Lamport ts, or same ts with higher ReplicaId)

The simplicity comes from not needing dual origins. Cola does not track right origins; it relies purely on the total ordering of (Lamport, ReplicaId).

### Deletion Handling

Deletions use version vectors to ensure correctness:

```rust
pub struct Deletion {
    start: Anchor,           // Start of deleted range
    end: Anchor,             // End of deleted range
    version_map: VersionMap, // Sender's view of all replicas at deletion time
    deletion_ts: DeletionTs, // Ordering timestamp for this deletion
}
```

When integrating a deletion:

1. Check version_map: can we merge this deletion?
   - We must have seen all insertions the deleting peer had seen
   - This prevents deleting content we have not yet received

2. Convert start/end anchors to leaf indices

3. Walk the range, marking EditRuns as deleted:

```rust
// Simplified from run_tree.rs merge_deletion
for run in runs_in_range {
    if run.end() > deletion.version_map.get(run.replica_id()) {
        // This run extends past what the deleter saw
        // Only delete the portion they could see
        let delete_up_to = deletion.version_map.get(run.replica_id()) - run.start();
        self.delete_leaf_range(run_idx, 0..delete_up_to);
    } else {
        // Delete entire run
        run.delete();
    }
}
```

This approach prevents a common CRDT bug: deleting content that was inserted concurrently and not visible to the deleter.

### Complexity Analysis

**Time Complexity:**

| Operation | Complexity | Notes |
|-----------|------------|-------|
| Local insert | O(log n) | Tree traversal + O(log n) ancestor updates |
| Local insert (cached) | O(1) amortized | Cursor cache enables O(1) extension |
| Local delete | O(log n + d) | d = size of deleted range |
| Remote insert | O(log f + log n) | f = fragments of anchor run |
| Remote delete | O(log f + k) | k = runs in deleted range |
| Position lookup | O(log n) | Top-down tree traversal |
| Anchor resolution | O(log f + log n) | Index lookup + tree traversal |

**Space Complexity:**

- Per EditRun: approximately 56 bytes (Text + timestamps + flags)
- Per character (typical): 10-20 bytes amortized with RLE
- Memory is grow-only: deleted content is tombstoned, not freed

Real-world benchmark on automerge trace (260k edits):
- Approximately 15k EditRuns created
- Well within 4-level tree capacity (160k runs at ARITY=32)

## Optimizations

### Run-Length Encoding

Consecutive insertions from the same peer compress into a single EditRun:

```rust
// From run_tree.rs insert
if run.len() == offset
    && run.end() == text.start()
    && run.replica_id() == text.inserted_by()
    && run.lamport_ts() == lamport_clock.highest()
{
    // Extend existing run instead of creating new one
    run.extend(text.len());
    return (None, None);
}
```

This dramatically reduces memory usage. Pasting a 107k-character document creates one EditRun, not 107k.

### Cursor Caching

The Gtree maintains a cached cursor for the last edit position:

```rust
struct Cursor<L: Leaf> {
    leaf_idx: LeafIdx<L>,   // Current leaf
    offset: Length,         // Offset from start
    child_idx: ChildIdx,    // Position in parent
}
```

When inserting at or near the cached position, cola skips the O(log n) tree traversal:

```rust
// From gtree.rs insert
if let Some(cursor) = self.cursor {
    let cursor_end = cursor.offset + self.leaf(cursor.leaf_idx).len();
    if offset > cursor.offset && offset <= cursor_end {
        return self.insert_at_leaf(cursor.leaf_idx, ...);
    }
}
```

This makes sequential typing O(1) amortized instead of O(log n) per character.

### Inline Fragment Storage

The Fragments enum uses small-vector optimization:

```rust
pub(crate) enum Fragments<const INLINE: usize> {
    Array(Array<INLINE>),           // First 8 fragments stored inline
    Gtree(Gtree<INLINE, Fragment>), // Falls back to tree for more
}
```

Most EditRuns are never fragmented or only split a few times. Storing the first 8 fragments inline avoids tree allocation for the common case.

### Backlog for Out-of-Order Delivery

Operations that arrive out of order are stored in a backlog:

```rust
pub(crate) struct Backlog {
    insertions: ReplicaIdMap<InsertionsBacklog>,
    deletions: ReplicaIdMap<DeletionsBacklog>,
}
```

When new operations arrive, previously-blocked operations are checked:

```rust
pub fn backlogged_insertions(&mut self) -> BackloggedInsertions<'_> {
    // Iterates over insertions that are now ready to merge
}
```

This enables cola to work with unreliable networks where messages may arrive out of order.

### Compact Wire Format

Cola uses LEB128 encoding and exploits common patterns:

```rust
// Insertions that continue existing runs omit the anchor
enum InsertionRun {
    BeginsNew,           // Must encode full anchor
    ContinuesExisting,   // Anchor derived from text + run_ts
}

// Deletions with same-replica anchors compress the encoding
enum AnchorsFlag {
    DifferentReplicaIds,    // Encode both fully
    SameReplicaId,          // Share replica_id
    SameReplicaIdAndRunTs,  // Share replica_id and run_ts, encode only length
}
```

This produces 3-7x smaller payloads compared to naive serialization.

## Code Walkthrough

### Inserting Text Locally

```rust
// Replica::inserted
pub fn inserted(&mut self, at_offset: Length, len: Length) -> Insertion {
    // 1. Advance version map
    let start = self.version_map.this();
    *self.version_map.this_mut() += len;
    let end = self.version_map.this();

    // 2. Create Text (metadata only, no actual string)
    let text = Text::new(self.id, start..end);

    // 3. Insert into run_tree, get anchor
    let anchor = self.run_tree.insert(
        at_offset,
        text.clone(),
        &mut self.run_clock,
        &mut self.lamport_clock,
    );

    // 4. Return insertion for network transmission
    Insertion::new(anchor, text, self.lamport_clock.highest(), self.run_clock.last())
}
```

### Integrating Remote Insertion

```rust
// Replica::integrate_insertion
pub fn integrate_insertion(&mut self, insertion: &Insertion) -> Option<Length> {
    // 1. Check if already merged
    if self.has_merged_insertion(insertion) { return None; }

    // 2. Check if we can merge (have anchor, sequential clock)
    if self.can_merge_insertion(insertion) {
        Some(self.merge_unchecked_insertion(insertion))
    } else {
        // 3. Backlog for later
        self.backlog.insert_insertion(insertion.clone());
        None
    }
}

// Merge preconditions
fn can_merge_insertion(&self, insertion: &Insertion) -> bool {
    // Must be next in sequence for this replica
    self.version_map.get(insertion.inserted_by()) == insertion.start()
    // Must have the anchor content
    && self.has_anchor(insertion.anchor())
}
```

### RunTree Merge

```rust
// RunTree::merge_insertion (simplified)
pub fn merge_insertion(&mut self, insertion: &Insertion) -> Length {
    let run = EditRun::from_insertion(insertion);

    // 1. Find anchor position
    let anchor_idx = self.run_indices.idx_at_anchor(insertion.anchor(), AnchorBias::Left);
    let anchor = self.gtree.leaf(anchor_idx);

    // 2. Check if we can append to anchor (RLE extension)
    if anchor.can_append(&run) {
        return self.append_run_to_another(run, anchor_idx);
    }

    // 3. Scan for correct insertion point
    let mut prev_idx = anchor_idx;
    for (idx, leaf) in self.gtree.leaves::<false>(anchor_idx) {
        if run > *leaf {  // run has higher priority
            prev_idx = idx;
        } else {
            break;
        }
    }

    // 4. Insert and update indices
    self.insert_run_after_another(run, prev_idx)
}
```

## Comparison with Yjs and Diamond-types

### Structural Comparison

| Aspect | Yjs | Diamond-types | Cola |
|--------|-----|---------------|------|
| Language | JavaScript | Rust | Rust |
| Content storage | Inline in Items | Separate JumpRope | External (user-managed) |
| Item storage | Doubly-linked list | B-tree (ContentTree) | B-tree (Gtree) |
| Position lookup | O(1) with markers | O(log n) | O(log n) with cursor cache |
| ID structure | (client, clock) | Local Version (LV) | (ReplicaId, temporal range) |
| Conflict resolution | YATA with dual origins | YATA with dual origins | Lamport + ReplicaId |

### Key Differences

**1. Content Decoupling**

Cola is unique in not storing text content at all. Yjs stores content inline in Items; diamond-types stores it in JumpRope. Cola only tracks metadata, returning offsets for the user to apply to their own buffer.

Tradeoff: simpler CRDT, but users must maintain buffer synchronization manually.

**2. No Dual Origins**

Yjs and diamond-types both use YATA's dual-origin approach (origin_left and origin_right). Cola uses only a single anchor plus Lamport timestamps.

From the source:
```rust
// Cola's ordering is purely timestamp-based
self.lamport_ts.cmp(&other.lamport_ts).reverse()
    .then(self.replica_id().cmp(&other.replica_id()))
```

vs yjs/diamond-types which scan based on origin relationships.

The practical difference: cola's merge is simpler but may produce different (though still correct) interleaving in some concurrent scenarios.

**3. Memory Model**

Diamond-types uses separate arrays for different concerns (operations, content, causal graph). Yjs uses a heap of linked objects. Cola uses contiguous vectors with indices.

Cola's approach provides:
- Better cache locality than yjs
- Simpler structure than diamond-types
- But no garbage collection of deleted content

**4. Time Travel**

Diamond-types has first-class time travel with advance/retreat operations. Yjs can replay from StructStore. Cola has no built-in time travel; the grow-only structure would require full replay.

### Performance Comparison

From cola's benchmarks on character-by-character editing traces:

| Direction | Cola | Diamond-types | Automerge | Yrs |
|-----------|------|---------------|-----------|-----|
| Upstream | 1x (baseline) | 1.4-2x slower | >100x slower | >100x slower |
| Downstream | 2x slower than upstream | crashed on traces | >100x slower | >100x slower |

Note: These benchmarks only measure CRDT operations, not text buffer manipulation. Real-world performance depends on buffer implementation.

### Limitations Compared to Others

1. **No rich text**: Cola is plain text only. Yjs has YText with formatting; Loro has Peritext-based rich text.

2. **No undo/redo**: The grow-only structure makes undo complex. Diamond-types tracks causality for this.

3. **No garbage collection**: Memory grows monotonically. Long-running documents accumulate tombstones.

4. **No WASM-first design**: Unlike yjs, cola was not designed for browser integration.

## Lessons for Our Implementation

### What to Adopt

1. **Anchor-based positioning**: Cola's anchor system is elegant. Storing (replica_id, run_ts, offset) provides stable references that survive concurrent edits. We should consider a similar approach.

2. **Content decoupling**: Not storing content in the CRDT itself is powerful. It allows the CRDT to focus purely on ordering while the user chooses their optimal buffer structure.

3. **Vector-based tree storage**: Using indices instead of pointers solves Rust ownership cleanly. This pattern could simplify our implementation.

4. **Cursor caching**: The cursor optimization for sequential edits is simple and effective. We should implement something similar.

5. **Inline small-vector for fragments**: The `Fragments` enum pattern (inline array that spills to tree) is a good optimization for the common case.

6. **Version vector for deletion safety**: Including the sender's version map in deletions prevents the "delete unseen content" bug.

### What to Consider Differently

1. **Simpler ordering vs YATA**: Cola's Lamport-only ordering is simpler but may produce different interleavings than YATA. We should evaluate whether this matters for our use cases. YATA's dual origins provide more predictable interleaving in certain concurrent scenarios.

2. **No garbage collection**: For long-running documents, this could be problematic. We may want to support tombstone pruning like diamond-types.

3. **No time travel**: If we need per-keystroke replay or branching, we should design that in from the start rather than trying to add it later.

4. **Single-anchor vs dual-origin**: Cola's single anchor is simpler but provides less information for conflict resolution. The YATA approach with dual origins handles some edge cases more predictably.

### Primitives to Extract

From cola's implementation, useful reusable primitives include:

- **Gtree**: A grow-only B-tree with vector storage, adaptable for various leaf types
- **RunIndices**: Secondary index pattern for fast anchor resolution
- **Fragments**: Small-vector with tree fallback
- **VersionMap/DeletionMap**: Efficient version vector with local replica optimization
- **Backlog**: Out-of-order operation buffering

## Sources

- [Cola Blog Post](https://nomad.foo/blog/cola) - Riccardo Mazzarini's deep dive
- [Cola GitHub](https://github.com/nomad/cola) - Source code
- [Cola docs.rs](https://docs.rs/cola-crdt/latest/cola/) - API documentation
- [Hacker News Discussion](https://news.ycombinator.com/item?id=37373796) - Community feedback
- Source code analysis of `/tmp/cola/src/` - gtree.rs, replica.rs, run_tree.rs, etc.
