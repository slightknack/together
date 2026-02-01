+++
model = "claude-opus-4-5"
created = 2026-01-31
modified = 2026-01-31
driver = "Isaac Clayton"
+++

# RGA Dual-Origin Fix Session

This document tracks the solution tree and learnings for fixing the RGA merge
commutativity bug through the dual-origin (YATA/FugueMax) approach.

## Current Status

**COMPLETE** - All tests pass.

- 20/20 fuzz tests pass (including extended runs with 5000 cases)
- 83/83 library tests pass
- No compiler warnings

## Solution Tree

```
A. Add dual origins (YATA/FugueMax approach) [CHOSEN - SUCCESS]
├── A1. Data structure changes [DONE]
│   ├── Add right_origin fields to Span [DONE]
│   ├── Update Op::Insert for right_origin [DONE]
│   └── Update split() semantics [DONE]
│
├── A2. Algorithm changes [DONE]
│   ├── Update insert_span_rga with YATA ordering [DONE]
│   ├── Implement subtree tracking [DONE]
│   ├── Fix right origin comparison [DONE]
│   │   ├── A2.1 Null comparison logic [DONE - was already correct]
│   │   └── A2.2 Partial span right_origin [DONE - key fix]
│   │
│   └── Update local insert for right_origin capture [DONE]
│
├── A3. Merge function changes [DONE]
│   ├── Topological sort for causal ordering [DONE]
│   ├── Partial span handling [DONE]
│   └── Right origin preservation during merge [DONE]
│
└── A4. Verification [DONE]
    ├── All 20 fuzz tests pass [DONE]
    ├── Extended proptest (5000 cases) [DONE]
    └── Library test suite passes [DONE]

B. Explicit tree structure [NOT NEEDED]
C. Port Yjs directly [NOT NEEDED]
```

## Learnings

### L1: Position-based boundaries break commutativity
**Attempt:** Used document position of right_origin as scan boundary
**Result:** Failed - positions differ between replicas
**Learning:** YATA compares origins as IDs (user, seq), never as positions.
The right_origin is a logical reference, not a physical location.

### L2: Subtree tracking is necessary
**Attempt:** Simple scan without tracking which spans are in subtree
**Result:** Failed - couldn't distinguish descendants from different branches
**Learning:** Must track (user_idx, seq_start, seq_end) ranges to know when
we've exited the subtree rooted at our origin.

### L3: Split semantics matter
**Attempt:** Updated left part's right_origin during split
**Result:** Failed - right_origin is an insertion-time property
**Learning:** When splitting a span, right_origin captures what was to the right
AT INSERTION TIME, not current document state. Don't modify during splits.

### L4: Merge must process spans in causal order
**Attempt:** Iterated spans in document order during merge
**Result:** Failed - origin might not exist yet when we try to insert
**Learning:** Must topologically sort spans before inserting. A span can only
be inserted after its left_origin exists in the target document.

### L5: Partial span left_origin needs adjustment
**Attempt:** When inserting partial span, used original span's left_origin
**Result:** Failed for partial spans
**Learning:** When only inserting the missing suffix of a coalesced span,
the left_origin should be the last existing char, not the original origin.

### L6: Null right_origin comparison is correct
**Attempt:** Analyzed null right_origin comparison logic
**Result:** Logic was already correct
**Learning:** The comparison logic was correct:
- null RO = "inserted at end" = infinite = comes AFTER finite RO
- When other has null RO and we have finite RO -> we come before -> break
- When we have null RO and other has finite RO -> other comes before -> skip

### L7: Partial spans must keep original right_origin (KEY FIX)
**Attempt:** Set right_origin=None for partial spans (missing_offset > 0)
**Result:** Failed - spans with None get treated as "end of doc"
**Learning:** When items are coalesced into a span, they ALL share the same
insertion context. The right_origin applies to the ENTIRE span. If user types
"hello" coalesced into one span with right_origin=X, then ALL of "hello" was
inserted with X to the right. When merging just "llo", it should still have
right_origin=X.

## Final Summary

The fix required implementing the YATA/FugueMax dual-origin algorithm:

1. **Data structures:** Added right_origin fields to Span and Op::Insert
2. **Algorithm:** Implemented YATA ordering using (left_origin, right_origin, uid) comparison
3. **Merge:** Topological sort + correct origin handling for partial spans
4. **Key insight:** Coalesced spans share insertion context - all items have the same right_origin

The dual-origin approach eliminates the "subtree boundary detection" problem
that plagued single-origin RGA. With right_origin, we can deterministically
order siblings without needing complex heuristics.
