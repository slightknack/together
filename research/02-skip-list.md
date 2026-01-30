---
model = "claude-opus-4-5"
created = "2026-01-30"
modified = "2026-01-30"
driver = "Isaac Clayton"
---

# Skip List for RGA Position Lookups

## Problem

Current RGA uses `Vec<Span>` with O(n) operations:
- `find_visible_pos`: linear scan to find span at position
- `Vec::insert`: shifts all subsequent elements
- `reindex_from`: updates HashMap for all shifted spans

Baseline: 2725x slower than diamond-types on sveltecomponent trace.

## Skip List Overview

A skip list is a probabilistic data structure that provides O(log n) search, insert, and delete. It's a series of linked lists stacked vertically:

```
Level 3: HEAD -----> 30 ---------------------------------> NIL
Level 2: HEAD -----> 30 ---------> 50 ------------------> NIL  
Level 1: HEAD -> 10 -> 30 -> 40 -> 50 -> 60 ------------> NIL
Level 0: HEAD -> 10 -> 20 -> 30 -> 40 -> 50 -> 60 -> 70 -> NIL
```

Each node has a random height. On average, half the nodes have height 1, quarter have height 2, etc. Search starts at the top level and descends.

## Augmented Skip List for Position Lookup

To support O(log n) position lookup, each forward pointer stores the "skip distance" - how many visible characters are skipped by taking that pointer.

```
Node structure:
- span: Span
- levels: Vec<(next_ptr, skip_distance)>
```

To find position P:
1. Start at HEAD, level = max_level
2. If skip_distance at current level <= remaining:
   - Subtract skip_distance from remaining
   - Move forward
3. Else descend one level
4. Repeat until level 0 and position found

Insert/delete: update skip_distances along the path.

## Diamond-Types Approach

Diamond-types uses a "content tree" which is a modified skip list called JumpRope:
- Each node stores up to ~400 bytes of text inline (gap buffer)
- Skip list for O(log n) position lookup
- Nodes are split when they exceed capacity

The key insight: store actual text in the skip list nodes, not just references.

## Implementation Plan for Together

**Phase 1: Simple skip list with spans**
- Replace `Vec<Span>` with skip list
- Each node holds one Span
- Maintain skip_distance (visible chars) at each level
- O(log n) position lookup and insertion

**Phase 2: Batched nodes (optional)**
- Store multiple spans per node
- Better cache locality
- More complex but potentially faster

## Algorithm Details

### Find Position

```
fn find_pos(target: u64) -> (Node, offset) {
    let mut node = head
    let mut pos = 0
    for level in (0..max_level).rev() {
        while node.next[level] exists {
            let skip = node.skip[level]
            if pos + skip <= target {
                pos += skip
                node = node.next[level]
            } else {
                break
            }
        }
    }
    // Now at correct node, find offset within span
    return (node, target - (pos - node.span.visible_len()))
}
```

### Insert After Position

```
fn insert_at(target: u64, span: Span) {
    // Find path and record predecessors at each level
    let mut preds = vec![head; max_level]
    let mut pos = 0
    let mut node = head
    
    for level in (0..max_level).rev() {
        while node.next[level] exists && pos + node.skip[level] < target {
            pos += node.skip[level]
            node = node.next[level]
        }
        preds[level] = node
    }
    
    // Create new node with random height
    let new_node = Node::new(span, random_height())
    let span_len = span.visible_len()
    
    // Insert at each level up to new_node.height
    for level in 0..new_node.height {
        new_node.next[level] = preds[level].next[level]
        preds[level].next[level] = new_node
        // Update skip distances...
    }
    
    // Increment skip distances for levels above new_node.height
    for level in new_node.height..max_level {
        preds[level].skip[level] += span_len
    }
}
```

### Complexity

- Find: O(log n) expected
- Insert: O(log n) expected
- Delete: O(log n) expected
- Space: O(n) expected

## References

- Pugh, William. "Skip Lists: A Probabilistic Alternative to Balanced Trees" (1990)
- Diamond-types JumpRope: https://github.com/josephg/diamond-types/blob/master/crates/jumprope/
