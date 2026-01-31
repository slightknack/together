---
model = "claude-opus-4-5"
created = "2026-01-31"
modified = "2026-01-31"
driver = "Isaac Clayton"
---

# Optimization Lessons from RGA Performance Work

## Summary

Optimized the RGA implementation from 347x slower than diamond-types to 4.5x slower, a 77x speedup.

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

## What Would Close the Remaining 4.5x Gap

To match diamond-types performance, would likely need:

1. B-tree or rope structure for true O(log n) with good constants
2. Span coalescing to reduce fragmentation  
3. More sophisticated caching (cursor + chunk + weight prefix sums)
4. Careful memory layout optimization
5. Possibly SIMD for weight summation

Diamond-types is highly optimized production code. Getting within 4.5x with straightforward Rust is a reasonable result.

### Additional Optimizations Applied

After initial chunked implementation:

1. Made `Span` and `ItemId` Copy to avoid heap allocations on clone
2. Combined weight and item counting in `find_chunk_by_weight` to avoid double iteration
3. Removed unnecessary clone calls throughout

Final performance:
- Insert: 217ns avg
- Delete: 641ns avg  
- Total: 6ms for 19,749 patches (vs 1.4ms for diamond-types)

## Applicable General Tips

1. Profile first, optimize second
2. Test asymptotic claims with real workloads
3. Consider "unrolled" chunked structures as a middle ground
4. Measure fragmentation and span counts during development
5. Chunk sizes around sqrt(n) are often optimal
6. Cache locality often beats algorithmic complexity
7. Simple approaches with good constants beat complex approaches with overhead
