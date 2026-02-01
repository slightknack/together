+++
model = "claude-opus-4-5"
created = 2026-01-31
modified = 2026-01-31
driver = "Isaac Clayton"
+++

# Postmortem: Fixing RGA Merge Commutativity

## The Bug

Property-based fuzz testing revealed that merging two independently-edited RGA documents produced different results depending on merge order. `merge(A, B)` did not equal `merge(B, A)`.

The minimal failing case was deceptively simple:

```
User1: Insert "a" at pos 0
User2: Insert "aa" at pos 0, then insert "aa" at pos 1, then insert "b" at pos 2

merge(user1, user2) != merge(user2, user1)
```

The 'a' from user1 ended up in different positions depending on which direction we merged.

## The Investigation

### First Hypothesis: Span Index Origins

The original implementation stored origins as `OriginRef { span_idx: u32, offset: u32 }`. This meant an origin referred to "span number 3, character 2 within that span." The problem was immediately suspicious: span indices are document-structure-dependent. If two documents have different span structures (due to different split histories), the same span_idx could refer to different logical characters.

But fixing this alone was not enough. I changed the origin representation to `OriginId { user_idx: u16, seq: u32 }`, storing the immutable identity of the character (who inserted it and when). This is what Yjs does:

"When an item is created, it stores a reference to the IDs of the preceding and succeeding item. These are stored in the item's `origin` and `originRight` fields, respectively." [1]

The tests still failed.

### Second Hypothesis: Split Semantics

When a span is split, what origin should the right part have? My initial fix gave the right part an origin pointing to the last character of the left part. This seemed correct: within a span "abc", character 'c' (seq 2) was typed after 'b' (seq 1), which was typed after 'a' (seq 0).

I briefly tried the opposite: make both parts inherit the original origin. This broke things worse. The right part of a split genuinely has a causal dependency on the left part.

### Third Hypothesis: The Tree Walk

The breakthrough came when I traced through the merge step by step with a concrete example. RGA forms a tree structure, as Fugue describes:

"Each node in the tree, except for the root, has a _parent_ and a _side_ (left or right child)." [2]

"The sort order on nodes is given by the tree walk (in-order traversal): first traverse a node's left children, then visit the node, then traverse its right children." [2]

In this tree model:
- No-origin items are at the root level
- Items with origin X are children of X
- Siblings (items with the same origin) are ordered by (user, seq) descending

The document order is a traversal of this tree. Here was the bug: when inserting an item, if a sibling had higher precedence, the code skipped past that sibling but not its subtree. The item would be inserted in the middle of its sibling's descendants instead of after them.

YATA makes this structure explicit:

"Based on the left and right origins of the block, we can establish a boundaries between which it's safe to insert a new block." [3]

The key insight: the "boundary" is not just the sibling, but the sibling's entire subtree.

## The Fix

Two changes were required:

### 1. Store Origins as Item IDs

```rust
// Before: span-structure-dependent
struct OriginRef {
    span_idx: u32,
    offset: u32,
}

// After: immutable item identity
struct OriginId {
    user_idx: u16,
    seq: u32,
}
```

This ensures origins refer to the same logical character across all documents, regardless of how spans have been split.

### 2. Skip Subtrees During Ordering

When finding where to insert an item, skip past higher-precedence siblings AND their entire subtrees:

```rust
// When a sibling has higher precedence, skip its subtree
while pos < self.spans.len() {
    let next = self.spans.get(pos).unwrap();
    if !next.has_origin() {
        break;  // Subtree ends at next root-level item
    }
    let next_origin = next.origin();
    if next_origin == our_origin {
        break;  // Subtree ends at next sibling
    }
    pos += 1;  // Skip this descendant
}
```

The subtree ends when we encounter:
- A no-origin span (another root-level item)
- A span with the same origin as ours (another sibling)

## Why Property-Based Testing Found This

Unit tests exercise expected paths. Property-based testing exercises unexpected combinations. The bug required:
- Multiple users inserting at position 0 (no origin)
- One user splitting their span by inserting in the middle
- The split creating a span with origin that happens to interleave with the other user's no-origin span during merge

This is a narrow corner case that emerges naturally from property-based exploration but would be unlikely to appear in hand-written tests.

As diamond-types notes about its time DAG: "The first change has a special parent of 'ROOT'." [4] Our bug was specifically about how ROOT's children (no-origin spans) interacted with their descendants during merge.

## Lessons

1. **Span indices are not stable identifiers.** Anything that references document structure is suspect. Use item IDs.

2. **Trees have subtrees.** When ordering siblings, you must skip entire subtrees, not just the sibling itself.

3. **Property-based testing finds bugs that humans miss.** The failing case required a specific sequence of operations that creates a particular tree shape.

4. **Read the papers.** Yjs's INTERNALS.md and the YATA paper describe exactly this tree structure and ordering. The answer was in the literature.

## References

[1] Yjs INTERNALS.md: https://github.com/yjs/yjs/blob/main/INTERNALS.md
[2] Matthew Weidner, "Fugue: A Basic List CRDT": https://mattweidner.com/2022/10/21/basic-list-crdt.html
[3] Bartosz Sypytkowski, "Delta-state CRDTs: indexed sequences with YATA": https://www.bartoszsypytkowski.com/yata/
[4] Diamond-types INTERNALS.md: https://github.com/josephg/diamond-types/blob/master/INTERNALS.md
