+++
model = "claude-opus-4-5"
created = 2026-02-01
modified = 2026-02-01
driver = "Isaac Clayton"
+++

# Procedure: Remote Insert O(n) to O(log n) Optimization

## Problem

Remote inserts currently have O(n) worst-case complexity due to the YATA/Fugue conflict scan. After finding the origin span, we must scan forward through all potential conflicts to find the correct insertion point. Adversarial input can force linear scans.

## Goal

Reduce remote insert from O(n) to O(log n) through one of three approaches, then compare performance.

## Approaches

### Approach 1: Tree-Structured Conflict Resolution

Store items in a tree ordered by YATA/Fugue comparison, not just by document position.

**Key idea:** Instead of linear scan through conflicts, use a tree where each node's position is determined by YATA ordering. Finding the insertion point becomes tree traversal.

**Data structure:**
- Primary tree: B-tree ordered by (origin_left, YATA_tiebreaker)
- Secondary index: position -> item for document order
- Or: Augmented tree with both orderings

**Complexity:** O(log n) for both local and remote inserts.

**Tradeoff:** More complex data structure, two trees to maintain.

### Approach 3: Index by Origin

Maintain a HashMap from origin_id to list of items that share that origin.

**Key idea:** Conflicts only occur between items with the same left origin. Instead of scanning all items after the origin, only scan items that share your origin.

**Data structure:**
```rust
origin_index: HashMap<OpId, Vec<SpanIndex>>
```

**Complexity:** O(k) where k = items with same origin. For typical editing, k is small (1-3). Worst case still O(n) if all items share same origin.

**Tradeoff:** Simple to implement, helps average case significantly, doesn't fix worst case.

### Approach 4: Epoch-Based Batching

Group operations into epochs. Conflicts only possible within same epoch.

**Key idea:** An epoch is a contiguous range of operations from one editing session. When merging, operations from different epochs can be ordered by epoch without YATA scan. Only operations within the same epoch need conflict resolution.

**Data structure:**
```rust
struct Epoch {
    id: u64,
    start_seq: u32,
    end_seq: u32,
    items: Vec<Span>,
}
epochs: Vec<Epoch>
epoch_index: HashMap<OpId, EpochId>
```

**Complexity:** O(e + k) where e = epochs, k = items in overlapping epoch. Epochs are typically small.

**Tradeoff:** Requires epoch tracking, may complicate merge semantics.

## Implementation Plan

Work on each approach serially in separate branches. Do basic tuning, then compare.

### Phase 1: Approach 3 (Origin Index)

**Branch:** `opt/origin-index`

1. Create branch from current master (or crdt-comparison-study if preferred)
2. Modify `OptimizedRga` (or create new variant) to add origin index
3. Update insert logic to use origin index for conflict scan
4. Run conformance tests to verify correctness
5. Benchmark against baseline
6. Basic tuning (HashMap vs BTreeMap, Vec vs SmallVec)
7. Record results

**Implementation details:**
- Add `origin_index: HashMap<(u16, u32), SmallVec<[usize; 4]>>` to struct
- On insert: lookup `origin_index[origin_left]` to get conflict candidates
- Only scan those candidates instead of all items after origin
- Update index on insert/delete

### Phase 2: Approach 1 (Tree-Structured)

**Branch:** `opt/tree-structured`

1. Create branch from master
2. Design dual-tree or augmented tree structure
3. Implement YATA-ordered tree alongside position tree
4. Update insert to use tree traversal for conflict resolution
5. Run conformance tests
6. Benchmark
7. Basic tuning
8. Record results

**Implementation details:**
- Option A: Two separate trees (position tree + YATA tree)
- Option B: Single tree with custom comparator that handles both orderings
- Option C: B-tree where each node stores items sorted by YATA order

### Phase 3: Approach 4 (Epoch-Based)

**Branch:** `opt/epoch-based`

1. Create branch from master
2. Design epoch structure and tracking
3. Implement epoch-aware merge
4. Conflicts only resolved within epoch boundaries
5. Run conformance tests
6. Benchmark
7. Basic tuning
8. Record results

**Implementation details:**
- Epoch = contiguous sequence of operations without merge
- On merge: create new epoch boundary
- Items in different epochs: order by epoch ID (deterministic)
- Items in same epoch: use YATA scan (limited to epoch size)

## Benchmarking

Use real editing traces for comparison:
- sveltecomponent
- rustcode  
- seph-blog1
- automerge-paper

Also create adversarial test cases:
- All inserts at same position (maximum conflicts)
- Alternating inserts from two users at same position
- Random positions (baseline, should be fast for all approaches)

### Metrics

For each approach, record:
1. Time per trace (ms)
2. Ratio vs diamond-types
3. Ratio vs baseline (current O(n) implementation)
4. Memory usage
5. Code complexity (lines changed, new data structures)

## Comparison and Selection

After all three approaches are implemented:

1. Create comparison table with all metrics
2. Analyze tradeoffs:
   - Which is fastest on real traces?
   - Which handles adversarial cases best?
   - Which is simplest to maintain?
3. Select best approach for integration
4. Document decision rationale

## Branch Management

```
master
  └── opt/origin-index      (Phase 1)
  └── opt/tree-structured   (Phase 2)
  └── opt/epoch-based       (Phase 3)
```

After comparison, merge winning approach to master.

## Completion Criteria

- [ ] All three approaches implemented
- [ ] All approaches pass conformance tests
- [ ] Benchmarks run on all traces
- [ ] Adversarial tests created and run
- [ ] Comparison table complete
- [ ] Best approach selected and documented
- [ ] Winning branch merged to master
- [ ] Worklog entry created

## Notes

- Start with Approach 3 (Origin Index) as it's simplest
- If Origin Index provides sufficient improvement, may not need the others
- Tree-Structured is theoretically best but most complex
- Epoch-Based may interact with log integration design
