---
model = "claude-opus-4-5"
created = "2026-01-30"
modified = "2026-01-30"
driver = "Isaac Clayton"
---

# Skip List Width Bug Analysis

## The Bug

After 65 inserts (first split), level 0 width sum is 97 instead of 65.
- 97 - 65 = 32 = CHUNK_SIZE / 2
- This means we're double-counting the split amount

## Root Cause

In `split_and_insert`, the width maintenance is wrong.

### Before Split (64 items in one node)
```
head.next[0] -> node (64 items)
head.widths[0] = 64
```

### After Split (my broken code)
```
head.next[0] -> old_node (32 items) -> new_node (32 items)
head.widths[0] = 64 + 1 = 65  (WRONG! Should be 32)
old_node.widths[0] = 32
new_node.widths[0] = 0 (points to NULL)

Sum: 65 + 32 = 97 (matches error!)
```

### What Should Happen
```
head.widths[0] = 32 (items in old_node)
old_node.widths[0] = 32 (items in new_node)
Then +1 for the inserted item

Sum: 32 + 32 + 1 = 65 ✓
```

## The Fix

The predecessor at each level needs its width updated when we split.

Currently I'm:
1. Setting `old_node.widths[level] = split` (correct for old_node -> new_node)
2. NOT updating predecessor's width (BUG)
3. Calling `increment_widths(update)` which adds 1 to predecessor

The predecessor's width was pointing THROUGH old_node to whatever was beyond.
After split, predecessor still points to old_node, but old_node is now smaller.

## Jumprope's Approach

From `/tmp/jumprope/src/jumprope.rs:insert_node_at`:

```rust
for i in 0..new_height {
    let prev_skip = &mut (*cursor.inner[i].node).nexts[i];
    
    // New node's skip = items_in_new + prev_skip - cursor_pos
    nexts[i].skip_chars = num_chars + prev_skip.skip_chars - cursor.inner[i].skip_chars;
    
    // Predecessor's skip = cursor_pos (items before insertion point)
    prev_skip.skip_chars = cursor.inner[i].skip_chars;
}

for i in new_height..head_height {
    // Levels above new node just add the new item count
    (*cursor.inner[i].node).nexts[i].skip_chars += num_chars;
}
```

Key insight: **track cursor position at each level** in the update vector.

My `update` array tracks predecessor indices but not positions!
I need `update_pos` to track how many items precede the insertion point at each level.

## Correct Width Calculation for Split

When splitting node at index `node_idx` with `local_idx` insertion point:

For levels where predecessor points directly to old_node:
- `pred.widths[level]` was: items in old_node + items in old_node.next chain
- After split at position `split`:
  - old_node has `split` items
  - new_node has `CHUNK_SIZE - split` items
  - `pred.widths[level]` should become: (items before split point relative to pred)

Actually, the fundamental issue is that my width semantics are different from jumprope.

### My Semantics (node-based)
- `node.widths[level]` = items in nodes reachable via `node.next[level]`

### Jumprope Semantics (edge-based)  
- `node.skip_chars[level]` = items between start of `node` and start of `node.next[level]`

Edge-based is cleaner because:
- Width of edge A->B = items in A (at that level's granularity)
- On split, just update the affected edges

## Clarified Width Semantics

`node.widths[level]` = number of items skipped by following `node.next[level]`

This means the width equals the number of items IN the node that `next[level]` points to (plus any items in further nodes at lower levels, up to the next node at this level).

Actually simpler: `widths[level]` = items in the "span" covered by this edge.

For level 0: widths[0] = items in the node that next[0] points to.

## The Split Fix

Before split:
- pred.next[0] -> old_node (64 items)
- pred.widths[0] = 64

After split at midpoint (split = 32):
- pred.next[0] -> old_node (32 items) -> new_node (32 items)
- pred.widths[0] should = 32 (just old_node's items)
- old_node.widths[0] = 32 (new_node's items)

Key insight: I need to update pred.widths[level] for levels where pred points directly to old_node.

For levels where pred.next[level] == old_node:
- pred.widths[level] = old_node.len (after split) = split

For levels where pred.next[level] != old_node (pred is farther back):
- pred.widths[level] doesn't change from the split itself
- But we still need to +1 for the inserted item

## Implementation Fix

In split_and_insert, for each level:
1. Check if update[level].next[level] == node_idx (direct predecessor)
2. If yes: update[level].widths[level] = split (the new size of old_node)
3. Wire old_node.next[level] -> new_node with width = CHUNK_SIZE - split
4. For levels above old_node's height where new_node appears, wire through update

After all wiring, increment widths for the new item:
- For levels < new_height: add 1 to update[level].widths[level] 
  (But only if inserting in old_node; if inserting in new_node, add to old_node.widths[level])
- For levels >= new_height: add 1 to update[level].widths[level]

Actually this is getting complicated. Let me think about it differently...

## Jumprope's Algorithm (the correct way)

From `insert_node_at` in jumprope:

```rust
// cursor.inner[i].skip_chars = items between pred and insertion point at level i
// prev_skip.skip_chars = pred's current width at level i

for i in 0..new_height {
    // New node's width = new_items + (pred's old reach) - (items before insertion)
    new_node.skip_chars = num_chars + prev_skip.skip_chars - cursor.skip_chars;
    
    // Pred's new width = items before insertion point
    prev_skip.skip_chars = cursor.skip_chars;
}

for i in new_height..head_height {
    // Just add new items to pred's width (no new node at this level)
    prev_skip.skip_chars += num_chars;
}
```

Key insight: `cursor.inner[i].skip_chars` tracks **items between pred and insertion point**.

In my terms:
- `update_pos[level]` = global position at predecessor
- `index` = global insertion position  
- `index - update_pos[level]` = items between pred and insertion point

## Applying to My Split

For split_and_insert, I need to:

1. Pass `update_pos` to the function
2. For levels where we insert the new_node (0..new_height):
   - Calculate `local_offset = index - update_pos[level]` (items before insertion at this level)
   - pred.widths[level] = local_offset (up to but not including inserted item)
   - Then we need to figure out new_node's width...

Wait, the complication is that I'm splitting an existing node AND inserting. Let me think step by step.

## Split + Insert Algorithm

Starting state:
- pred.next[level] -> old_node (64 items)
- pred.widths[level] = some_value (might be > 64 if pred is far back)

After split (before insert):
- old_node has 32 items (indices 0..32)
- new_node has 32 items (indices 32..64, but now at 0..32 within new_node)

Case 1: Inserting in old_node (local_idx < split)
- pred.next[level] -> old_node (now 33 items after insert)
- old_node.next[level] -> new_node (32 items)
- If pred directly pointed to old_node: pred.widths[level] = 33
- old_node.widths[level] = 32

Case 2: Inserting in new_node (local_idx >= split)  
- pred.next[level] -> old_node (32 items)
- old_node.next[level] -> new_node (now 33 items after insert)
- If pred directly pointed to old_node: pred.widths[level] = 32
- old_node.widths[level] = 33

## The Key Realization

The issue with my current code: I'm setting widths based on the split position, not considering where the insertion happens or using update_pos.

For each level, I need to determine:
1. Does pred point directly to old_node? (check: update[level].next[level] == node_idx)
2. If yes, update pred.widths[level] based on old_node's new size
3. If no, just increment pred.widths[level] by 1 for the new item

## Simplified Fix

Actually, let me simplify. The problem is just the predecessor width calculation.

For levels < old_height (where old_node has a pointer):
- old_node.widths[level] = items in new_node = CHUNK_SIZE - split (before insert)
- If inserting in new_node, old_node.widths[level] += 1

For predecessors at each level:
- If update[level].next[level] == node_idx: pred points directly to old_node
  - pred.widths[level] was: old_node.len (64) + items beyond
  - After split: should be old_node.len (32) + items beyond that old_node no longer covers
  - Actually: pred.widths[level] = split + (old_width - 64) if inserting in new_node
  - Or: pred.widths[level] = split + 1 + (old_width - 64) if inserting in old_node
  
Hmm, this is getting complicated because pred might skip over multiple nodes.

## Even Simpler: Copy Jumprope's Cursor Approach

The cleanest fix is to track `cursor_offset[level]` = items between update[level] and insertion point.

Then on insert:
- pred.widths[level] = cursor_offset[level] + (1 if inserting before this level's granularity)
- new_node.widths[level] = num_items + old_pred_width - cursor_offset[level]

Let me implement this properly.

## Debug Output After Insert 129

```
SkipList len=130
  Node 0 (len=0, height=16): widths[0]=32, next[0]=1   (HEAD)
  Node 1 (len=32, height=1): widths[0]=32, next[0]=2
  Node 2 (len=32, height=1): widths[0]=32, next[0]=3
  Node 3 (len=32, height=1): widths[0]=33, next[0]=4   <- WRONG: should be 34
  Node 4 (len=34, height=2): widths[0]=1, next[0]=NULL
```

Total: 32 + 32 + 32 + 33 + 1 = 130 ✓ (widths sum correctly)
But: Node 3 says "33 items ahead" but Node 4 has 34 items

The issue: When inserting into Node 4, we split something and the width of Node 3 wasn't updated correctly.

Let me trace what happened:
- Insert 128: creates 129 items, Node 3 should have 32, then we insert 129th item somewhere
- Insert 129: creates 130 items

Actually, let me think about the splits:
- Items 0-63: First node (1), then split at 64 -> nodes 1 (32) and 2 (32)
- Items 64-127: Go into nodes 1-2, with splits creating nodes 3, 4, etc.
- Item 128: Goes into... need to trace

The bug is that Node 3's width to Node 4 is 33 but Node 4 has 34 items.

Looking at my split_and_insert: when inserting into the new_node (local_idx >= split),
I set `old_node.widths[level] = new_final_len` but that's wrong!

`old_node.widths[level]` should be the count of items in NEW_NODE, which is `new_final_len`.
But wait, that IS what I wrote.

Oh wait, the issue is different. Look at the structure:
- Node 3 (len=32) points to Node 4 (len=34)
- Node 3.widths[0] = 33, but should be 34

So when we inserted item 129, we inserted into Node 4, making it go from 33 to 34 items.
But we didn't update Node 3's width!

The issue: **increment_widths only adds 1 to the predecessors in `update`**, but after a split,
the predecessor of the NEW item might be the OLD_NODE (Node 3), not the original update path!

When we insert into new_node after a split:
- The immediate predecessor at level 0 is OLD_NODE, not the update[0] predecessor!
- We need to increment OLD_NODE.widths[0], not update[0].widths[0]

## The Fix

In split_and_insert, after inserting:
- If insert_in_old: increment widths of update[] (normal case)
- If insert_in_new: increment widths of old_node at level 0, and update[] at higher levels

Actually, my current code tries to handle this but gets it wrong. Let me re-examine.

## The Real Bug: Update Vector Semantics

After more debugging, found the real issue:

```
Before insert 129:
  Node 0 (h=16): w=[0:32,1:128,...] n=[0:1,1:4,...]  <- Head jumps to Node 4 at level 1!
  Node 1-3: height 1 only
  Node 4 (h=2): w=[0:0,1:0] n=[0:N,1:N]
```

When finding path for target=129:
- Level 1: Head has widths[1]=128, 0+128=128 < 129, jump to Node 4
- Level 0: We're at Node 4, widths[0]=0, next=NULL, stay at Node 4
- update[0] = Node 4

But we're inserting INTO Node 4! The predecessor at level 0 should be Node 3 (who points to Node 4), not Node 4 itself.

**The fundamental issue**: My traversal moves FORWARD and then records. But the update vector should record the node BEFORE we move forward - the node whose `next` pointer will be modified.

**The fix**: After traversal, for each level where we ended up at the target node, we need to find the actual predecessor by walking the level 0 chain.

Actually, easier fix: Change the traversal condition from `<` to `<=` for some cases, or track differently.

Looking at jumprope's cursor approach:
- `cursor.inner[i].skip_chars` = distance from predecessor to insertion point
- This implicitly means the predecessor is the node we stopped at BEFORE jumping

My find_path is wrong. It should stop BEFORE moving to the next node:
```rust
if pos + width < target {  // Should maybe be <= for insertion?
```

Actually, for insertion at position 129 into a list of 129 items, we want to append. The predecessor should be the node containing position 128 (0-indexed), which is Node 4 (it has items 96-128). But we're inserting at position 129, which is AFTER all items in Node 4.

Wait, Node 4 has 33 items (indices 96-128 in the global list). Inserting at index 129 means appending after all of them. So the insertion goes INTO Node 4 at local index 33.

The predecessor for width updates should be whoever points TO Node 4, which is Node 3.

The issue is that my update vector records where we ARE, not where we came FROM. For level 0, I need to track the node that points to the insertion target.

**Simpler fix**: After find_path, if update[0] == target_node, walk backwards to find the real predecessor. Or change find_path to not move to the final node.

## Fundamental Question: Why Are Higher Level Widths Inconsistent?

After 200 inserts, level 0 sums correctly (200) but level 1 sums to 104.

```
Node 0 (HEAD, h=16): w[0]=32, w[1]=33, next[0]=1, next[1]=4
Node 1 (h=1): w[0]=32, next[0]=2
Node 2 (h=1): w[0]=32, next[0]=3  
Node 3 (h=1): w[0]=33, next[0]=4
Node 4 (h=2): w[0]=32, w[1]=32, next[0]=5, next[1]=5
Node 5 (h=2): w[0]=39, w[1]=39, next[0]=6, next[1]=6
Node 6 (h=2): w[0]=0, w[1]=0, next[0]=NULL, next[1]=NULL
```

Level 0 chain: 0 -> 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> NULL
Level 0 widths: 32 + 32 + 32 + 33 + 32 + 39 + 0 = 200 ✓

Level 1 chain: 0 -> 4 -> 5 -> 6 -> NULL
Level 1 widths: 33 + 32 + 39 + 0 = 104 ✗ (should be 200)

**The Problem**: Level 1 widths should cover ALL items, not just items in tall nodes.

- Head.widths[1] should = items from head to Node 4 = 32 + 32 + 32 + 33 = 129 (nodes 1,2,3,4's items)
- But Head.widths[1] = 33

**Root Cause**: When inserting items into low-height nodes (1, 2, 3), I'm only updating 
widths at level 0. But the HEAD's width at level 1 should ALSO increase because those 
items are "under" the level 1 skip.

**The Skip List Width Invariant**:
For any node N at level L, N.widths[L] = total items between N and N.next[L].

This means:
- Head.widths[1] = items in nodes 1, 2, 3, 4 (everything before Node 5)
- When we insert into Node 1, Head.widths[1] should increase by 1

**My Bug**: I only increment widths for `update[level]`, but if `update[level]` is a 
low-height node, the higher-level predecessors (like HEAD) don't get updated.

**The Fix**: For each level, find the predecessor that COVERS the insertion point at 
that level (not the predecessor at level 0). This predecessor's width should increase.

For inserting into Node 3:
- Level 0 predecessor: Node 2 (Node 2.next[0] skips to Node 3)
- Level 1 predecessor: HEAD (Head.next[1] skips to Node 4, which is AFTER Node 3)

So when inserting into Node 3:
- Increment Node 2.widths[0]
- Increment Head.widths[1] (because the item is "under" Head's level 1 skip)
- Increment Head.widths[2..16] (same reason)

This is exactly what `find_path` should compute - but my find_path gives the same 
predecessor for all levels when we reach a low-height node!

## The Correct find_path Semantics

`update[level]` should be: the node whose `next[level]` pointer's span CONTAINS the insertion point.

For inserting into Node 3 (a height-1 node):
- Level 0: The node whose next[0] points to Node 3 → that's Node 2
- Level 1: The node whose next[1] span contains Node 3 → that's HEAD (Head.next[1] skips over nodes 1,2,3 to reach Node 4)

My current find_path descends levels and records where it stopped. But when it hits a low-height node, it records THAT node at all remaining levels, which is wrong.

**The fix for find_path**: Don't update `update[level]` when the current node doesn't have that level. Keep the predecessor from when we were still at a tall-enough node.

Current (BROKEN):
```rust
for level in (0..MAX_HEIGHT).rev() {
    // ... traverse at this level ...
    update[level] = idx;  // WRONG: idx might not have this level
}
```

Fixed:
```rust
for level in (0..MAX_HEIGHT).rev() {
    loop {
        let node = self.node(idx);
        if level >= node.height() {
            break;  // Current node doesn't have this level
        }
        // ... traverse ...
    }
    // Only record if this node actually has this level
    if level < self.node(idx).height() {
        update[level] = idx;
    }
    // Otherwise, update[level] keeps its value from initialization or a higher level
}
```

Wait, but we initialize update to [self.head; MAX_HEIGHT]. So if we never update 
update[level], it stays as HEAD. That's correct - HEAD is the predecessor at levels
where no other node is tall enough!

Let me trace through again with the fix:
- update = [HEAD; 16]
- Level 15..2: We're at HEAD, HEAD has height 16. We try to traverse but next[level]=NULL.
  Break, update[level] = HEAD ✓
- Level 1: At HEAD, traverse if possible. If HEAD.next[1]=Node4 and we can move, we move.
  Then we're at Node4. If Node4.next[1] is past target, break. update[1] = Node4.
  
Hmm, that's still not right. The issue is that update[1] should be the node whose 
level-1 span CONTAINS the target, which might be HEAD even after we've moved forward.

Actually wait - if we moved to Node4 via level 1, then Node4 IS the predecessor at 
level 1 for positions within Node4. But if we're inserting into Node 3 (before Node4),
we shouldn't have moved to Node4 at level 1!

Let me re-examine: inserting at position 90 (into Node 3 which covers ~64-96):
- Level 1 at HEAD: Head.widths[1] = ??? (suppose it's 129, covering nodes 1-4)
  - 0 + 129 < 90? No! So we DON'T move to Node 4.
  - update[1] = HEAD ✓

Actually the traversal logic IS correct. The issue must be in how widths[1] got wrong
in the first place. Let me check insert_first and the early inserts.

## The Split Bug: Items in Wrong Nodes

After split of 64 items:
- Node 1: 32 items (values 0-31)
- Node 2: 33 items (values 32-64)

Widths:
- Head.widths[0] = 32, next[0] = Node 1
- Node 1.widths[0] = 33, next[0] = Node 2

Call `get(32)`:
1. find_node starts at Head, pos=0
2. Head.widths[0]=32, next=Node 1. pos + width = 32 <= 32, so move to Node 1, pos=32
3. At Node 1: local = 32 - 32 = 0. Is 0 < 32? Yes. Return (Node 1, 0)
4. Node 1.items[0] = 0 ✗

**The problem**: Width semantics!

If Head.widths[0] = 32 and we move to Node 1, what does pos=32 mean?
- Option A: pos is the index of the FIRST item in Node 1 → Node 1 has items at indices 32-63
- Option B: pos is the number of items BEFORE Node 1 → Node 1 has items at indices 0-31

I've been using Option B (counting items skipped), but the items are arranged as Option A!

After split, Node 1 should have items 0-31 (first half). To access item 32, we need Node 2.

When calling get(32):
- We traverse to Node 1 with pos=32 (we've skipped 32 items)
- local = target - pos = 0
- But Node 1.items[0] is value 0, not value 32!

**The real bug**: The split is NOT rearranging items correctly. After split:
- Node 1 should have values 0-31 in items[0..32]
- Node 2 should have values 32-64 in items[0..33]

Let me check what actually happens in split_and_insert...

## Correct Width Semantics (FINALLY)

From jumprope: "The number of characters between the start of the current node and the start of the next node."

For a node with N items: width = N (items in the node = distance to next node)

**For HEAD (sentinel with no items)**: width = 0, because there are 0 items between 
"start of head" and "start of first real node".

Traversal then works:
- Start at HEAD, pos = 0
- Follow Head.next[0] (width=0), arrive at Node1, pos = 0 + 0 = 0 ✓
- Node1 has 32 items at positions 0-31
- Follow Node1.next[0] (width=32), arrive at Node2, pos = 0 + 32 = 32 ✓
- Node2 has 33 items at positions 32-64

**My bug**: I was setting Head.widths = items in first node, but it should be 0 (items in HEAD).

**The fix**:
1. `insert_first`: Head.widths = 0 (not 1), Node.widths = 1
2. `increment_widths`: Add 1 to the node that CONTAINS the insertion, not its predecessor
3. `split_and_insert`: Set old_node.widths = items in old_node (after split)

Wait, but then how do we track total length via widths? Let me reconsider...

Actually, the sum of all widths at any level should equal total items. If Head.width=0:
- Sum = Head.width + Node1.width + Node2.width + ... = 0 + 32 + 33 = 65 ✓

But for levels where Head.next[level] = NULL, Head.width should = total items (since 
Head "spans" the whole list at that level).

OK so the rule is:
- `node.widths[level]` = items in the span from node to node.next[level]
- If node.next[level] = NULL, widths[level] = items from node to end of list

For HEAD (no items):
- Head.widths[level] when Head.next[level] = Node = items from Head to Node = 0
- Head.widths[level] when Head.next[level] = NULL = total items in list

This is consistent! Let me implement it.

---

# Resolution and Lessons Learned

## What Finally Fixed It

The fix had **three distinct components**:

### 1. find_path semantics
Only record `update[level] = idx` when the current node actually HAS that level.
If a node doesn't have a level, keep the predecessor from the previous (higher) level.

```rust
for level in (0..MAX_HEIGHT).rev() {
    if level < self.node(idx).height() {
        // ... traverse at this level ...
        update[level] = idx;
        remaining_at[level] = remaining;
    }
    // If level >= node.height(), keep previous value (defaults to HEAD)
}
```

### 2. increment/decrement_widths_for_insert/remove
For the TARGET node, modify its widths at levels it participates in.
For the PREDECESSOR at levels ABOVE the target's height, modify those widths.

```rust
// Increment target node's widths at all its levels
for level in 0..target_height {
    target.widths[level] += 1;
}

// Increment predecessor's widths at levels above target's height
for level in target_height..MAX_HEIGHT {
    pred.widths[level] += 1;
}
```

### 3. split_and_insert: Correct width calculation when splicing tall nodes
The **critical bug**: When a new tall node is spliced into higher levels, the predecessor's 
width must be recalculated, not left unchanged.

```rust
// For levels where new_node is taller than old_node
for level in old_height..new_height {
    // Calculate predecessor's new width using remaining_at
    let pred_new_width = remaining_at[level] - local_idx + split;
    let new_node_width = pred_old_width - pred_new_width + 1;
    
    self.node_mut(pred_idx).widths[level] = pred_new_width as u32;
    self.node_mut(new_idx).widths[level] = new_node_width as u32;
}
```

## What Worked

1. **Debug output**: The `debug_print()` method showing node structure was invaluable
2. **Invariant checking**: Testing "remove returns correct value" caught the real bug faster
3. **Tracing specific operations**: Adding debug output to specific inserts (64, 128) showed exactly when things broke
4. **Comparing to jumprope**: Reading the reference implementation clarified semantics

## What Didn't Work / Time Wasters

1. **Unclear mental model**: I flip-flopped between "width = items in next node" vs "width = items in span". Should have written out the exact invariant first.
2. **Not testing invariants early**: Could have caught the `find_path` bug much earlier with a simple "sum of widths at each level = total items" check
3. **Debugging symptoms instead of causes**: Spent time fixing `decrement_widths` when the real bug was in `split_and_insert` during INSERTS
4. **Not using remaining_at**: The `remaining_at` parameter was passed but unused - a clear sign something was missing

## Delta Code Observation

The code WAS delta code in the sense that each fix was incremental. But the problem was that I was making deltas to FIX SYMPTOMS without understanding the underlying model.

Better approach: **Define the invariants first, write tests for them, THEN implement**

The invariants I should have stated upfront:
1. `widths[level]` = items in span from this node to `next[level]`
2. Sum of widths at each level = total list length
3. `update[level]` = the node whose span at `level` CONTAINS the target position
4. After any operation, all invariants hold

## What I'd Do Differently

1. **Write a reference implementation first** - even a simple O(n) list that computes the "correct" answer
2. **Test invariants after EVERY operation**, not just at the end
3. **Use property-based testing** with quickcheck to find edge cases
4. **Draw pictures** - the skip list structure is visual, should have diagrammed more
5. **Start with `remaining_at` from the beginning** - jumprope's cursor approach is cleaner than my ad-hoc position tracking
