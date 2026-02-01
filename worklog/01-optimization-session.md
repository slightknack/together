+++
model = "claude-opus-4-5"
created = 2026-01-31
modified = 2026-01-31
driver = "Isaac Clayton"
+++

# Optimization Session Log

## GOAL ACHIEVED: FASTER THAN DIAMOND-TYPES ON 3 OF 4 BENCHMARKS!

## Final Results (All 4 Benchmarks)

| Trace | Patches | Together | Diamond-types | Ratio | Status |
|-------|---------|----------|---------------|-------|--------|
| sveltecomponent | 19,749 | 1.09ms | 1.13ms | **0.96x = 4% FASTER** | WIN |
| rustcode | 40,173 | 2.57ms | 2.66ms | **0.97x = 3% FASTER** | WIN |
| seph-blog1 | 137,993 | 5.32ms | 6.54ms | **0.81x = 19% FASTER** | WIN |
| automerge-paper | 259,778 | 19.3ms | 10.9ms | 1.77x slower | (under 2x) |

**Goal: Faster than diamond-types on at least 3/4 benchmarks, no slower than 2x on any.**
**Result: 3/4 wins, worst case is 1.77x (under 2x limit).**

## Starting Point

| Trace | Together | Diamond-types | Ratio |
|-------|----------|---------------|-------|
| sveltecomponent | 2.59ms | 1.15ms | 2.3x slower |
| rustcode | 6.53ms | 2.72ms | 2.4x slower |
| seph-blog1 | 27.0ms | 6.57ms | 4.1x slower |

## Total Improvement

| Trace | Start | End | Speedup |
|-------|-------|-----|---------|
| sveltecomponent | 2.59ms | 1.09ms | **2.4x faster** |
| rustcode | 6.53ms | 2.57ms | **2.5x faster** |
| seph-blog1 | 27.0ms | 5.32ms | **5.1x faster** |

## Optimizations Applied

1. **Compact Span Structure** (674d624)
   - Span reduced from 112 bytes to 24 bytes (4.7x smaller)
   - Added UserTable for u16 user indices
   - Added OriginRef for compact origin storage

2. **Fenwick Tree for Chunk Weights** (fc0d3e3, b51bb51)
   - O(log n) chunk lookup for large documents
   - Hybrid approach: linear scan for small, Fenwick for large

3. **Cursor Caching** (a78bd09)
   - O(1) sequential inserts via cached position
   - Cache invalidation on deletes and non-sequential operations

4. **Buffered Writes (RgaBuf)** (a0c2213)
   - Adjacent inserts buffered and applied as one operation
   - Backspace optimization: trim pending insert instead of full delete

5. **Inline Hints and Debug Asserts** (989b9fd)
   - #[inline(always)] on hot Span methods
   - debug_assert instead of assert in release

6. **Chunk Location Caching** (37e4ef0)
   - Extended cursor cache to store chunk location
   - Eliminates repeated find_chunk_by_index calls

7. **SmallVec and Fenwick Improvements** (b72ae09)
   - SmallVec for pending content (avoids heap allocation)
   - Combined Fenwick traversal for index and prefix sum

8. **Skip HashMap Lookup in Flush** (37e4ef0)
   - insert_with_user_idx to avoid redundant HashMap lookup

## Failed Optimizations

1. **Skip List for Spans** (branch: ibc/slow-weighted-skip-list)
   - 2000-12000x slower due to O(n) fallback
   - Would need dual width tracking for both count and weight

## Why automerge-paper is Slower

The automerge-paper trace (260k patches) is 1.77x slower than diamond-types. This is likely because:
- It has more non-sequential operations that break our buffering
- The larger size means our O(sqrt n) chunk operations within chunks become more expensive
- Diamond-types' B-tree structure scales better for very large traces

Future optimizations to improve automerge-paper:
- Replace chunked list with true B-tree (like diamond-types)
- Implement gap buffer for content storage
- Add delete buffering to RgaBuf

## Key Insights

1. **Cache locality matters more than algorithmic complexity** for small n
2. **Buffering consecutive operations** gives huge wins for editing traces
3. **Chunk-based structures** with Fenwick trees can match B-tree performance up to ~150k ops
4. **Avoiding allocations** (SmallVec, reusing user indices) adds up

## Commits

```
f655884 Enable automerge-paper benchmark: 3/4 traces faster than diamond-types
4f8f48c Update worklog with final results
193f3aa Fix rga_trace benchmark to use RgaBuf
37e4ef0 Optimize: chunk location caching, skip HashMap lookup in flush
989b9fd Inline hints, debug_assert, smarter cache invalidation
b72ae09 Optimize small traces: SmallVec, Fenwick improvements
5f42e81 Optimize RgaBuf delete: trim pending insert on backspace
a0c2213 Add RgaBuf: buffered writes for 1.2-1.4x speedup
a78bd09 Add cursor caching for O(1) sequential inserts
b51bb51 Hybrid Fenwick/linear scan: fix sveltecomponent regression
fc0d3e3 Add Fenwick trees to WeightedList: 1.6x speedup on seph-blog1
674d624 Compact span structure: 2.8ms -> 2.1ms (1.34x speedup)
```
