+++
model = "claude-opus-4-5"
created = 2026-02-01
modified = 2026-02-01
driver = "Isaac Clayton"
+++

# Worklog: Remote Insert O(n) to O(log n) Optimization

## Goal

Reduce remote insert complexity from O(n) to O(log n) by optimizing the YATA conflict scan.

## Problem

Remote inserts had O(n) worst-case complexity. After finding the origin span, we scanned forward through all potential conflicts to find the correct insertion point. Adversarial input could force linear scans.

## Approaches Implemented

Three approaches were implemented in separate branches, following `procedures/04-insert-optimization.md`.

### Approach 1: Origin Index (Winner)

**Branch:** `opt/origin-index`

Added a HashMap from origin ID to list of sibling spans:
```rust
origin_index: FxHashMap<(u16, u32), SmallVec<[usize; 4]>>
```

On insert, look up siblings via the index instead of scanning all spans after the origin. Only scan the k siblings that share the same origin.

**Complexity:** O(k) where k = siblings with same origin. For typical editing, k is 1-3.

### Approach 2: Tree-Structured Conflict Resolution

**Branch:** `opt/tree-structured`

Added a secondary BTreeMap ordered by YATA comparison for origins with many siblings:
```rust
sibling_index: FxHashMap<(u16, u32), BTreeMap<SiblingKey, (u16, u32)>>
```

Hybrid approach: linear scan for < 8 siblings, tree lookup for >= 8.

**Complexity:** O(log k) for large k, but overhead cancels benefit for typical workloads.

### Approach 3: Epoch-Based Batching

**Branch:** `opt/epoch-based`

Added epoch tracking to spans. Operations from different epochs ordered by epoch ID without YATA scan:
```rust
struct Span {
    epoch: u32,
    // ...
}
```

**Complexity:** O(1) for cross-epoch comparisons, but adds overhead and doesn't help single-user traces.

## Benchmark Results

All times as ratio vs diamond-types (lower = faster, <1.0 = faster than diamond):

| Approach | sveltecomponent | rustcode | seph-blog1 | automerge-paper |
|----------|-----------------|----------|------------|-----------------|
| Baseline | 1.14x | 1.07x | 0.72x | 0.38x |
| **Origin Index** | **0.80x** | **0.80x** | **0.53x** | **0.32x** |
| Tree-Structured | 1.08x | 1.05x | 0.73x | 0.38x |
| Epoch-Based | 1.13x | 1.09x | 0.73x | 0.40x |

## Winner: Origin Index

The origin index approach provides consistent 20-30% improvement across all traces:

- sveltecomponent: 30% faster
- rustcode: 25% faster
- seph-blog1: 26% faster
- automerge-paper: 16% faster

Tree-structured and epoch-based approaches add overhead that cancels their theoretical benefits for sequential editing traces. They would help more with adversarial concurrent workloads.

## Changes Merged

The origin index optimization was merged to `crdt-comparison-study`:

**Files changed:**
- `src/crdt/rga.rs` - Added origin_index field and lookup logic
- `src/crdt/op.rs` - Added right_origin field for correct merge commutativity

**Key implementation details:**
- Changed origin representation from unstable indices to stable (user_idx, seq) IDs
- Index maps origin ID to SmallVec of sibling span indices
- Updated on insert, rebuilt when indices become stale after splits

## Branches

All three approaches pushed to remote:
- `opt/origin-index` - Merged to crdt-comparison-study
- `opt/tree-structured` - Preserved for reference
- `opt/epoch-based` - Preserved for reference

## Lessons Learned

1. **Simple solutions often win** - The HashMap lookup beat the fancy tree structure
2. **Measure before optimizing** - Tree-structured approach had theoretical O(log k) but constant factors made it slower
3. **Typical vs adversarial** - Optimizations that help adversarial cases may hurt typical cases
4. **Stable IDs matter** - Using (user, seq) instead of span indices simplified the implementation

## Files

- `procedures/04-insert-optimization.md` - The procedure followed
- `research/42-tree-structured-optimization.md` - Tree-structured worklog (in worktree)
- `research/34-epoch-based-optimization.md` - Epoch-based worklog (in worktree)
