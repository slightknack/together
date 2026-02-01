+++
model = "claude-opus-4-5"
created = 2026-01-31
modified = 2026-01-31
driver = "Isaac Clayton"
+++

# Weighted Skip List for O(log n) Position Lookup

## Problem

The current `WeightedList<T>` uses a Vec, giving O(n) for all operations:
- `find_by_weight(pos)`: O(n) linear scan
- `insert(index, item, weight)`: O(n) from Vec::insert
- `remove(index)`: O(n) from Vec::remove

With 19,749 patches in the sveltecomponent trace:
- O(n) per operation = O(n^2) total = 390 million operations
- O(log n) per operation = O(n log n) total = 282k operations
- **Theoretical speedup: 1384x**

## Goal

Replace the Vec-backed WeightedList with a skip list that maintains weight sums at each level, enabling O(log n) operations.

## Skip List Basics

A skip list is a probabilistic data structure with multiple layers of linked lists:

```
Level 2: HEAD ---------> [B] ---------------------------> NULL
Level 1: HEAD ---------> [B] --------> [D] -----------> NULL
Level 0: HEAD -> [A] -> [B] -> [C] -> [D] -> [E] -> NULL
```

Each node has a random height. Search starts at the top level and descends when the target is past the current span.

## Weighted Skip List Design

For RGA, we need to find items by **cumulative weight** (visible character count), not by index.

### Node Structure

```rust
struct Node<T> {
    item: T,
    weight: u64,           // This item's weight
    height: u8,
    next: [Idx; MAX_HEIGHT],
    widths: [u64; MAX_HEIGHT],  // Cumulative weight to next[level]
}
```

At each level, `widths[level]` stores the total weight of items from this node (inclusive) to `next[level]` (exclusive).

For level 0: `widths[0] = weight` (just this item's weight)
For higher levels: `widths[level] = sum of weights of all items in the span`

### Find by Weight

```rust
fn find_by_weight(&self, pos: u64) -> Option<(usize, u64)> {
    let mut idx = self.head;
    let mut cumulative = 0u64;
    let mut item_index = 0usize;
    
    for level in (0..MAX_HEIGHT).rev() {
        while idx != NULL {
            let node = self.node(idx);
            if level >= node.height() { break; }
            let next = node.next[level];
            if next == NULL { break; }
            
            let span_weight = node.widths[level];
            if cumulative + span_weight <= pos {
                cumulative += span_weight;
                // Count items in this span (need skip counts too)
                idx = next;
            } else {
                break;
            }
        }
    }
    
    // Now at level 0, idx is the node containing pos
    if idx == self.head {
        idx = self.node(self.head).next[0];
    }
    
    // Return (item_index, offset_within_item)
    Some((item_index, pos - cumulative))
}
```

**Problem**: To return `item_index`, we also need to track item counts, not just weights.

### Solution: Track Both Weights and Item Counts

```rust
struct Node<T> {
    item: T,
    weight: u64,
    height: u8,
    next: [Idx; MAX_HEIGHT],
    widths: [u64; MAX_HEIGHT],   // Weight spans
    skips: [u32; MAX_HEIGHT],    // Item count spans
}
```

At each level:
- `widths[level]` = total weight from this node to next[level]
- `skips[level]` = number of items from this node to next[level]

For level 0: `widths[0] = weight`, `skips[0] = 1`

### Insert with Weight

When inserting at index `i` with weight `w`:

1. Find the path using `skips` to reach index `i`
2. Record predecessors at each level
3. Create new node with random height
4. Wire up forward pointers
5. Update `widths` and `skips` for predecessors and new node

The key insight: predecessors need their spans split.

```
Before: pred ---(skip=3, width=100)---> next
Insert at position 1 within this span, weight=10

After:
pred ---(skip=1, width=w1)---> NEW ---(skip=2, width=w2+10)---> next

Where w1 = weight of items 0 (from pred's span start)
      w2 = weight of items 1-2 (remaining span)
```

### Update Weight

When updating the weight of item at index `i`:

1. Find the item using `skips`
2. Update `node.weight`
3. Update `widths` along the path from head to item

This is where having the update path recorded is crucial.

### Remove

When removing item at index `i`:

1. Find predecessors at each level using `skips`
2. Unlink the node at each level where it appears
3. Merge predecessor's span with removed node's span:
   - `pred.skips[level] += removed.skips[level] - 1`
   - `pred.widths[level] += removed.widths[level] - removed.weight`

## Complexity Analysis

- **find_by_weight**: O(log n) - descend through levels
- **insert**: O(log n) - find path + update pointers
- **remove**: O(log n) - find path + update pointers
- **update_weight**: O(log n) - find path + update widths

Space: O(n) expected (each node has O(1) expected height)

## Implementation Strategy

1. Start with the existing `SkipList<T>` from skip_list.rs as a reference
2. Add `weight` field to nodes
3. Add `widths` array tracking cumulative weights
4. Modify `find_path` to work with both skips (for index) and widths (for weight position)
5. Add `find_by_weight` method
6. Ensure `insert` and `remove` maintain both `skips` and `widths` invariants

## Key Invariants

1. `sum of widths[0] for all nodes = total_weight`
2. `sum of skips[0] for all nodes = len`
3. For each node: `widths[0] = weight`
4. For each node: `skips[0] = 1`
5. For higher levels: `widths[level] = sum of weights in span`, `skips[level] = count of items in span`

## Testing Strategy

1. Property: `find_by_weight(w)` agrees with linear scan
2. Property: After any sequence of operations, invariants hold
3. Property: `iter().count() == len()`
4. Property: `iter().map(|n| n.weight).sum() == total_weight()`

## References

- Pugh, William. "Skip Lists: A Probabilistic Alternative to Balanced Trees" (1990)
- Diamond-types JumpRope: uses similar approach for byte-offset indexing
- Current skip_list.rs: working implementation with item-count indexing
