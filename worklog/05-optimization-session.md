+++
model = "claude-opus-4-5"
created = 2026-02-01
modified = 2026-02-01
driver = "Isaac Clayton"
+++

# Worklog: RGA Optimization Session

## Goal

Achieve faster than diamond-types on 3/4 benchmarks while maintaining correctness.

## Starting Point

After previous refactoring work, benchmarks showed:
- sveltecomponent: 4.5x slower
- rustcode: 6.7x slower  
- seph-blog1: 8.7x slower
- automerge-paper: 1.9x slower

## Optimizations Applied

### 1. Refactor insert_span_rga into smaller functions

Extracted YATA ordering logic into separate functions:
- `yata_compare()` - pure YATA comparison
- `find_position_with_origin()` - find insert position with origin
- `find_position_at_root()` - find insert position without origin
- `origin_in_subtree()`, `add_to_subtree()`, `is_sibling()` - helpers

This reduced the main function from ~260 lines to ~50 lines (Linux kernel style: max 3 levels deep).

### 2. Implement ID lookup index

Added `id_index: FxHashMap<(u16, u32), usize>` for O(1) span lookup by ID during merge operations. Index is rebuilt before merge via `rebuild_id_index()`.

### 3. Optimize non-coalescing inserts with hint

Created `insert_span_rga_with_hint()` and `find_position_with_origin_hint()` to pass the known origin position directly, avoiding redundant `find_span_by_id()` O(n) lookup.

Key insight: In `insert_span_at_pos_optimized`, when we can't coalesce, we already know the origin's position from the previous lookup. Passing this as a hint skips redundant work.

### 4. Add fast path for YATA scan

Added early exit checks in both `find_position_with_origin` and `find_position_with_origin_hint`:
- If no spans after origin → return immediately
- If next span has no origin → return immediately  
- If next span's origin doesn't match ours → return immediately

This avoids setting up subtree tracking (~24% of YATA scans exit via fast path).

### 5. Use SmallVec for subtree tracking

Replaced `Vec<(u16, u32, u32)>` with `SmallVec<[(u16, u32, u32); 8]>` in:
- `find_position_with_origin`
- `find_position_with_origin_hint`
- `skip_subtree`
- `add_to_subtree`

This avoids heap allocation for typical single-user editing scenarios.

### 6. Enable LTO and single codegen unit

Added to Cargo.toml:
```toml
[profile.release]
lto = true
codegen-units = 1
```

This enables link-time optimization for better cross-crate inlining.

## Profiling Insights

Added profiling counters to understand hot paths:

| Trace | Cursor Hit Rate | Coalesce | YATA Scans | Fast Exit |
|-------|-----------------|----------|------------|-----------|
| sveltecomponent | 0.1% | 80 | 2868 | 690 (24%) |
| rustcode | 0.2% | 241 | 5906 | 1526 (26%) |
| seph-blog1 | 0.4% | 501 | 6800 | 1454 (21%) |
| automerge-paper | 0.0% | 120 | 4471 | 799 (18%) |

Key findings:
1. **Cursor cache hit rate is ~0%** - Expected because RgaBuf batches inserts
2. **Coalesce rate is low** - RgaBuf does coalescing at higher level
3. **~76% of YATA scans enter slow path** - Spans after origin ARE siblings/descendants
4. **Fast exit triggers ~24%** - Still valuable for avoiding subtree tracking setup

## Final Results

| Trace | Together | Diamond | Ratio | Status |
|-------|----------|---------|-------|--------|
| sveltecomponent | 2.0ms | 1.6ms | 1.2-1.3x slower | ❌ |
| rustcode | 4.2ms | 3.9ms | 1.0-1.2x slower | ⚠️ borderline |
| seph-blog1 | 7.0ms | 8.6ms | **0.8x faster** | ✅ |
| automerge-paper | 5.8ms | 14.0ms | **0.4x faster** | ✅ |

Improvement from start of session:
- sveltecomponent: 4.5x → 1.3x (3.5x improvement)
- rustcode: 6.7x → 1.1x (6x improvement)
- seph-blog1: 8.7x → 0.8x (11x improvement)
- automerge-paper: 1.9x → 0.4x (5x improvement)

## Why Slower Traces Are Slower

Analysis of trace characteristics:

| Trace | Jump Insert Ratio | Scattered Deletes |
|-------|-------------------|-------------------|
| sveltecomponent | 20.6% | 75.2% |
| rustcode | 22.2% | 71.7% |
| seph-blog1 | 8.2% | 66.0% |
| automerge-paper | 3.4% | 5.5% |

Higher jump insert ratio = more YATA scans needed.
Higher scattered deletes = less benefit from delete batching.

## Fundamental Differences with diamond-types

diamond-types uses:
1. **JumpRope** - skip list with 392-byte gap buffers
2. **Cursor-based access** - maintains position for sequential operations
3. **Gap buffers** - sequential inserts just extend gap, no tree rebalancing

Our approach:
1. **B-tree** - O(log n) for every operation
2. **Cursor cache** - only helps for exact position match
3. **Span coalescing** - good for sequential inserts but requires tree ops

The ~10% gap on rustcode is likely due to:
- More tree traversals for jump inserts
- B-tree overhead vs skip list for sequential access patterns

## Future Optimization Ideas

1. **Splay tree / self-adjusting tree** - Move recently accessed nodes to root
2. **Better cursor caching** - Cache nearby positions, not just exact match
3. **Gap buffer integration** - Like diamond-types' JumpRope
4. **Batch YATA scans** - Process multiple jump inserts together

## Lessons Learned

1. **Profile before optimizing** - The cursor cache wasn't the bottleneck we thought
2. **Understand the data** - Jump insert ratio explains performance differences
3. **Fast paths matter** - Even 24% early exit is valuable
4. **Allocation avoidance** - SmallVec for small, hot allocations
5. **LTO is free performance** - Just add it to Cargo.toml

## Files Changed

- `src/crdt/rga.rs` - Main optimizations
- `src/crdt/btree_list.rs` - Already had good performance
- `src/crdt/profiling.rs` - New, for performance counters
- `Cargo.toml` - LTO settings
- `comparisons/criterion/src/bin/analyze_trace.rs` - Trace analysis tool
