---
model = "claude-opus-4-5"
created = "2026-01-31"
modified = "2026-01-31"
driver = "Isaac Clayton"
---

# Weighted Skip List: Lessons Learned

## Summary

Attempted to create a WeightedSkipList to replace WeightedList for O(log n) weight-based lookup.
The implementation failed: 2000-12000x slower than diamond-types on benchmarks.

## The Goal

Replace the chunked WeightedList (O(sqrt n) operations) with a skip list that tracks weights
for O(log n) operations on all operations:
- `find_by_weight(weight)` - find item containing weight position
- `insert(index, item, weight)` - insert at index
- `remove(index)` - remove at index
- `update_weight(index, new_weight)` - change item's weight

## Why It Failed

### Problem 1: Dual Tracking Requirement

A weighted skip list needs to track TWO different things:
1. **Item count** - for index-based operations (insert at index 5)
2. **Weight sum** - for weight-based operations (find position at weight 100)

The existing SkipList tracks only item counts via `widths[level]`. To support both,
we would need either:
- Two separate width arrays per node (doubles memory overhead)
- A single array tracking weights, with O(n) item counting

I chose option B, which made index-based operations O(n).

### Problem 2: Width Semantics Confusion

In a standard skip list:
- `widths[level]` = number of items skipped by following `next[level]`
- This enables O(log n) "find item at index K"

For a weighted skip list:
- `widths[level]` = total weight of items in the span
- This enables O(log n) "find item at weight W"

But these two uses are incompatible. When I tried to use weights for `widths[level]`,
I lost the ability to efficiently find items by index.

### Problem 3: O(n) Fallback

When find_by_weight failed with O(log n) logic (due to incorrect width tracking),
I fell back to O(n) level-0 traversal. But this made the skip list structure useless -
we were just iterating through a linked list.

The overhead of the skip list (16 forward pointers per node, height tracking, etc.)
made it slower than a simple Vec.

## Performance Results

| Trace | WeightedSkipList | WeightedList | Diamond-types |
|-------|------------------|--------------|---------------|
| sveltecomponent | 2.4s | 5ms | 2.8ms |
| rustcode | 9.7s | 6.9ms | 4ms |
| seph-blog1 | 85s | 12ms | 6.5ms |

WeightedSkipList was 200-7000x slower than WeightedList.

## What Would Work

### Option A: Dual-Width Skip List

Track both item counts and weight sums:
```rust
struct Node<T> {
    item: T,
    weight: u64,
    height: u8,
    next: [Idx; MAX_HEIGHT],
    item_widths: [u32; MAX_HEIGHT],   // For index-based ops
    weight_widths: [u64; MAX_HEIGHT], // For weight-based ops
}
```

This would double the per-node memory but enable O(log n) for both operations.

### Option B: B-Tree with Augmented Data

Use a B-tree where each internal node stores:
- Pointers to children
- Total item count in subtree
- Total weight in subtree

This is what diamond-types uses (ContentTree).

### Option C: Two Separate Structures

Keep the current WeightedList for weight-based ops (O(sqrt n) via Fenwick tree),
and add a separate index if index-based ops become a bottleneck.

## Recommendations

1. The current WeightedList with Fenwick tree is good enough (1.7-1.9x vs diamond-types)
2. If more optimization is needed, consider Option B (B-tree with augmented data)
3. Do not attempt Option A without careful benchmarking - the memory overhead may hurt cache performance

## Files

The failed WeightedSkipList implementation is in `src/crdt/weighted_skip_list.rs`.
It is not used by RGA but is kept for reference.
