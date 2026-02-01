+++
model = "claude-opus-4-5"
created = 2026-01-31
modified = 2026-01-31
driver = "Isaac Clayton"
+++

# Fuzz Testing Session

## Goal

Implement property-based fuzz testing for RGA as part of optimization work (following procedures/00-optimize.md).

## What Was Done

### Created `tests/rga_fuzz.rs`

Comprehensive property-based tests using proptest:

1. **Invariant tests** (PASSING)
   - `rga_invariants_hold`: Length matches content, slice works correctly
   - `rga_buf_invariants_hold`: RgaBuf maintains consistency after operations

2. **Equivalence tests** (PASSING)
   - `rga_and_rga_buf_equivalent`: Rga and RgaBuf produce same output for same operations

3. **Merge tests** (PARTIAL)
   - `merge_idempotent`: PASSING - `merge(a, a) == a`
   - `merge_with_empty_is_identity`: PASSING - `merge(a, empty) == a`
   - `merge_commutative`: DISABLED - Found bugs, see research/28-rga-merge-issues.md
   - `multi_user_convergence`: DISABLED - Found bugs

4. **Pattern tests** (PASSING)
   - `sequential_typing_at_end`: Common typing pattern
   - `backspace_pattern`: Type then delete
   - `insert_in_middle`: Insert between existing content
   - `many_small_operations`: Stress test with 500-1000 operations

### Added Clone to Rga and BTreeList

Required for merge testing - need to clone documents before merging.

### Fixed merge deletion handling

Original bug: When merging a span that was deleted in the source, we called `delete_by_id` which only deleted the first character of multi-character spans.

Fix: Set `new_span.deleted = span.deleted` when creating the span, so it's inserted already deleted.

### Discovered fundamental merge issue

The RGA merge algorithm is not commutative when documents have been independently edited with insertions that cause span splits. See research/28-rga-merge-issues.md for detailed analysis.

## Test Results

```
running 9 tests
test backspace_pattern ... ok
test insert_in_middle ... ok
test many_small_operations ... ok
test merge_idempotent ... ok
test merge_with_empty_is_identity ... ok
test rga_and_rga_buf_equivalent ... ok
test rga_buf_invariants_hold ... ok
test rga_invariants_hold ... ok
test sequential_typing_at_end ... ok

test result: ok. 9 passed; 0 failed
```

## Key Insight

The property-based tests found real bugs that unit tests missed. The merge commutativity issue is subtle - it only manifests when:
1. Multiple users insert at position 0 (creating no-origin spans)
2. One user then inserts in the middle (causing splits)
3. The split creates an origin relationship that didn't exist before

This changes the merge semantics because the split span now has an origin, placing it differently than other no-origin spans during merge.

## Files Changed

- `tests/rga_fuzz.rs` (new)
- `src/crdt/rga.rs` (Clone derive, merge fixes)
- `src/crdt/btree_list.rs` (Clone derive)

## Next Steps

1. Resolve the fundamental design question: operation-based vs state-based merge
2. Consider separating temporal (causal) relationships from document structure
3. Re-enable merge tests once the design is sound
4. Continue to splay tree research and implementation
