+++
model = "claude-opus-4-5"
created = 2026-01-31
modified = 2026-01-31
driver = "Isaac Clayton"
+++

# RGA Fix Approach: Separating Causal Identity from Span Structure

## Summary

The merge non-commutativity bug stems from conflating two distinct concepts:
1. **Causal identity**: The unique ID of each character (user, seq)
2. **Span structure**: How characters are grouped for storage efficiency

When a span is split, the current implementation creates new origin pointers that reference span indices. This means the same logical character can have different origin representations depending on whether it was split before or during merge.

## Key Insight from Research

Diamond-types and Yjs both solve this by storing **item IDs** (user, seq) as origins, not span/position references. The origin is immutable: it records which character this was inserted after at the time of the original operation. This never changes, even when spans are split or merged.

From diamond-types INTERNALS.md:
> "Unlike CRDT based systems, the operations are stored in this original form, regardless of the sequencing algorithm used."

From Yjs:
> "Origins represent the position of its neighbors at the moment of original insertion, while these pointers represent current placement of a block in relation to others."

## Root Cause Analysis

### Current Implementation

In `rga.rs`, origins are stored as `OriginRef { span_idx: u32, offset: u32 }`. This references the physical span structure.

When `insert_span_at_pos_optimized` splits a span:
```rust
right.set_origin(OriginRef::some(
    prev_idx as u32,              // Points to the left part
    (split_offset - 1) as u32,    // Last character of the left part
));
```

The problem: `prev_idx` is a span index in the current document. This index can differ between documents if they have different span structures.

### The Specific Bug

Consider user2's document after operations:
1. Insert "aa" at pos 0 -> Span(user=2, seq=0-1, origin=None)
2. Insert "aa" at pos 1 -> Splits span 0, creates:
   - Span(user=2, seq=0, origin=None)
   - Span(user=2, seq=2-3, origin=(span_idx=0, offset=0))  <- inserted span
   - Span(user=2, seq=1, origin=(span_idx=0, offset=0))    <- right part of split

The right part (seq=1) now has origin=(span_idx=0, offset=0), meaning "after character at span 0, offset 0".

During merge into user1's document:
- User1's document has a different span structure
- span_idx=0 in user2's document does not map to the same character in user1's document
- The origin resolution produces different results depending on which document we merge into

## The Fix

### Change 1: Store Origins as Item IDs

Replace `OriginRef { span_idx: u32, offset: u32 }` with `OriginId { user_idx: u16, seq: u32 }`.

This directly identifies the character by its unique ID (user, sequence number), which is invariant across all documents.

### Change 2: Never Create New Origins During Split

When splitting a span, both parts inherit the original origin (or lack thereof):
- Left part: keeps original origin
- Right part: also keeps original origin (but its *actual* causal parent is the last character of the left part, which is implicit from the sequence numbers)

Wait - this doesn't work either. The right part genuinely was inserted after the left part's last character. We need to record this.

### Revised Approach: Store Origin as Item ID

The origin should be stored as an immutable item ID:
```rust
struct OriginId {
    user_idx: u16,
    seq: u32,
}
```

When creating a span during local insert:
- Look up the character at `pos - 1`
- Store its (user_idx, seq) as the origin

When splitting a span:
- Left part keeps its original origin
- Right part's origin is the left part's last character: (left.user_idx, left.seq + left.len - 1)

This is correct because:
- The original span was contiguous: characters seq, seq+1, ..., seq+len-1
- Each character (except the first) was conceptually inserted after the previous one
- So the right part's first character (seq + offset) was inserted after (seq + offset - 1)

### Change 3: Resolve Origins During Merge by Item ID Lookup

During merge, instead of using span_idx, look up the origin item by its (user, seq):

```rust
let origin = if span.has_origin() {
    let origin_id = span.origin();
    Some(ItemId { user: origin_id.user, seq: origin_id.seq })
} else {
    None
};
```

This is already what the code does conceptually, but the indirection through span_idx breaks it.

## Implementation Plan

1. Change `OriginRef` to store (user_idx, seq) instead of (span_idx, offset)
2. Update `insert_span_at_pos_optimized` to compute origin as (user_idx, seq) of the predecessor character
3. Update `split` to compute the right part's origin as (user_idx, seq) of the left part's last character
4. Update merge to resolve origins directly without span index lookup
5. Update `insert_span_rga` to find origin by item ID, not span index

## Alternative: Separate Causal Graph

A more radical approach (like diamond-types) separates:
- The operation log (causal graph with parent pointers)
- The document state (current content with position information)

This is cleaner but requires more restructuring. The item ID approach above is a minimal fix that preserves the current architecture.

## Testing

The fix should make these tests pass:
- `merge_commutative`
- `multi_user_convergence`

Run: `cargo test --test rga_fuzz`

## Solution Implemented

The fix involved two key changes:

### Change 1: Store Origins as Item IDs

Replaced `OriginRef { span_idx: u32, offset: u32 }` with `OriginId { user_idx: u16, seq: u32 }`.

This stores the origin as an immutable item ID (user, sequence number) that is invariant across document structures, rather than a span index that can differ between documents.

### Change 2: Skip Subtrees When Ordering

The RGA ordering algorithm needed to skip not just sibling spans but their entire subtrees when finding the insertion position. In RGA, spans form a tree structure where:
- No-origin spans are at the root level
- Spans with origin are children of their origin character
- In document order, a span's subtree immediately follows it

When inserting a span, we need to:
1. Find spans with the same origin (siblings in the tree)
2. Order siblings by (user, seq) descending
3. When a sibling has higher precedence, skip it AND its entire subtree

The subtree ends when we encounter:
- Another sibling (same origin as us)
- A no-origin span (root level)

This applies both to:
- No-origin spans (root level ordering)
- Spans with origins (child ordering within a subtree)

### Files Changed

- `src/crdt/rga.rs`:
  - Replaced `OriginRef` with `OriginId`
  - Updated `Span` struct to store origin as (user_idx, seq)
  - Updated `split()` to set the right part's origin correctly
  - Updated `insert_span_at_pos_optimized()` to compute origin as item ID
  - Updated `insert_span_rga()` to compare origins by item ID and skip subtrees
  - Updated `merge()` to translate origins directly from item IDs

### Result

All 137 tests pass, including:
- `merge_commutative`: 100 cases
- `multi_user_convergence`: 50 cases

The merge operation is now commutative: `merge(A, B) == merge(B, A)` for all document states A and B.
