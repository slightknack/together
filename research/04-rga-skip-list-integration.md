---
model = "claude-opus-4-5"
created = "2026-01-30"
modified = "2026-01-30"
driver = "Isaac Clayton"
---

# RGA Skip List Integration

## Status

**Completed:**
- `WeightedList<T>` implemented in `src/crdt/weighted_list.rs`
- O(n) baseline implementation with correct semantics
- Tests passing for: insert, find_by_weight, update_weight, iterate

**Remaining:**
- Replace `Vec<Span>` with `WeightedList<Span>` in RGA
- Optimize `WeightedList` to O(log n) using skip list levels

## Problem Statement

The RGA currently uses `Vec<Span>` which has O(n) operations:
- Position lookup: O(n) linear scan
- Insertion: O(n) due to Vec::insert shifting elements
- The goal is O(log n) operations using a skip list

## Current RGA Structure

```rust
pub struct Rga {
    spans: Vec<Span>,                    // Document order
    columns: HashMap<KeyPub, Column>,    // Per-user content storage
    visible_len: u64,                    // Cached visible length
    cursor: (u64, usize, u64),           // Position cache
}

pub struct Span {
    user: KeyPub,
    seq: u64,
    len: u64,
    origin: Option<ItemId>,
    content_offset: usize,
    deleted: bool,
}
```

## Two Approaches

### Approach A: Use SkipList<Span> directly

Store spans in the existing `SkipList<T>` where `T = Span`.

**Challenges:**
1. The skip list tracks *item count* (number of spans), not *visible character count*
2. RGA needs to find spans by visible position, not by span index
3. The skip list's widths would need to track character counts, not span counts

**Would require:**
- Modifying `SkipList` to accept a custom "weight" function per item
- Or creating a wrapper that maintains a parallel character-count skip list

### Approach B: Specialized span skip list

Create a new skip list specifically for spans where:
- Each node holds one span (no chunking/unrolling)
- Widths track visible character counts
- API designed for RGA operations

**Challenges:**
1. Duplicates skip list logic
2. Need to carefully maintain width invariants

### Approach C: Augment existing SkipList with weighted items

Add a `weight` concept to the existing skip list:
- `SkipList<T>` becomes `SkipList<T, W>` where W is the weight type
- A weight function `fn(&T) -> W` determines each item's contribution
- Widths track cumulative weights, not item counts
- Position lookup uses weights

This is the cleanest approach because:
- Reuses existing tested skip list code
- Makes the weight semantics explicit
- Allows the RGA to use character counts as weights

## Chosen Approach: Weighted Skip List

### Design

```rust
pub struct WeightedSkipList<T, F>
where
    F: Fn(&T) -> u64,
{
    inner: SkipList<T>,
    weight_fn: F,
}
```

Or simpler: add an optional weight to items:

```rust
pub struct SkipList<T> {
    // existing fields...
}

impl<T> SkipList<T> {
    /// Insert with custom weight (defaults to 1)
    pub fn insert_weighted(&mut self, index: usize, item: T, weight: u32);
    
    /// Find by weighted position (sum of weights)
    pub fn find_weighted(&self, weight_pos: u64) -> Option<(usize, u64)>;
}
```

Actually, the cleanest is to parameterize the skip list:

```rust
pub trait Weighted {
    fn weight(&self) -> u64;
}

impl Weighted for Span {
    fn weight(&self) -> u64 {
        self.visible_len()
    }
}
```

But this requires changing width storage from `u32` to `u64` for character counts.

## Simplest Path Forward

Rather than generalizing, recognize that:
1. The current `SkipList<T>` tracks *item count* via widths
2. For RGA, we need to track *visible character count*
3. These are fundamentally different metrics

**Solution:** Keep the current `SkipList` for item-based indexing, and create a parallel structure for character-based indexing.

But actually, the RGA only needs character-based indexing. It never needs "give me the 5th span" - it needs "give me the span containing character 500".

So the simplest path is:
1. Modify `SkipList` widths to use `u64` (to hold character counts)
2. When inserting a span, set its width to `span.visible_len()`
3. Position lookup uses these character-based widths

## Invariants for RGA Skip List

### Invariant 1: Width Sum Equals Visible Length
```
sum of all widths at level 0 = total visible character count
```

### Invariant 2: Width at Level 0 Equals Span's Visible Length
```
for each span node N:
    N.widths[0] = N.span.visible_len()
```

### Invariant 3: Higher Level Widths are Cumulative
```
for each node N at level L > 0:
    N.widths[L] = sum of visible_len for all spans from N to N.next[L]
```

### Invariant 4: Document Order Preserved
```
level-0 traversal yields spans in document order
```

### Invariant 5: CRDT Properties
```
for any two spans A and B inserted at the same position:
    order is determined by (user, seq) descending
    (higher user/seq comes first)
```

## Operations Needed

### 1. find_visible_pos(pos: u64) -> (Idx, offset: u64)
Find the span containing visible position `pos` and the offset within that span.

### 2. insert_at_visible_pos(pos: u64, span: Span)
Insert a span at the given visible position (for local edits).

### 3. insert_after_span(pred: Option<Idx>, span: Span)
Insert a span after a known predecessor (for CRDT apply).

### 4. mark_deleted(idx: Idx)
Mark a span as deleted and update all affected widths.

### 5. split_span(idx: Idx, offset: u64) -> Idx
Split a span at the given offset, returning the new right half's index.

### 6. iter() -> impl Iterator<Item = &Span>
Iterate spans in document order.

### 7. find_span_by_id(id: &ItemId) -> Option<Idx>
Find a span by its ItemId (needs auxiliary index).

## Implementation Plan

### Phase 1: Add weighted width support to SkipList

1. Change `widths` from `[u32; MAX_HEIGHT]` to `[u64; MAX_HEIGHT]`
2. Add `insert_with_weight(index, item, weight)` method
3. Add `find_by_weight(weight_pos) -> Option<(index, local_offset)>` method
4. Update all width calculations to use the provided weight

### Phase 2: Create RGA-specific wrapper

```rust
pub struct RgaSpanList {
    list: SkipList<Span>,
    id_index: HashMap<(KeyPub, u64), Idx>,  // ItemId -> span index
}
```

### Phase 3: Replace Vec<Span> in RGA

1. Replace `spans: Vec<Span>` with `spans: RgaSpanList`
2. Update `find_visible_pos` to use `spans.find_by_weight`
3. Update insertion methods
4. Update deletion to call `spans.update_weight` when visible_len changes

## Test Plan

### Unit Tests for Weighted Skip List

1. `insert_weighted_single`: Insert one item with weight 10, verify find_by_weight(5) returns it
2. `insert_weighted_multiple`: Insert items with weights [5, 10, 3], verify find_by_weight at boundaries
3. `weight_sum_invariant`: After random insertions, verify weight sum equals expected
4. `find_by_weight_edge_cases`: pos=0, pos=total-1, pos beyond end

### Integration Tests for RGA with Skip List

1. `rga_insert_find_roundtrip`: Insert text, verify find_visible_pos returns correct span
2. `rga_delete_updates_weights`: Delete span, verify weights are updated
3. `rga_split_preserves_invariants`: Split span, verify both halves have correct weights
4. `rga_crdt_ordering`: Concurrent inserts at same position have deterministic order

### Property-Based Tests

1. `random_ops_preserve_invariants`: Random insert/delete/split maintain all invariants
2. `find_agrees_with_iter`: find_by_weight(pos) agrees with manual iteration
3. `weight_sum_equals_visible_len`: Always true after any operation

## Open Questions

1. Should `Span` store its own `Idx` for O(1) self-reference? (Memory vs convenience tradeoff)
2. Should the `id_index` HashMap be part of `RgaSpanList` or kept in `Rga`?
3. How to handle span splits - does the right half need a new ItemId mapping?

## Implementation Status

### WeightedList (Completed)

Instead of modifying the existing `SkipList`, a new `WeightedList<T>` was created in `src/crdt/weighted_list.rs`. This provides:

```rust
pub struct WeightedList<T> {
    nodes: Vec<Node<T>>,   // Arena allocation
    head: Idx,             // Sentinel node
    total_weight: u64,     // Sum of all weights
    len: usize,            // Number of items
    // ...
}

impl<T> WeightedList<T> {
    pub fn new() -> Self;
    pub fn len(&self) -> usize;
    pub fn total_weight(&self) -> u64;
    pub fn insert(&mut self, index: usize, item: T, weight: u64);
    pub fn find_by_weight(&self, pos: u64) -> Option<(usize, u64)>;
    pub fn get(&self, index: usize) -> Option<&T>;
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T>;
    pub fn update_weight(&mut self, index: usize, new_weight: u64) -> u64;
    pub fn iter(&self) -> impl Iterator<Item = &T>;
}
```

**Current complexity**: O(n) for all operations (linked list at level 0 only).

**Rationale for separate struct**: The unrolled `SkipList<T>` has complex width semantics tied to chunk sizes. Creating a simpler weighted list allows:
1. Correct semantics first, optimization later
2. Clear separation between item-count indexing and weight-based indexing
3. Easier testing and debugging

### Next Steps

1. Replace `Vec<Span>` with `WeightedList<Span>` in RGA
2. Update RGA methods to use WeightedList API
3. Optimize WeightedList to O(log n) by using skip list levels
4. Add ItemId index for O(1) span lookup by ID
3. Write tests for `RgaSpanList`
4. Implement `RgaSpanList`
5. Replace `Vec<Span>` in `Rga`
6. Benchmark against baseline
