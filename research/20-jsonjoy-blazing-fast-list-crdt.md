+++
model = "claude-opus-4-5"
created = 2026-01-31
modified = 2026-01-31
driver = "Isaac Clayton"
source = "https://jsonjoy.com/blog/performant-rga-list-crdt-algorithm"
+++

# json-joy: Blazing Fast List CRDT

## Overview

json-joy implements a novel Block-wise RGA CRDT algorithm that claims to be approximately 100x faster than state-of-the-art JavaScript list CRDT libraries, and even 10x faster than native V8 JavaScript strings.

## Key Algorithm: Block-wise RGA

### Core Concept

Instead of storing each character individually, json-joy stores text in chunks (blocks). This is called "Block-wise RGA" - a modification to the original RGA algorithm that fits better into scenarios where entire ranges of elements are inserted and deleted.

```
Traditional RGA: Each character = 1 node
Block-wise RGA:  Consecutive characters with consecutive timestamps = 1 block
```

### How Block Storage Works

RGA nodes support block-wise internal representation where consecutive elements are stored in a single block if:
1. The logical timestamps of consecutive elements are consecutive
2. The session IDs are the same
3. The sequence numbers are consecutively incremented

When a new list of elements is inserted:
- First element gets the ID of the operation that inserted the list
- Subsequent elements get consecutive logical timestamps (same session ID, incrementing sequence numbers)

### Metadata Efficiency

When serialized, JSON CRDT documents store a single logical timestamp per block. Each logical timestamp consumes on average 2-3 bytes of storage space. This is highly efficient compared to per-character metadata.

## Data Structure: Dual Splay Trees

json-joy uses a sophisticated tree structure with two sets of pointers per node:

1. **Spatial Tree**: In-order traversal gives the document layout
2. **Temporal Tree**: In-order traversal gives the editing history

Both are implemented as **Splay trees**, which are "self-optimizing" because they rotate recently accessed nodes to the root.

### Benefits of Splay Trees

1. **Optimized for common case**: "Inserting or deleting right at the last place I inserted or deleted" is O(1) amortized
2. **Efficient dual traversal**: Can navigate by document position OR by temporal ID
3. **Rope-like behavior**: Each node maintains a "length" (its span plus children's spans), so navigating the spatial tree is equivalent to navigating a rope

### Complexity

The specification notes it is possible to implement RGA such that all local and remote operations take no more than O(log n) time.

## Performance Optimizations

### 1. Fast Text Diff Algorithm

json-joy implements a fast text diff algorithm for computing patches between states.

### 2. Fast-Path Optimizations

Various fast-path optimizations for common cases:
- Single character typing (most common operation)
- Sequential insertions at same position
- Backspace/delete sequences

### 3. Block Coalescing

The automerge-paper trace (259,778 single character insert/delete transactions, final document 104,852 bytes) results in only 12,387 json-joy RGA blocks - about 21x compression ratio.

## Comparison: RGA vs YATA

| Aspect | RGA (json-joy, Automerge) | YATA (Y.js, Y.rs) |
|--------|---------------------------|-------------------|
| Insert reference | Single (character after) | Double (before + after) |
| Complexity | Simpler | More complex |
| Tie-breaking | Different approach | Different approach |

Both algorithms use logical clocks to timestamp operations.

## Key Insights for Together

### Already Implemented
- **Span coalescing**: We already coalesce adjacent spans (79-91% rate)
- **Chunked storage**: We use chunked WeightedList

### Could Implement
1. **Splay tree consideration**: Splay trees self-optimize for access patterns
   - Our skip list provides O(log n) but without the "hot path" optimization
   - Splay trees excel when there's locality in access patterns

2. **Dual tree pointers**: Having both spatial and temporal indices
   - Currently we have only spatial (position-based)
   - Temporal index could speed up merge operations

3. **Per-block gap buffer**: json-joy stores content within blocks
   - We store content in separate per-user columns
   - Inline content could improve cache locality

## Performance Claims

- 100x faster than other JavaScript CRDT libraries
- 10x faster than native V8 strings
- 5M transactions per second on single thread
- Faster than Rope.js (specialized fast string library)

## References

- Source: https://jsonjoy.com/blog/performant-rga-list-crdt-algorithm
- json-joy GitHub: https://github.com/streamich/json-joy
- JSON CRDT Spec: https://jsonjoy.com/specs/json-crdt
