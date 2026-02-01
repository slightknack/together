+++
model = "claude-opus-4-5"
created = 2026-02-01
modified = 2026-02-01
driver = "Isaac Clayton"
+++

# Tree-Structured Conflict Resolution Optimization

## Goal

Reduce remote insert complexity from O(n) worst-case to O(log n) by using a tree-structured index for YATA conflict resolution.

## Background

When inserting a span with left_origin O, the YATA algorithm must:
1. Find all siblings (spans also with left_origin O)
2. Compare the new span against each sibling using YATA rules
3. Skip subtrees of siblings we pass over

The current implementation uses linear scan, which is O(k) where k = number of siblings. Adversarial inputs can force k = n, giving O(n) worst-case.

## Design

### SiblingKey

A key structure that implements YATA ordering for BTreeMap storage:

```rust
struct SiblingKey {
    right_origin: Option<(u16, u32)>,  // None = infinity
    user_idx: u16,
    seq: u32,
}
```

Ordering rules (for BTreeMap, smaller = comes first in document):
1. Has right_origin (finite) < no right_origin (infinity)
2. Higher right_origin ID = smaller key (comes first)
3. Higher (user, seq) = smaller key (tiebreaker)

### Sibling Index

```rust
sibling_index: FxHashMap<(u16, u32), BTreeMap<SiblingKey, (u16, u32)>>
```

Maps origin_id to siblings sorted by YATA order. Stores stable IDs (user_idx, seq) instead of span indices to avoid invalidation when indices shift.

### Hybrid Approach

The key insight is that sequential editing traces have very few siblings per origin (typically 0-1). Using the tree index for these cases adds overhead without benefit.

Solution: threshold-based hybrid
- For few siblings (< 8): use existing linear scan
- For many siblings (>= 8): use sibling index for O(log k) lookup

### Lazy Population

To avoid overhead for the common case, the sibling index is populated lazily:
- `add_to_sibling_index` only adds if there's already an entry for that origin
- When `find_position_with_origin` detects many siblings but no index, it scans to count and populates if threshold is met

## Implementation

Key changes to `src/crdt/rga.rs`:
1. Added `SiblingKey` struct with `Ord` implementation
2. Added `sibling_index` field to `Rga`
3. Modified `find_position_with_origin` for hybrid approach
4. Added `add_to_sibling_index` (lazy)
5. Added `populate_sibling_index` (builds index when threshold crossed)
6. Added `rebuild_sibling_index` (for merge operations)

## Benchmark Results

Sequential editing traces (profile_trace):

| Trace | Baseline | Tree (naive) | Tree (hybrid+lazy) |
|-------|----------|--------------|-------------------|
| sveltecomponent | 2.01ms | 2.27ms (+13%) | 2.04ms (+1.5%) |
| rustcode | 4.45ms | 4.85ms (+9%) | 4.41ms (-1%) |
| seph-blog1 | 7.69ms | 8.44ms (+10%) | 7.79ms (+1.3%) |
| automerge-paper | 9.02ms | 9.45ms (+5%) | 8.92ms (-1.1%) |

The naive tree approach added 5-13% overhead due to index maintenance. The hybrid+lazy approach eliminates this overhead for sequential traces while providing O(log k) for adversarial cases.

## Complexity Analysis

| Scenario | Before | After |
|----------|--------|-------|
| No siblings | O(1) | O(1) |
| Few siblings (< 8) | O(k) | O(k) |
| Many siblings (>= 8) | O(k) | O(log k) + O(k) index build |
| Repeated many siblings | O(k) each | O(log k) after first |

The O(k) index build happens once when threshold is crossed. Subsequent inserts at the same origin are O(log k).

## Conclusion

The tree-structured optimization with lazy population achieves the goal: O(log k) conflict resolution for adversarial cases without regressing the common case. The threshold of 8 siblings provides a good balance between overhead and benefit.

## Future Work

- Tune threshold based on profiling (currently 8, could be higher)
- Consider storing span indices with update-on-insert for faster lookup
- Investigate B-tree with stable node IDs for truly O(log n) without ID lookup
