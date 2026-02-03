+++
model = "claude-opus-4-5"
created = 2026-02-02
modified = 2026-02-02
driver = "Isaac Clayton"
+++

# Split Continuation Bug Fix

## Goal

Fix a bug where local inserts after merge went to the wrong position in the document.

## The Bug

When a user inserted content at a position that required splitting an existing span, the new content sometimes appeared at the wrong position (e.g., at the end of the document instead of at the cursor position).

### Root Cause

The bug occurred due to incorrect RGA sibling ordering of "split continuations":

1. When inserting at position P in the middle of span S, we split S into:
   - Left part: characters before P
   - Right part: characters after P (the "split continuation")

2. The right-split gets an origin pointing to the last character of the left-split

3. The new insert ALSO gets an origin pointing to the same character

4. This makes them "siblings" in RGA terms - spans with the same origin

5. RGA orders siblings by `(user_key, seq)` - higher priority goes first

6. **Bug**: If the original span's user had higher priority than the inserting user, the right-split went BEFORE the new insert, placing the user's content after the rest of the original span

### Example

```
Document: "CDEFGHIJKLMNOPQRS" (from user U2)
User U0 inserts "XXX" at position 6 (after 'H')

Expected: "CDEFGHXXXIJKLMNOPQRS"
Actual:   "CDEFGHIJKLMNOPQRSXXX" (insert went to end!)
```

This happened because:
- Split creates: "CDEFGH" + "IJKLMNOPQRS" (right-split has origin='H')
- New insert "XXX" also has origin='H'
- U2 > U0 lexicographically, so right-split has higher priority
- RGA puts right-split before XXX

## The Fix

Introduced the concept of **split continuations** - spans created by splitting an existing span, NOT by concurrent inserts from different users.

### Identifying Split Continuations

A span is a split continuation if:
```rust
user_idx == origin_user_idx && seq == origin_seq + 1
```

This means:
- The span's user is the same as its origin's user (same person's content)
- The span's sequence number immediately follows the origin (contiguous content that was split)

### Excluding from Sibling Ordering

Split continuations are excluded from RGA sibling ordering in both:

1. **`insert_span_at_pos_optimized()`** - local insert path
2. **`insert_span_rga()`** - merge/remote insert path

This ensures:
- New inserts go at the cursor position (before the right-split)
- Split continuations stay in their natural position (after real concurrent inserts)
- Both local and merge paths produce identical results (convergence maintained)

## Code Changes

### `src/crdt/rga.rs`

In both `insert_span_at_pos_optimized` and `insert_span_rga`, added checks to skip split continuations during sibling ordering:

```rust
// Skip split continuations - they're not real siblings
if sibling.is_split_continuation() {
    continue;
}
```

The `is_split_continuation()` method already existed but was unused:
```rust
fn is_split_continuation(&self) -> bool {
    self.user_idx == self.origin_user_idx && self.seq == self.origin_seq + 1
}
```

### `tests/rga_fuzz.rs`

Updated test expectations to match correct behavior:
- `test_local_insert_sibling_order`: Now expects insert at cursor position
- `test_trace_merge_step_by_step`: Updated expected merge result

## Test Results

| Test Suite | Result |
|------------|--------|
| Library tests (83) | All pass |
| coverage_gaps (36) | All pass |
| rga_fuzz | 130/142 pass |

### Remaining Failures (12 tests)

All 12 failing tests are related to **delete propagation behavior** - a separate, pre-existing issue:

- `test_delete_not_propagated_in_merge`
- `test_delete_local_only_two_users`
- `test_multiple_deletes_not_propagated`
- `prop_merge_does_not_propagate_deletes`
- etc.

These tests expect deletes to NOT be propagated during merge, but the actual `merge()` implementation has Phase 2 that explicitly syncs deletions. This is a design question (should deletes propagate?) not related to the split continuation fix.

## Key Insight

The distinction between "real concurrent inserts" and "split continuations" is crucial for correct CRDT behavior:

- **Real concurrent inserts**: Created independently by different users at the same logical position. Must be ordered deterministically using RGA priority.

- **Split continuations**: Created locally when splitting a span to insert new content. Should NOT participate in sibling ordering - they're just the "rest" of an existing span and should stay after new inserts.

This distinction ensures that local editing behavior matches user intent (content appears at cursor) while still maintaining CRDT convergence guarantees.

## Files Changed

- `src/crdt/rga.rs` - Split continuation exclusion in both insert paths
- `tests/rga_fuzz.rs` - Updated test expectations
