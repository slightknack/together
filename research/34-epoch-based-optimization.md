+++
model = "claude-opus-4-5"
created = 2026-02-01
modified = 2026-02-01
driver = "Isaac Clayton"
+++

# Epoch-Based Batching Optimization

This document records the implementation of Approach 4 (Epoch-Based Batching) from the insert optimization procedure.

## Problem

Remote inserts have O(n) worst-case complexity due to the YATA/Fugue conflict scan. After finding the origin span, we must scan forward through all potential conflicts to find the correct insertion point. Adversarial input can force linear scans.

## Solution: Epoch-Based Batching

The key insight: an epoch is a contiguous range of operations from one editing session. Operations from different epochs can be ordered deterministically by epoch ID without YATA scan. Only operations within the same epoch need conflict resolution.

### Implementation

Branch: `opt/epoch-based` (from `crdt-comparison-study`)
Worktree: `/tmp/together-worktrees/epoch-based`

Changes to `src/crdt/rga.rs`:

1. Added `epoch: u32` field to `Span` struct
2. Added `current_epoch: u32` to `Rga` struct
3. Modified `yata_compare` to check epochs first (fast path)
4. Modified `merge` to increment epochs after merging

### Epoch Ordering Rules

In `yata_compare`:
```rust
// EPOCH-BASED FAST PATH: Different epochs can be ordered deterministically
// Lower epoch = inserted earlier = comes first in document order
if new_epoch != existing.epoch {
    if new_epoch < existing.epoch {
        return YataOrder::Before;
    } else {
        return YataOrder::After;
    }
}
// Same epoch: fall back to full YATA comparison
```

In `merge`:
```rust
// Find the maximum epoch in the other document and ensure our epoch
// is higher than both our current epoch and other's max epoch.
let other_max_epoch = other.spans.iter().map(|s| s.epoch).max().unwrap_or(0);
self.current_epoch = self.current_epoch.max(other_max_epoch) + 1;
```

### Trade-offs

- Span size increases from 32 to 40 bytes (added `epoch: u32` and adjusted padding)
- Negligible overhead for local operations
- Significant benefit for merging documents from different editing sessions

## Test Results

All tests pass:
- 235 library tests passed
- 156 conformance tests passed

## Benchmark Results

Comparing epoch-based (opt/epoch-based) vs baseline (crdt-comparison-study):

### Epoch-Based (3 runs, total time in ms)

| Trace | Run 1 | Run 2 | Run 3 | Avg |
|-------|-------|-------|-------|-----|
| sveltecomponent | 2.05 | 2.05 | 2.02 | 2.04 |
| rustcode | 4.44 | 4.42 | 4.43 | 4.43 |
| seph-blog1 | 7.95 | 7.78 | 7.89 | 7.87 |
| automerge-paper | 9.24 | 9.15 | 9.20 | 9.20 |

### Baseline (3 runs, total time in ms)

| Trace | Run 1 | Run 2 | Run 3 | Avg |
|-------|-------|-------|-------|-----|
| sveltecomponent | 2.08 | 2.04 | 2.05 | 2.06 |
| rustcode | 4.36 | 4.48 | 4.39 | 4.41 |
| seph-blog1 | 7.75 | 7.77 | 7.81 | 7.78 |
| automerge-paper | 9.02 | 9.21 | 9.15 | 9.13 |

### Analysis

The results are within measurement noise, showing negligible overhead for local operations. The epoch-based optimization does not slow down single-user editing traces.

The real benefit will be visible when:
1. Merging documents from different editing sessions (different epochs)
2. Handling concurrent edits where spans from different epochs can be ordered in O(1) instead of O(k) YATA comparisons

## Complexity Analysis

Without epochs (baseline):
- Local insert: O(log n) position lookup + O(1) amortized (coalescing)
- Remote insert: O(log n) position lookup + O(k) YATA scan where k = siblings

With epochs:
- Local insert: same as baseline
- Remote insert with different epoch: O(log n) position lookup + O(1) epoch comparison
- Remote insert with same epoch: same as baseline

The optimization provides the most benefit when merging documents that were edited independently for a while. In collaborative editing, most operations will be in different epochs, making the O(1) fast path common.

## Files Changed

- `src/crdt/rga.rs`: Added epoch tracking (+49 lines, -9 lines)

## Commit

```
e8c6366 feat(rga): add epoch-based batching optimization for remote inserts
```

## Next Steps

1. Test with adversarial concurrent editing patterns to measure the benefit
2. Consider epoch compaction (merging adjacent epochs from same user)
3. Compare with Approach 1 (Tree-Structured) and Approach 3 (Origin Index)
