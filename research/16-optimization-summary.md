// model = "claude-opus-4-5"
// created = "2026-01-31"
// modified = "2026-01-31"
// driver = "Isaac Clayton"

# Optimization Summary: Together vs Diamond-Types

## Overview

Starting point: **347x slower** than diamond-types
Final result: **3-62% faster** than diamond-types
Total speedup: **280-2060x** depending on trace

## Final Results

| Trace | Ops | Together | Diamond | vs Diamond | Our Speedup |
|-------|-----|----------|---------|------------|-------------|
| sveltecomponent | 19,749 | 1.67ms | 1.73ms | **1.03x faster** | 278x from start |
| rustcode | 40,173 | 3.46ms | 4.32ms | **1.25x faster** | ~430x from start |
| seph-blog1 | 137,993 | 6.06ms | 9.10ms | **1.50x faster** | ~990x from start |
| automerge-paper | 259,778 | 5.82ms | 15.44ms | **2.65x faster** | ~2060x from start |

## Starting Point (Naive Implementation)

| Trace | Together | Diamond | vs Diamond |
|-------|----------|---------|------------|
| sveltecomponent | 465ms | 1.2ms | **347x slower** |
| rustcode | ~1.5s | ~3ms | **~500x slower** |
| seph-blog1 | ~6s | ~7ms | **~850x slower** |
| automerge-paper | ~12s | ~15ms | **~800x slower** |

## Complete Optimization History

### Phase 1: From 347x Slower to Parity

| # | Optimization | Speedup | vs Diamond After |
|---|-------------|---------|------------------|
| 1 | Remove HashMap index | 8.8x | 39x slower |
| 2 | Chunked weighted list (sqrt n chunks) | 77x | 4.5x slower |
| 3 | Span coalescing (merge adjacent) | 1.9x | 2.8x slower |
| 4 | Combined origin/insert lookup | 1.24x | 2.3x slower |
| 5 | Compact Span (112B → 24B) | 1.34x | ~1.7x slower |
| 6 | Fenwick tree for chunk weights | 1.6x (large) | ~1.5x slower |
| 7 | Hybrid Fenwick/linear scan | fixed regression | ~1.5x slower |
| 8 | Cursor caching | 1.15x | ~1.3x slower |
| 9 | RgaBuf (buffered writes) | 1.2-1.4x | ~1.1x slower |
| 10 | Backspace optimization | 1.1x | ~1.0x (parity on 3/4) |
| 11 | SmallVec for pending content | 1.05x | ~1.0x |
| 12 | Inline hints + debug_assert | 1.05x | ~1.0x |
| 13 | Chunk location caching | 1.05x | **3/4 faster, 1 at 1.77x slower** |

### Phase 2: Beat Diamond-Types on All Benchmarks

Starting point for automerge-paper: 32ms (2.1x slower than diamond's 15ms)

| # | Optimization | Speedup | automerge-paper vs Diamond |
|---|-------------|---------|---------------------------|
| 14 | B-Tree for spans | 1.25x | 1.6x slower (25ms) |
| 15 | Delete buffering | 2.8x | **1.8x faster** (8.5ms) |
| 16 | FxHashMap for UserTable | 1.1x | **2.65x faster** (5.8ms) |

### Failed Optimizations

| Optimization | Result | Why It Failed |
|-------------|--------|---------------|
| Skip list for spans | 2000-12000x slower | O(n) fallback, high constant factors |
| Binary search over chunks | slower | O(n) prefix sum recalc per step |
| Smaller B-tree leaves (32) | 10% slower | more tree depth |
| Larger B-tree leaves (128) | 5% slower | more within-leaf scanning |
| Simplified cursor cache | 5% slower | lost sequential optimizations |

## Cumulative Effect by Trace

### sveltecomponent (19,749 ops)
```
Start:    465.00 ms  (347x slower than diamond)
Step 2:     6.80 ms  (5.1x slower)  -- chunked list
Step 5:     2.10 ms  (1.6x slower)  -- compact spans
Step 9:     1.09 ms  (0.96x = 4% faster)  -- buffering
Final:      1.67 ms  (1.03x faster)
Total:      278x speedup
```

### automerge-paper (259,778 ops)
```
Start:   ~12.00 s   (~800x slower than diamond)
Step 2:   ~150 ms   (~10x slower)  -- chunked list
Step 9:    32.0 ms  (2.1x slower)  -- buffering
Step 14:   25.0 ms  (1.6x slower)  -- B-tree
Step 15:    8.5 ms  (1.8x faster)  -- delete buffering
Final:      5.8 ms  (2.65x faster)
Total:    ~2060x speedup
```

## Key Insights

1. **Biggest wins**: Chunked list (77x), delete buffering (2.8x), span coalescing (1.9x)
2. **Cache locality > algorithmic complexity** for small-medium n
3. **Batching consecutive operations** is critical for editing traces
4. **Skip lists have high constant factors** - not always faster than arrays
5. **Measure before optimizing** - theoretical gains often don't materialize

## Summary

| Metric | Value |
|--------|-------|
| Starting point | 347x slower than diamond-types |
| Final result | 1.03-2.65x faster than diamond-types |
| Total speedup | 278-2060x depending on trace |
| Successful optimizations | 16 |
| Failed optimizations | 5 |
