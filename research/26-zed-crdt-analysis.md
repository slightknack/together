# Zed CRDT Analysis

## Architecture

Zed's collaborative editing stack consists of multiple crates:

1. **`sum_tree`** - A concurrent B-tree data structure for indexing
2. **`rope`** - Text rope built on sum_tree (general text storage)
3. **`text`** - The actual CRDT implementation (Buffer, anchors, operations)

The rope crate alone is NOT the CRDT - it's just the underlying text storage.
The CRDT logic (replica IDs, tombstones, vector clocks) is in the `text` crate.

## Key CRDT Concepts (from blog post)

### Anchors
- Stable logical references using (insertion_id, offset) pairs
- Don't rely on absolute byte positions
- Survive concurrent edits

### Unique ID Generation
- Centrally-assigned replica IDs + incrementing sequence numbers
- Example: replica 0 generates IDs like `0.0`, `0.1`, `0.2`

### Immutable Insertion History
- Text insertions are never modified
- Deletions use tombstones (markers)
- Enables correct concurrent edit application

### Vector Timestamps for Deletions
- Version vectors encode latest observed sequence per replica
- Prevents concurrent insertions from being hidden

### Lamport Timestamps for Ordering
- Concurrent insertions at same location ordered by Lamport timestamp
- Respects causality for user intent

### Copy-on-Write B-Tree
- Indexes fragments for efficient lookups
- Likely refers to sum_tree

## Benchmark Clarification

My earlier "Zed rope" benchmark (62-384x slower than Together) was **misleading**:
- It only benchmarked the rope crate (plain text operations)
- Did NOT benchmark the actual CRDT (text crate)
- Zed's rope uses `replace()` which is expensive for single-char edits
- A fair comparison would require extracting the full text crate

## Difficulty of Extraction

The text crate has deep dependencies:
- `clock` - Lamport/vector clocks
- `collections` - Custom HashMap/HashSet
- `postage` - Async channels
- `util` - Various utilities
- Plus rope and sum_tree

Extracting for standalone benchmark would require significant effort.

## Comparison to Together

| Aspect | Zed | Together |
|--------|-----|----------|
| Tree structure | sum_tree (B-tree) | BTreeList (weighted B-tree) |
| Text storage | Rope (tree of chunks) | Per-user columns (Vecs) |
| Anchors | (insertion_id, offset) | (user_idx, seq) |
| Tombstones | Yes | Yes (deleted flag on spans) |
| Coalescing | Unknown | Yes (adjacent spans merge) |
| Cursor cache | Unknown | Yes (O(1) sequential typing) |

## Potential Insights

1. **Copy-on-write B-tree** - Might enable better versioning
2. **Rope structure** - Could help with very large documents
3. **Anchor design** - Similar to Together's approach
