+++
title = "Deep Dive: Sequence CRDT Implementations"
date = 2026-01-31
+++

# Deep Dive: Sequence CRDT Implementations

This document captures research into how major CRDT libraries implement
sequence/list CRDTs for collaborative text editing.

## Goal

Understand from first principles what the correct approach is for implementing
a sequence CRDT that:
1. Guarantees merge commutativity (merge(A,B) == merge(B,A))
2. Handles concurrent edits correctly
3. Is efficient for real-world editing patterns

---

## 1. Yjs / YATA

### Source
- https://github.com/yjs/yjs
- Paper: "YATA: Yet Another Transformation Algorithm"

### Key Insights

**Algorithm: YATA with TWO origins**

Yjs stores both `origin` (left) and `rightOrigin` (right) for every insertion:
- `origin`: The item that was immediately to the LEFT when this item was inserted
- `rightOrigin`: The item that was immediately to the RIGHT when this item was inserted

**Why two origins matter:**
The dual-origin approach solves the "subtree boundary" problem that single-origin
RGA implementations struggle with. When you have only the left origin, you cannot
reliably determine when you've exited a "subtree" (descendants of a sibling).

**Ordering algorithm:**
When inserting at the same position concurrently:
1. Compare by `leftOrigin` ID
2. If equal, compare by `rightOrigin` ID  
3. If still equal, use client ID as tiebreaker

**Key quote from Yjs implementation:**
```javascript
// We have an origin! Let's search for the right position.
// Case 1: The character was deleted. In this case, the origin
// was probably part of a concurrent change, so we don't need to
// do anything special.
// Case 2: origin is not deleted. We need to find the right position.
```

---

## 2. diamond-types

### Source
- https://github.com/josephg/diamond-types
- INTERNALS.md in repo

### Key Insights

**Algorithm: Modified Yjs (YjsMod) = FugueMax**

Diamond-types uses a modified version of Yjs's algorithm. Seph Gentle confirms:

> "YjsMod / FugueMax items generate identical merge behaviour"

This is significant: FugueMax (the "maximal non-interleaving" variant of Fugue)
produces the same results as the modified Yjs algorithm.

**Performance:**
Diamond-types achieves ~0.056s for the automerge-perf editing trace, 
compared to ~45s for Automerge. This 800x speedup comes from:
- Rust implementation with B-tree storage
- Run-length encoding (grouping consecutive chars)
- Skip list optimization for position lookup

**Key insight:**
The algorithm choice (YATA vs Fugue vs RGA) matters less than implementation
details for performance. All three can achieve comparable performance with
proper optimization.

---

## 3. Automerge

### Source
- https://github.com/automerge/automerge
- https://automerge.org/docs/

### Key Insights

**Algorithm: RGA (Replicated Growable Array)**

Automerge uses the original RGA algorithm with:
- Single origin (right origin - "inserting after" the parent element)
- OpID as tiebreaker: `(Lamport counter, actor ID lexicographically)`

**Tree model:**
RGA forms a conceptual tree where each item has a parent pointer. Document
order is computed via depth-first traversal. However, Automerge doesn't
maintain an actual tree in memory.

**Key quote from documentation:**
> "Using the object before the insertion point ensures that two runs 
> concurrently inserted in the same spot cannot become interleaved, 
> guaranteeing convergence to either A x y z 1 2 3 B or A 1 2 3 x y z B 
> but never an interleaving sequence such as A x 1 y 2 z 3 B."

**Physical implementation:**
B-trees with metadata tracking visible elements per subtree. This enables
efficient position lookup and subtree skipping.

**Important limitation:**
With single-origin RGA, concurrent inserts can still interleave in
pathological cases. The dual-origin approach (YATA/Fugue) provides
stronger non-interleaving guarantees.

---

## 4. json-joy

### Source
- https://github.com/nickvision/json-joy

### Key Insights

**Algorithm: RGA with splay trees**

json-joy uses a dual-tree structure:
- **Spatial tree**: Maintains document order for rendering
- **Temporal tree**: Tracks insertion history for CRDT semantics

**Tie-breaking:**
Session ID comparison for concurrent inserts at the same position.
Uses splay trees for adaptive performance - frequently accessed
nodes bubble up to the root.

**Split links:**
Maintains split relationships when spans are divided, enabling
efficient run-length encoding with proper CRDT semantics.

---

## 5. Fugue

### Source
- Paper: "The Art of the Fugue: Minimizing Interleaving in Collaborative Text Editing"
- https://mattweidner.com/2022/10/21/fugue.html

### Key Insights

**The Core Problem: Interleaving**

When two users concurrently insert text at the same position:
- User A inserts "aaaaa" at position 0
- User B inserts "bbbbb" at position 0 concurrently
- Bad result: "ababababa" (interleaved)
- Good result: "aaaaabbbbb" or "bbbbbaaaaa" (non-interleaved)

Fugue is the first algorithm to formally guarantee "maximal non-interleaving".

**Tree Structure:**

Each node has:
- Unique identifier (UID) = `(replicaID, counter)` (called a "causal dot")
- Parent pointer
- Side indicator (LEFT or RIGHT child)

Document order = in-order depth-first traversal of the tree.

**Two origins (like YATA):**

Each insertion stores:
- `leftOrigin`: The UID of the element immediately to the left when inserted
- `rightOrigin`: The UID of the element immediately to the right when inserted

**Ordering algorithm:**
```
comparison_tuple = (leftOrigin_id, rightOrigin_id, element_uid)
```
All replicas independently sort by this same tuple, producing identical results.
This is the mathematical basis of commutativity.

**Fugue vs FugueMax:**

- **Fugue**: Simpler, may interleave slightly more in edge cases
- **FugueMax**: One key difference in right-sibling traversal order,
  guarantees maximal non-interleaving (same behavior as Yjs)

**Key insight:**
FugueMax = YjsMod (as confirmed by diamond-types). The two independent
approaches converged on the same semantics.

---

## Synthesis

### The Core Problem in Our Implementation

Our current bug: `merge(A,B) != merge(B,A)` in certain cases involving
concurrent insertions with different user orderings.

### Root Cause Analysis

**Our implementation uses SINGLE LEFT ORIGIN only.**

Looking at `rga.rs:insert_span_rga()`:
```rust
// We only store:
span.set_origin(OriginId::some(origin_user_idx, origin_id.seq as u32));

// And use this "ultra-conservative" algorithm for subtree detection:
while pos < self.spans.len() {
    let other = self.spans.get(pos).unwrap();
    // ... complex logic trying to detect subtree boundaries ...
}
```

The problem: **With only left origin, we cannot reliably detect subtree boundaries.**

When we insert a span and need to find its position among siblings, we must
skip past any sibling with higher precedence AND that sibling's entire subtree.
But detecting where a subtree ends is fundamentally impossible with only left
origins, because we can't distinguish:
- A descendant of our sibling (should skip)
- A descendant of a different branch (should not skip)

### Why Two Origins Work

With BOTH left and right origin:

1. When inserting between L and R, we record `leftOrigin=L, rightOrigin=R`
2. When comparing concurrent inserts, we compare tuples: `(leftOrigin, rightOrigin, uid)`
3. The right origin acts as a "boundary marker" - we know we've exited a subtree
   when we see an item whose right origin doesn't match our left origin

This is why Yjs, diamond-types, and Fugue all use dual origins.

### Recommended Approach

**Option 1: Add Right Origin (Recommended)**

Modify our implementation to store both left and right origins:
- Add `right_origin_user_idx` and `right_origin_seq` to `Span`
- Update `insert_span_rga` to use the dual-origin ordering algorithm
- This matches Yjs/YATA/FugueMax semantics

Benefits:
- Proven correct by Fugue paper's formal analysis
- Matches production implementations (Yjs, diamond-types)
- Eliminates the "subtree boundary detection" problem entirely

Cost:
- 6 more bytes per span (2 for user_idx, 4 for seq)
- Slightly more complex insertion logic

**Option 2: Full Tree Structure**

Maintain an actual tree in memory (like Fugue reference implementation):
- Each node knows its children
- Subtree boundaries are explicit in the structure

Benefits:
- Conceptually cleaner
- Easy to traverse and reason about

Cost:
- Higher memory overhead
- More complex implementation
- Slower for large documents (tree traversal vs flat array)

### Final Recommendation

**Add right origin to achieve Yjs/FugueMax semantics.**

The key insight from this research is that the dual-origin approach is not
just an optimization - it's fundamentally necessary for correct merge
commutativity without complex subtree tracking.

Our current implementation tries to infer subtree boundaries from left origins
alone, which is mathematically impossible in the general case. The "ultra-conservative"
algorithm we have may work for many cases but will always have edge cases where
it fails.

The fix is straightforward:
1. Store right origin alongside left origin
2. Use the proven YATA/FugueMax ordering algorithm
3. Remove the complex subtree-detection heuristics

This matches what all production CRDTs do (Yjs, diamond-types, Automerge's
recent updates, Fugue).

---

## Implementation Plan

### Changes to Span structure:

```rust
struct Span {
    // ... existing fields ...
    
    // Left origin (character we inserted after)
    origin_user_idx: u16,
    origin_seq: u32,
    
    // Right origin (character that was to our right when we inserted)
    right_origin_user_idx: u16,  // NEW
    right_origin_seq: u32,       // NEW
}
```

### Changes to insert_span_rga:

Replace the current "ultra-conservative" algorithm with the proven YATA/FugueMax
algorithm:

```rust
fn insert_span_rga(&mut self, mut span: Span, origin: Option<ItemId>, right_origin: Option<ItemId>) {
    // ... set both origins on span ...
    
    // Find position: start after left origin
    let mut pos = left_origin_idx + 1;
    
    // YATA/FugueMax ordering: 
    // Skip items where (leftOrigin, rightOrigin, uid) > our tuple
    while pos < self.spans.len() {
        let other = self.spans.get(pos).unwrap();
        
        // Compare by left origin first
        // Then by right origin
        // Then by uid
        
        if should_insert_before(span, other) {
            break;
        }
        pos += 1;
    }
    
    self.spans.insert(pos, span, span_len);
}
```

The exact comparison logic follows the YATA paper / Fugue paper.

### Testing

After implementing:
1. All existing tests should pass
2. The proptest fuzz tests should find no more commutativity violations
3. Consider adding the diamond-types compatibility tests

---

## Sources

- Yjs: https://github.com/yjs/yjs
- YATA paper: "YATA: Yet Another Transformation Algorithm"
- diamond-types: https://github.com/josephg/diamond-types
- Automerge: https://github.com/automerge/automerge
- json-joy: https://github.com/nickvision/json-joy  
- Fugue paper: "The Art of the Fugue: Minimizing Interleaving in Collaborative Text Editing" (arXiv:2305.00583)
- Matthew Weidner's blog: https://mattweidner.com/2022/10/21/fugue.html
- Joseph Gentle's blog: https://josephg.com/blog/crdts-go-brrr/
