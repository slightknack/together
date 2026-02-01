+++
model = "claude-opus-4-5"
created = 2026-01-31
modified = 2026-01-31
driver = "Isaac Clayton"
+++

# Optimization Lessons from RGA Performance Work

## Summary

Optimized the RGA implementation from 347x slower than diamond-types to 2.3x slower, a **150x speedup**.

### Optimization Progression

| Optimization | Time | Ratio vs Diamond | Speedup |
|-------------|------|------------------|---------|
| Baseline (Vec) | 465ms | 347x slower | - |
| Chunked weighted list | 6.8ms | 5.1x slower | 68x |
| Span coalescing | 3.25ms | 2.8x slower | 1.9x |
| Combined origin/insert lookup | 2.66ms | 2.3x slower | 1.22x |
| **Total** | 2.66ms | 2.3x slower | **150x** |

## Key Lessons

### 1. Measure Before Optimizing

Before any optimization work, establish a baseline benchmark. The sveltecomponent trace with 19,749 patches showed:
- Initial: 470ms (385x slower than diamond-types at 1.2ms)
- After fixes: 465ms (347x slower)
- Final: 6.8ms (5.2x slower)

Profile to understand where time is spent. Our profiling showed:
- Insert: 17,786 ops, 320ms total, 18us average
- Delete: 3,227 ops, 72ms total, 22us average

82% of time was in inserts. Each operation averaged 18-22us, vs diamond-types at ~66ns per operation.

### 2. Theoretical Complexity Can Be Misleading

Skip list (O(log n)) vs Vec (O(n)) analysis suggested 1384x theoretical speedup. In practice:
- Skip list implementation was 3x *slower* than naive Vec (1033x vs 347x)
- Overhead of pointer chasing, random height generation, and multi-level updates outweighed algorithmic gains
- Cache locality matters more than asymptotic complexity for small-to-medium n

The crossover point where O(log n) beats O(n) depends heavily on constants. For ~20k items, O(n) with good constants wins.

### 3. Chunked Data Structures Are a Sweet Spot

The winning approach was a chunked list: Vec of chunks, each chunk a small Vec.

- O(sqrt(n)) complexity for all operations
- Chunks of 64 items gave best performance
- Linear scan across ~300 chunks is cache-friendly
- Small Vec operations within chunks are fast

This "unrolled" approach appears in many high-performance systems:
- Rope data structures use chunks
- B-trees are essentially chunked search trees
- Database pages are chunked storage

### 4. Fragmentation Kills Performance

Analysis showed 19,884 spans after 19,749 operations, nearly 1 span per operation. This happened because:
- Each edit at a non-sequential position creates a new span
- Deletes split existing spans into multiple pieces
- No span coalescing was happening

With 20k spans, each Vec::insert/remove was moving ~10k elements on average.

### 5. Cursor Caching Has Modest Benefits

Adding a cursor cache (remembering last lookup position) gave 15% improvement. Text editing has locality, but:
- Benefits are limited when structure is highly fragmented
- Cache invalidation on insert/remove reduces hit rate
- Worth doing but not transformative

### 6. Chunk Size Tuning Matters

Tested chunk sizes from 32 to 512:

| Chunk Size | Ratio |
|------------|-------|
| 32         | 6.7x  |
| 48         | 5.8x  |
| 64         | 5.1x  |
| 80         | 5.5x  |
| 128        | 6.3x  |
| 256        | 10.8x |
| 512        | 20.5x |

Optimal was around 64 items per chunk. This balances:
- Too small: too many chunks to scan
- Too large: expensive insert/remove within chunk

For this workload, sqrt(20000) = 141, so ~64-128 is in the right ballpark.

### 7. Binary Search Can Be Slower

Attempted binary search over chunk weights, but it was slower than linear scan because:
- Each binary search step required recomputing prefix sums O(n)
- Without cached prefix sums, binary search is O(n log n) total
- Linear scan with good cache behavior is O(n) with small constants

To make binary search work, would need to maintain a separate prefix sum array and keep it updated on modifications.

## What Would Close the Remaining 2.3x Gap

To match diamond-types performance, would likely need:

1. Skip list for true O(log n) lookups (vs O(sqrt n) chunked list)
2. Fenwick tree for O(log n) prefix sum queries over chunk weights
3. Smaller Span struct to reduce memory traffic
4. Gap buffer per chunk for sequential local edits

Diamond-types uses JumpRope: a skip list where each leaf node contains a gap buffer. This gives O(log n) navigation plus O(1) local edits.

### Optimizations Applied

1. **Chunked weighted list** (68x speedup): O(sqrt n) operations instead of O(n)
2. **Span coalescing** (1.9x speedup): 79% of inserts extend existing spans instead of creating new ones
3. **Combined origin/insert lookup** (1.22x speedup): Single find_by_weight call serves origin lookup, coalescing check, and insert position
4. Made `Span` and `ItemId` Copy to avoid heap allocations
5. Combined weight and item counting in `find_chunk_by_weight`

### Optimizations Attempted but Reverted

1. **Binary search over chunks**: Slower because each step recalculates prefix sums O(n)
2. **Inline hints**: No effect (compiler already inlined)
3. **Skip list**: 3x slower than Vec due to overhead for this workload size

Final performance:
- Total: 2.66ms for 19,749 patches (vs 1.16ms for diamond-types)
- 2.3x slower than diamond-types, down from 347x

## Applicable General Tips

1. Profile first, optimize second
2. Test asymptotic claims with real workloads
3. Consider "unrolled" chunked structures as a middle ground
4. Measure fragmentation and span counts during development
5. Chunk sizes around sqrt(n) are often optimal
6. Cache locality often beats algorithmic complexity
7. Simple approaches with good constants beat complex approaches with overhead
