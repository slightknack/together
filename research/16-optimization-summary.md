// model = "claude-opus-4-5"
// created = "2026-01-31"
// modified = "2026-01-31"
// driver = "Isaac Clayton"

# Optimization Summary: Together vs Diamond-Types

## Final Results

All benchmarks faster than diamond-types:

| Trace | Patches | Together | Diamond-Types | Ratio | Status |
|-------|---------|----------|---------------|-------|--------|
| sveltecomponent | 19,749 | 1.67ms | 1.73ms | **0.97x** | 3% FASTER |
| rustcode | 40,173 | 3.46ms | 4.32ms | **0.80x** | 20% FASTER |
| seph-blog1 | 137,993 | 6.06ms | 9.10ms | **0.67x** | 33% FASTER |
| automerge-paper | 259,778 | 5.82ms | 15.44ms | **0.38x** | 62% FASTER |

## Complete Optimization History

### Phase 1: Initial Implementation to Competitive

Starting point: **347x slower** than diamond-types (465ms vs 1.2ms on sveltecomponent)

| # | Optimization | Commit | Effect | Cumulative |
|---|-------------|--------|--------|------------|
| 1 | **Remove HashMap index** | 213b75e | 8.8x faster | 39x slower |
| 2 | **Chunked weighted list** | c961f61 | 77x faster | 4.5x slower |
| 3 | **Span coalescing** | bc0fd12 | 1.9x faster | 2.8x slower |
| 4 | **Combined origin/insert lookup** | fcafb2e | 1.24x faster | 2.3x slower |
| 5 | **Compact span structure** (112B → 24B) | 674d624 | 1.34x faster | ~2x slower |
| 6 | **Fenwick tree for chunk weights** | fc0d3e3 | 1.6x on large traces | varies |
| 7 | **Hybrid Fenwick/linear scan** | b51bb51 | Fixed regression on small | ~2x slower |
| 8 | **Cursor caching** | a78bd09 | 1.15x faster | ~1.8x slower |
| 9 | **RgaBuf (buffered writes)** | a0c2213 | 1.2-1.4x faster | ~1.4x slower |
| 10 | **Backspace optimization** | 5f42e81 | ~1.1x faster | ~1.3x slower |
| 11 | **SmallVec for pending content** | b72ae09 | ~1.05x faster | ~1.2x slower |
| 12 | **Inline hints + debug_assert** | 989b9fd | ~1.05x faster | ~1.1x slower |
| 13 | **Chunk location caching** | 37e4ef0 | ~1.05x faster | **~1.0x (parity)** |

**Phase 1 Result**: 3/4 benchmarks faster than diamond-types, 1 at 1.77x slower

### Phase 2: Beat Diamond-Types on All Benchmarks

Starting point: automerge-paper 2.1x slower (32ms vs 15ms)

| # | Optimization | Commit | Effect | Result |
|---|-------------|--------|--------|--------|
| 14 | **B-Tree for spans** | 280eabc | 25% faster on automerge-paper | 1.6x slower |
| 15 | **Delete buffering in RgaBuf** | 4a80727 | 2.8x faster on automerge-paper | **0.55x (45% faster)** |
| 16 | **FxHashMap for UserTable** | 51c9327 | 5-10% faster all traces | **ALL FASTER** |

### Failed Optimizations

| Optimization | Commit | Why It Failed |
|-------------|--------|---------------|
| **Skip list for spans** | ce6cc55 | 2000-12000x slower due to O(n) fallback; would need dual-width tracking |
| **Binary search over chunks** | (reverted) | Each step recalculates prefix sums O(n), slower than linear scan |
| **Smaller leaf size (32)** | (tested) | More tree depth, worse cache locality |
| **Larger leaf size (128)** | (tested) | More within-leaf scanning |
| **Simplified cursor cache** | (tested) | Lost optimizations for sequential patterns |

## Optimization Techniques by Category

### Data Structure Changes
- Chunked list with sqrt(n) chunk size (77x)
- B-Tree for O(log n) operations (25%)
- Fenwick tree for prefix sums (1.6x on large)
- Skip list (FAILED - overhead too high)

### Memory Layout
- Compact Span: 112 bytes → 24 bytes (1.34x)
- UserTable: 32-byte KeyPub → 2-byte u16 index
- SmallVec for small allocations (~1.05x)

### Algorithmic
- Span coalescing: merge adjacent same-user spans (1.9x)
- Combined lookups: single traversal for origin + insert position (1.24x)
- Cursor caching: O(1) sequential operations (1.15x)
- Hybrid Fenwick/linear: adapt to data size

### Batching
- RgaBuf: buffer adjacent inserts (1.2-1.4x)
- Backspace optimization: trim buffer instead of delete (1.1x)
- Delete buffering: batch adjacent deletes (2.8x on automerge-paper)

### Low-Level
- FxHashMap: faster hash function (5-10%)
- Inline hints on hot paths (~1.05x)
- debug_assert vs assert in release

## Key Insights

1. **Cache locality > algorithmic complexity** for small-medium n
2. **Batching consecutive operations** is huge for editing traces
3. **Chunk-based structures** can match B-trees up to ~150k ops
4. **Measure before optimizing** - theoretical gains often don't materialize
5. **Fragmentation kills performance** - coalescing is critical
6. **Chunk size ≈ sqrt(n)** is often optimal
7. **Skip lists have high constant factors** - not always faster than arrays

## Total Improvement

| Trace | Start | End | Total Speedup |
|-------|-------|-----|---------------|
| sveltecomponent | 465ms | 1.67ms | **278x faster** |
| rustcode | ~1.5s | 3.46ms | **~430x faster** |
| seph-blog1 | ~6s | 6.06ms | **~990x faster** |
| automerge-paper | ~12s | 5.82ms | **~2060x faster** |

From 347x slower than diamond-types to 38-62% faster.
