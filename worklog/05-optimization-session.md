+++
title = "Optimization Session: Faster than Diamond-Types"
date = 2026-02-01
+++

## Goal

Faster than diamond-types on 3/4 benchmarks.

## Baseline Benchmarks

| Trace | Patches | Together | Diamond | Ratio |
|-------|---------|----------|---------|-------|
| sveltecomponent | 19,749 | 7.1ms | 1.6ms | 4.5x slower |
| rustcode | 40,173 | 26.3ms | 3.9ms | 6.7x slower |
| seph-blog1 | 137,993 | 72.5ms | 8.4ms | 8.7x slower |
| automerge-paper | 259,778 | 25.7ms | 13.8ms | 1.9x slower |

## Research Findings

### Diamond-Types
- Uses RLE-compressed operation log
- Content stored separately (SoA layout)
- O(log n) lookups via binary search on RleVec
- Fast-path for sequential local edits
- Cursor caching for sequential access

### json-joy
- Dual splay tree architecture:
  - `root` tree ordered by document position
  - `ids` tree ordered by (sid, time) for O(log n) ID lookup
- Each chunk has 6 tree pointers (3 per tree)
- Splay operation moves recently accessed to root
- Good for sequential access patterns

### Our Bottlenecks
1. `find_span_by_id()` is O(n) linear scan - called in tight loops during merge
2. `insert_span_rga` is 260 lines with duplicated logic
3. Subtree tracking uses O(n) linear search per iteration

## Optimization Plan

### Phase 1: Refactoring (Correctness Focus)
1. Extract `insert_span_rga` into smaller functions
2. Add comprehensive tests for each extracted function
3. Extract YATA comparison logic

### Phase 2: ID Lookup Index
1. Add `HashMap<(u16, u32), usize>` for O(1) ID lookup
2. Maintain index on insert/split/delete
3. Add debug assertions to validate index correctness

### Phase 3: Batch Merge Optimization
1. Pre-scan to find known ID ranges
2. Bulk insert missing spans
3. Verify against naive merge in tests

### Phase 4: Cursor Caching
1. Cache last insertion position
2. Start from cache if nearby
3. Fallback to normal path on cache miss

## Progress

### Refactoring insert_span_rga

TODO: Document progress here
