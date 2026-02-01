+++
model = "claude-opus-4-5"
created = 2026-01-31
modified = 2026-01-31
driver = "Isaac Clayton"
+++

# RGA Merge Issues Discovered Through Property-Based Testing

## Summary

Property-based fuzz testing revealed that the current RGA merge implementation is not commutative. Merging document A into B produces different results than merging B into A when both documents have been independently edited with insertions in the middle.

## The Core Problem

The RGA algorithm works correctly for its primary use case: replaying operation logs. When operations are applied in causal order (respecting the origin chain), the results are deterministic.

The problem arises with **state-based merge** of independently-edited documents. The current implementation iterates over spans in document order and uses each span's stored `origin` to determine placement. This breaks down because:

1. **Spans without explicit origin**: When a span is inserted at position 0, it has no origin (the document beginning). Multiple such spans from different users are ordered by `(user, seq)` descending.

2. **Span splitting**: When an insert happens in the middle of an existing span, the span is split. The right part is given an origin pointing to the left part. This origin is stored as a span index + offset.

3. **Origin relationships don't compose correctly**: When merging, a span's origin must be resolved in the target document. But the origin was computed relative to the source document's structure, which may differ.

## Concrete Failing Case

```
User1: Insert "a" at pos 0
       Result: "a"

User2: Insert "aa" at pos 0      -> "aa"
       Insert "aa" at pos 1      -> splits "aa", inserts "aa" between
       Insert "b" at pos 2       -> further splits
       Result: "aabaa"

Merge user2 into user1: "aaaaba"
Merge user1 into user2: "aaabaa"
```

The 'a' from user1 ends up in different positions depending on merge order.

## Why This Happens

When user2 inserts "aa" at position 1 in their document "aa":
- The original "aa" span (seq 0-1, no origin) is split
- Left part: 'a' (seq 0, no origin)
- Right part: 'a' (seq 1, origin = seq 0)
- Inserted: 'aa' (seq 2-3, origin = seq 0)

The right part now has an origin pointing to seq 0. But it originally had NO origin - it was part of a no-origin span at the document beginning.

When merging into user1's document:
- User1's 'a' (seq 0, no origin) is positioned by RGA ordering with other no-origin spans
- User2's spans with explicit origins are positioned after their origins
- User2's span with seq 1 now has an origin, so it's positioned differently than user1's no-origin span

The fundamental issue: **splitting a no-origin span creates a span with an origin**, changing its merge semantics.

## Questions to Resolve

1. **Why do we ever have spans without origins besides the initial edit?**
   - Currently: Every insert at position 0 has no origin
   - Problem: This means multiple users inserting at the beginning create multiple no-origin spans
   - Alternative: Only the very first character in the entire CRDT should have no origin?

2. **Should the temporal order (operation log) be separate from the document tree?**
   - The span's origin is trying to encode both "what character was this inserted after" (temporal/causal) and "where does this belong in the document" (structural)
   - These are conflated in the current design
   - Separating them might give cleaner semantics

3. **Is state-based merge even the right model?**
   - RGA is naturally operation-based
   - The current merge tries to reconstruct operations from document state
   - This loses information that was present in the original operation stream

## Potential Approaches

### A. Never have no-origin spans after the first character

Every insert (except the very first in the entire CRDT) would have an explicit origin. Inserts at position 0 would have origin = "beginning sentinel" rather than "no origin".

This requires a sentinel value or a way to distinguish "inserted at beginning" from "no origin information".

### B. Separate causal graph from document structure

Maintain two structures:
- A causal graph (operation log) that tracks what operation each item depends on
- A document tree/list that tracks current positions

Merge would operate on the causal graph, and document structure would be derived.

### C. Use a different CRDT (Logoot/LSEQ)

Logoot and LSEQ assign globally unique, ordered identifiers to each character. These identifiers are dense (can always insert between any two) and globally ordered without needing explicit origins.

This avoids the origin-tracking problem entirely but has different tradeoffs (identifier size, allocation strategy).

### D. Fix the split behavior

When splitting a no-origin span, perhaps both parts should remain no-origin? The right part would be ordered by `(user, seq)` among no-origin spans.

But this might break other invariants - need to think through carefully.

## Current State

The fuzz tests for merge commutativity and multi-user convergence are disabled with a TODO comment. The tests that pass are:
- `rga_invariants_hold`: Length and slice consistency
- `rga_buf_invariants_hold`: RgaBuf consistency
- `rga_and_rga_buf_equivalent`: Rga and RgaBuf produce same output
- `merge_idempotent`: `merge(a, a) == a`
- `merge_with_empty_is_identity`: `merge(a, empty) == a`
- `sequential_typing_at_end`: Common typing pattern
- `backspace_pattern`: Common editing pattern
- `insert_in_middle`: Common editing pattern
- `many_small_operations`: Stress test

The failing tests are disabled:
- `merge_commutative`: `merge(a, b) == merge(b, a)`
- `multi_user_convergence`: All merge orders converge

## Files Changed

- `src/crdt/rga.rs`: Added `Clone` derive, fixed merge to copy deleted status, various attempted fixes
- `src/crdt/btree_list.rs`: Added `Clone` derive
- `tests/rga_fuzz.rs`: Created comprehensive property-based tests, disabled failing merge tests

## Resolution

This issue has been fixed. See `research/29-rga-fix-approach.md` for the solution.

The fix involved two key changes:
1. Store origins as item IDs (user_idx, seq) instead of span indices
2. Skip entire subtrees when ordering siblings in the RGA tree

All fuzz tests now pass, including `merge_commutative` and `multi_user_convergence`.
