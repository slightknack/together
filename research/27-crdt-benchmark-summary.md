# CRDT Library Benchmark Summary

Comprehensive benchmark results comparing text CRDT implementations across multiple editing traces.

## Traces Used

| Trace | Final Size | Operations | Ops/Char | Description |
|-------|-----------|------------|----------|-------------|
| automerge-paper | 104KB | 259,778 | 2.5 | Academic paper editing |
| rustcode | 6KB | 40,151 | 6.7 | Rust code writing |
| seph-blog1 | 16KB | 137,927 | 8.6 | Blog post writing |
| sveltecomponent | 2KB | 19,700 | 9.9 | Component development |

## Benchmark Results (ms total time)

| Library | sveltecomponent | rustcode | seph-blog1 | automerge-paper |
|---------|----------------|----------|------------|-----------------|
| **Together** | 1.29 | 3.06 | 4.94 | 4.37 |
| diamond-types | 1.53 | 3.83 | 9.50 | 15.02 |
| Cola | 2.48 | 21.47 | 38.96 | 142.98 |
| json-joy (JS) | 7.64 | 25.13 | 53.49 | 99.19 |
| Loro | 15.16 | 36.16 | 77.20 | 144.84 |
| Automerge | 165.20 | 1180.60 | 431.83 | 303.34 |
| Yrs | 359.38 | 1182.30 | 5563.46 | 6520.55 |
| Zed (text) | 1320 | 2990 | 10100 | 18800 |

## Benchmark Results (ns/op)

| Library | sveltecomponent | rustcode | seph-blog1 | automerge-paper |
|---------|----------------|----------|------------|-----------------|
| **Together** | 65 | 76 | 36 | 17 |
| diamond-types | 77 | 95 | 69 | 58 |
| Cola | 126 | 534 | 282 | 550 |
| json-joy (JS) | 387 | 626 | 388 | 382 |
| Loro | 768 | 900 | 560 | 558 |
| Automerge | 8,365 | 29,391 | 3,130 | 1,168 |
| Yrs | 18,198 | 29,434 | 40,316 | 25,101 |
| Zed (text) | 66,835 | 74,428 | 73,193 | 72,369 |

## Speedup vs Together

| Library | sveltecomponent | rustcode | seph-blog1 | automerge-paper |
|---------|----------------|----------|------------|-----------------|
| diamond-types | 1.2x slower | 1.3x slower | 1.9x slower | 3.4x slower |
| Cola | 1.9x slower | 7.0x slower | 7.9x slower | 32.7x slower |
| json-joy (JS) | 5.9x slower | 8.2x slower | 10.8x slower | 22.7x slower |
| Loro | 11.8x slower | 11.8x slower | 15.6x slower | 33.1x slower |
| Automerge | 128x slower | 386x slower | 87x slower | 69x slower |
| Yrs | 279x slower | 386x slower | 1126x slower | 1492x slower |
| Zed (text) | 1023x slower | 977x slower | 2044x slower | 4302x slower |

## Performance Tiers

### Tier 1: Ultra-Fast (< 20 ms on automerge-paper)
- **Together** (4.37 ms) - Fastest overall
- **diamond-types** (15.02 ms) - Excellent performance, well-optimized

### Tier 2: Fast (20-150 ms on automerge-paper)
- **Cola** (142.98 ms) - Good Rust implementation
- **json-joy** (99.19 ms) - Impressive for JavaScript, dual splay tree design
- **Loro** (144.84 ms) - Feature-rich with good performance

### Tier 3: Moderate (150-500 ms on automerge-paper)
- **Automerge** (303.34 ms) - Focus on correctness and features over speed

### Tier 4: Slow (> 1 second on automerge-paper)
- **Yrs** (6520.55 ms) - Performance issues on some traces
- **Zed text** (18800 ms) - Designed for editor features, not raw speed

## Key Observations

### Together's Performance Advantages

1. **Cursor Cache**: O(1) sequential typing via cached lookup position
2. **Span Coalescing**: Adjacent same-user operations merge into single spans
3. **Columnar Storage**: Per-user vectors enable efficient traversal
4. **RgaBuf Buffering**: Delayed integration reduces overhead

### Why automerge-paper is Fastest for Together

The automerge-paper trace has the lowest ops/char ratio (2.5), meaning:
- More sequential typing patterns
- Better coalescing opportunities
- Cursor cache hit rate is highest

### Notable Findings

1. **Cola performs well on small traces** but scales poorly on larger ones (32x slower on automerge-paper vs 1.9x on sveltecomponent)

2. **json-joy is remarkably fast for JavaScript** - faster than Loro (Rust) and competitive with Cola on larger traces

3. **Yrs has severe performance regression** on seph-blog1 and automerge-paper traces (possibly due to specific edit patterns)

4. **Zed's text CRDT** is designed for rich editor features (anchors, selections, undo groups), not raw throughput

## Conclusions

Together achieves the best performance across all traces by:
1. Specializing for text editing (not general CRDT)
2. Optimizing for the common case (sequential typing)
3. Using memory-efficient columnar layouts
4. Aggressive span coalescing to reduce node count

The performance gap widens on larger, more edit-heavy traces where Together's optimizations compound. On automerge-paper, Together is **3.4x faster than diamond-types** (the next fastest) and **22-4300x faster** than other libraries.
