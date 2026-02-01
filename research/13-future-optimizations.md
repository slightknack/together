+++
model = "claude-opus-4-5"
created = 2026-01-31
modified = 2026-01-31
driver = "Isaac Clayton"
+++

# Future Optimization Opportunities

## Current State

We beat diamond-types on 3/4 benchmarks. The remaining gap is on automerge-paper (260k patches), where we're 1.77x slower. This document catalogs future optimization opportunities.

## High Impact (Would Help automerge-paper)

### 1. B-Tree for Spans (Instead of Chunked List)

**Current**: WeightedList uses chunked Vec with Fenwick tree for O(log n) chunk lookup, but O(sqrt n) within-chunk operations.

**Proposed**: Replace with a proper B-tree where each node stores:
- Children (for internal nodes) or spans (for leaves)
- Cumulative weight of subtree
- Cumulative item count of subtree

**Expected gain**: O(log n) for all operations instead of O(sqrt n) within chunks.

**Complexity**: High. Would require significant refactoring.

**Reference**: Diamond-types' ContentTree in `ost/content_tree.rs`

### 2. Gap Buffer for Content Storage

**Current**: Content stored in per-user append-only columns. Reading content requires offset arithmetic.

**Proposed**: Store content in gap buffers within skip list/B-tree nodes, like JumpRope.

```rust
struct GapBuffer<const LEN: usize = 392> {
    data: [u8; LEN],
    gap_start: u16,
    gap_len: u16,
}
```

**Benefits**:
- O(1) sequential inserts (just extend gap)
- Better cache locality (content near metadata)
- No offset indirection

**Expected gain**: 1.2-1.5x for sequential typing patterns.

**Reference**: JumpRope's `gapbuffer.rs`

### 3. Delete Buffering in RgaBuf

**Current**: RgaBuf only buffers inserts. Deletes flush the buffer and apply immediately.

**Proposed**: Buffer adjacent deletes too:

```rust
enum PendingOp {
    Insert { user_idx: u16, pos: u64, content: SmallVec<[u8; 32]> },
    Delete { start: u64, len: u64 },
}
```

Adjacent deletes at positions P, P-1, P-2 (backspace) or P, P, P (forward delete) can be merged.

**Expected gain**: 1.1-1.2x on traces with many deletes.

### 4. Lazy Fenwick Tree Updates

**Current**: Fenwick tree updated on every operation.

**Proposed**: Batch Fenwick updates using delta accumulation:

```rust
struct LazyFenwick {
    tree: FenwickTree,
    pending_deltas: Vec<(usize, i64)>,  // (index, delta) pairs
}
```

Flush pending deltas only when querying or when batch is large.

**Expected gain**: 1.1x for sequences of operations in same region.

## Medium Impact

### 5. SIMD for Chunk Scanning

**Current**: Linear scan within chunks uses scalar comparisons.

**Proposed**: Use SIMD to scan multiple weights in parallel:

```rust
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

fn find_by_weight_simd(weights: &[u64], target: u64) -> usize {
    // Load 4 weights at a time, compare with target
    // Return first index where cumulative sum exceeds target
}
```

**Expected gain**: 1.2-1.5x for chunk operations, but only affects within-chunk work.

### 6. Arena Allocation for Spans

**Current**: Spans stored in Vec within chunks, which may reallocate.

**Proposed**: Use a typed arena allocator:

```rust
struct SpanArena {
    chunks: Vec<Box<[Span; 64]>>,
    next_slot: usize,
}
```

**Benefits**:
- No reallocation during inserts
- Stable references
- Better cache behavior

**Expected gain**: 1.1x by reducing allocator pressure.

### 7. Span Pooling for Deletes

**Current**: Deleted spans remain in memory with `deleted: true`.

**Proposed**: Pool deleted spans for reuse:

```rust
struct SpanPool {
    free_list: Vec<usize>,  // Indices of deleted spans
}
```

When inserting, check pool first before allocating new span.

**Expected gain**: Minimal for benchmarks, but reduces memory fragmentation for long sessions.

### 8. Content Deduplication

**Current**: Each insert stores its content in the user's column.

**Proposed**: Hash content and deduplicate:

```rust
struct ContentStore {
    chunks: Vec<Vec<u8>>,
    hash_to_chunk: HashMap<u64, (usize, usize)>,  // hash -> (chunk_idx, offset)
}
```

**Expected gain**: Reduces memory 2-10x for repetitive content, but adds hashing overhead.

## Low Impact (Polish)

### 9. Prefetch Hints

Add prefetch instructions before accessing span data:

```rust
#[inline]
fn prefetch_span(chunk: &Chunk, idx: usize) {
    unsafe {
        std::arch::x86_64::_mm_prefetch(
            chunk.items.as_ptr().add(idx) as *const i8,
            std::arch::x86_64::_MM_HINT_T0
        );
    }
}
```

### 10. Branch Prediction Hints

Use likely/unlikely macros for common paths:

```rust
#[cold]
fn handle_rare_case() { ... }

if likely(cache.is_valid()) {
    // fast path
} else {
    handle_rare_case();
}
```

### 11. Custom Hash for UserTable

Replace HashMap with a faster hash function for KeyPub lookups:

```rust
use rustc_hash::FxHashMap;
type UserMap = FxHashMap<KeyPub, u16>;
```

### 12. Reduce CursorCache Size

Current cache stores multiple fields. Could compress:

```rust
// Current: 40+ bytes
struct CursorCache {
    pos: u64,
    span_idx: usize,
    offset_in_span: u64,
    chunk_idx: usize,
    idx_in_chunk: usize,
}

// Compact: 16 bytes
struct CompactCache {
    pos: u32,
    span_idx: u32,
    offset_and_chunk: u32,  // packed
    idx_in_chunk: u16,
    valid: bool,
}
```

## Architecture Changes

### 13. Separate Text and CRDT Layers

**Current**: Spans store both CRDT metadata and content references.

**Proposed**: Split into two structures like diamond-types:

```rust
struct Document {
    text: JumpRope,           // Fast text operations
    crdt: ContentTree<Span>,  // CRDT ordering
}
```

**Benefits**:
- Each structure optimized for its purpose
- Text layer can use gap buffers
- CRDT layer focuses on ordering

**Complexity**: Very high. Major architectural change.

### 14. Async/Parallel Merge

**Current**: Merge operations are synchronous.

**Proposed**: For large merges, process spans in parallel:

```rust
async fn merge_parallel(&mut self, other: &Rga) {
    let spans: Vec<_> = other.spans.iter().collect();
    let results = futures::future::join_all(
        spans.chunks(1000).map(|chunk| self.process_chunk(chunk))
    ).await;
    self.apply_results(results);
}
```

**Benefits**: Faster sync for large document merges.

## Prioritized Recommendation

For closing the automerge-paper gap (1.77x -> 1.0x), focus on:

1. **B-Tree for Spans** - Biggest algorithmic improvement
2. **Delete Buffering** - Quick win for delete-heavy traces
3. **Gap Buffer** - Better sequential insert performance

These three together could realistically achieve parity with diamond-types on automerge-paper.

## Benchmarking Strategy

Before implementing, profile automerge-paper to identify:
1. What percentage of time is in inserts vs deletes?
2. How much time in chunk operations vs Fenwick operations?
3. Memory access patterns (cache misses)?

Use `cargo flamegraph` or `perf` to get accurate profiles.
