// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-01-30"
// driver = "Isaac Clayton"

//! Unrolled Indexable Skip List
//!
//! A random-access list with O(log n) insert/delete at arbitrary indices.
//! Uses arena allocation and chunking for cache locality.
//!
//! # Usage
//!
//! ```
//! use together::crdt::skip_list::SkipList;
//!
//! let mut list = SkipList::new();
//!
//! // Insert at arbitrary positions
//! list.insert(0, "hello");
//! list.insert(1, "world");
//! list.insert(1, "beautiful");  // Insert in middle
//!
//! // Random access by index
//! assert_eq!(list.get(0), Some(&"hello"));
//! assert_eq!(list.get(1), Some(&"beautiful"));
//! assert_eq!(list.get(2), Some(&"world"));
//!
//! // Remove by index
//! let removed = list.remove(1);
//! assert_eq!(removed, "beautiful");
//! assert_eq!(list.len(), 2);
//!
//! // Iterate
//! let items: Vec<_> = list.iter().collect();
//! assert_eq!(items, vec![&"hello", &"world"]);
//! ```
//!
//! # Structure
//!
//! A skip list is a probabilistic data structure built from multiple layers of
//! linked lists. Each node has a random height, and nodes at height h appear in
//! all levels 0..h. Higher levels skip over more nodes, enabling O(log n) search.
//!
//! This implementation is "unrolled": each node stores up to `CHUNK_SIZE` items
//! (64 by default) rather than one item per node. This improves cache locality
//! and reduces pointer overhead.
//!
//! ```text
//! Level 2: HEAD ---------> [A,B] ---------------------------> NULL
//! Level 1: HEAD ---------> [A,B] --------> [E,F] -----------> NULL
//! Level 0: HEAD -> [C,D] -> [A,B] -> [G,H] -> [E,F] -> [I,J] -> NULL
//! ```
//!
//! # Width Semantics
//!
//! Each forward pointer has an associated "width" that tracks how many items
//! are skipped by following that pointer. This enables O(log n) position lookup.
//!
//! The width semantics are edge-based:
//!
//! - `node.widths[level]` = items in the span from this node to `node.next[level]`
//! - For the HEAD node (which has no items), `widths[level]` = 0 when pointing
//!   to a real node, or = total items when pointing to NULL
//! - For data nodes, `widths[0]` = number of items in this node
//!
//! # Invariants
//!
//! These invariants must hold after every operation:
//!
//! 1. **Length consistency**: `iter().count() == len()`
//!
//! 2. **Width sum**: Sum of `widths[level]` along any fully traversable level
//!    equals `len()`. A level is fully traversable if we can follow next pointers
//!    from HEAD to NULL without hitting a node that lacks that level.
//!
//! 3. **Width-length agreement**: For non-head nodes, `widths[0] == node.len()`.
//!    This is because widths[0] represents items in this node.
//!
//! 4. **Level reachability**: All nodes reachable at any level are also reachable
//!    at level 0. (Higher levels are "shortcuts", not separate lists.)
//!
//! 5. **Traversal correctness**: `get(i) == iter().nth(i)` for all valid i.
//!
//! 6. **Position consistency**: The cumulative width sum at any level, when we
//!    arrive at a node, equals the node's position in the level-0 ordering.
//!
//! # Update Vector
//!
//! Mutations (insert, remove) use an "update vector" to track the path through
//! the skip list. `update[level]` is the node whose `next[level]` span contains
//! the target position. This enables O(log n) width updates after modification.
//!
//! Critical: `update[level]` is only set when the current node has that level.
//! For levels above a node's height, `update[level]` retains the value from a
//! higher level (defaulting to HEAD). This ensures width updates propagate to
//! all levels that span the modification point.
//!
//! # References
//!
//! - Pugh, William. "Skip Lists: A Probabilistic Alternative to Balanced Trees" (1990)
//! - diamond-types JumpRope: https://github.com/josephg/diamond-types

use std::mem::MaybeUninit;

/// Items per node. Power of two for efficient division.
const CHUNK_SIZE: usize = 64;

/// Maximum skip list height. 16 levels covers billions of elements.
const MAX_HEIGHT: usize = 16;

/// Node index type. u32 saves space vs usize on 64-bit.
type Idx = u32;

/// Null index marker.
const NULL: Idx = Idx::MAX;

/// A node in the skip list.
struct Node<T> {
    items: [MaybeUninit<T>; CHUNK_SIZE],
    len: u16,
    height: u8,
    next: [Idx; MAX_HEIGHT],
    widths: [u32; MAX_HEIGHT],
}

impl<T> Node<T> {
    fn new(height: u8) -> Self {
        Node {
            items: unsafe { MaybeUninit::uninit().assume_init() },
            len: 0,
            height,
            next: [NULL; MAX_HEIGHT],
            widths: [0; MAX_HEIGHT],
        }
    }

    fn len(&self) -> usize {
        self.len as usize
    }

    fn height(&self) -> usize {
        self.height as usize
    }

    fn has_room(&self) -> bool {
        self.len() < CHUNK_SIZE
    }
}

/// An unrolled indexable skip list.
pub struct SkipList<T> {
    nodes: Vec<Node<T>>,
    head: Idx,
    len: usize,
    free_list: Vec<Idx>,
    rand_state: u64,
}

impl<T> SkipList<T> {
    pub fn new() -> Self {
        let mut list = SkipList {
            nodes: Vec::new(),
            head: 0,
            len: 0,
            free_list: Vec::new(),
            rand_state: 0x12345678_9abcdef0,
        };
        list.head = list.alloc_node(MAX_HEIGHT as u8);
        list
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    // --- Node access helpers ---

    fn node(&self, idx: Idx) -> &Node<T> {
        &self.nodes[idx as usize]
    }

    /// Check structural invariants. Panics if any invariant is violated.
    /// Only runs in debug builds (compiled out in release).
    /// 
    /// Called before and after every mutating operation (insert, remove).
    /// 
    /// RATCHET: If this ever fails, a minimal reproducible test case MUST be
    /// added to the test suite before fixing the bug. See research/03-skip-list-debug.md
    /// for debugging methodology.
    /// 
    /// Invariants checked:
    /// 1. Length consistency: iter().count() == len()
    /// 2. Width sum at level 0 == len()
    /// 3. For non-head nodes: widths[0] == node.len()
    #[cfg(debug_assertions)]
    fn check_invariants(&self) {
        // Invariant 1: iter count matches len
        let iter_count = self.iter().count();
        assert_eq!(iter_count, self.len, "INVARIANT VIOLATED: iter().count() != len()");
        
        // Invariant 2: width sum at level 0 == len
        let mut width_sum = 0usize;
        let mut idx = self.head;
        while idx != NULL {
            let node = self.node(idx);
            if node.height() > 0 {
                width_sum += node.widths[0] as usize;
                idx = node.next[0];
            } else {
                break;
            }
        }
        assert_eq!(width_sum, self.len, "INVARIANT VIOLATED: width sum at level 0 != len()");
        
        // Invariant 3: for non-head nodes, widths[0] == node.len()
        let mut idx = self.node(self.head).next[0];
        while idx != NULL {
            let node = self.node(idx);
            assert_eq!(
                node.widths[0] as usize, 
                node.len(), 
                "INVARIANT VIOLATED: node {} widths[0]={} != len()={}", 
                idx, node.widths[0], node.len()
            );
            idx = node.next[0];
        }
    }
    
    #[cfg(not(debug_assertions))]
    #[inline(always)]
    fn check_invariants(&self) {}

    #[cfg(test)]
    fn debug_print(&self) {
        eprintln!("SkipList len={}", self.len);
        let mut idx = self.head;
        while idx != NULL {
            let node = self.node(idx);
            let widths_str: String = (0..node.height())
                .map(|l| format!("{}:{}", l, node.widths[l]))
                .collect::<Vec<_>>()
                .join(",");
            let nexts_str: String = (0..node.height())
                .map(|l| {
                    if node.next[l] == NULL { format!("{}:N", l) } 
                    else { format!("{}:{}", l, node.next[l]) }
                })
                .collect::<Vec<_>>()
                .join(",");
            eprintln!(
                "  Node {} (len={}, h={}): w=[{}] n=[{}]",
                idx, node.len(), node.height(), widths_str, nexts_str
            );
            idx = node.next[0];
        }
    }

    fn node_mut(&mut self, idx: Idx) -> &mut Node<T> {
        &mut self.nodes[idx as usize]
    }

    fn alloc_node(&mut self, height: u8) -> Idx {
        if let Some(idx) = self.free_list.pop() {
            let node = self.node_mut(idx);
            node.height = height;
            node.len = 0;
            for i in 0..MAX_HEIGHT {
                node.next[i] = NULL;
                node.widths[i] = 0;
            }
            idx
        } else {
            let idx = self.nodes.len() as Idx;
            self.nodes.push(Node::new(height));
            idx
        }
    }

    fn random_height(&mut self) -> u8 {
        self.rand_state ^= self.rand_state << 13;
        self.rand_state ^= self.rand_state >> 7;
        self.rand_state ^= self.rand_state << 17;
        let zeros = self.rand_state.trailing_zeros() as u8;
        (zeros / 2 + 1).min(MAX_HEIGHT as u8)
    }

    // --- Core operations ---

    /// Get item at index.
    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.len {
            return None;
        }
        let (node_idx, local) = self.find_node(index);
        let node = self.node(node_idx);
        debug_assert!(local < node.len(), "local {} >= node.len {}", local, node.len());
        Some(unsafe { node.items[local].assume_init_ref() })
    }

    /// Find the node containing the given index.
    /// Returns (node_index, local_index_within_node).
    fn find_node(&self, target: usize) -> (Idx, usize) {
        let mut idx = self.head;
        let mut remaining = target;

        // Traverse from top level down
        for level in (0..MAX_HEIGHT).rev() {
            loop {
                let node = self.node(idx);
                if level >= node.height() {
                    break;
                }
                let next = node.next[level];
                if next == NULL {
                    break;
                }
                let width = node.widths[level] as usize;
                if remaining >= width {
                    // Can skip this node's span
                    remaining -= width;
                    idx = next;
                } else {
                    // Target is within this span
                    break;
                }
            }
        }

        // If at head, move to first real node (head has no items)
        if idx == self.head {
            let next = self.node(self.head).next[0];
            if next != NULL {
                idx = next;
                // remaining stays the same (head has 0 items)
            }
        }

        // remaining is now the local index within idx
        (idx, remaining)
    }

    /// Insert item at index.
    pub fn insert(&mut self, index: usize, item: T) {
        assert!(index <= self.len, "index {} out of bounds (len {})", index, self.len);
        self.check_invariants();

        // Empty list case
        if self.is_empty() {
            self.insert_first(item);
            self.check_invariants();
            return;
        }

        // Find path to insertion point
        let (update, remaining_at) = self.find_path(index);

        // Find target node and local index
        let (target_idx, local_idx) = self.find_insert_point(&update, remaining_at[0]);

        // Insert into node (may split)
        self.insert_at(target_idx, local_idx, item, &update, &remaining_at, index);

        self.len += 1;
        self.check_invariants();
    }

    fn insert_first(&mut self, item: T) {
        let height = self.random_height();
        let new_idx = self.alloc_node(height);

        // Set up pointers and widths for all levels of HEAD
        // Levels below height point to new node, levels at/above height point to NULL
        let head_height = self.node(self.head).height();
        for level in 0..head_height {
            if level < height as usize {
                // Head points to new_node at this level
                self.node_mut(self.head).next[level] = new_idx;
                // Head has 0 items, so widths = 0 when pointing to a real node
                self.node_mut(self.head).widths[level] = 0;
            } else {
                // Head points to NULL at this level (new node doesn't have this level)
                // IMPORTANT: Clear any stale pointers from previous nodes!
                self.node_mut(self.head).next[level] = NULL;
                // widths = total items in list = 1
                self.node_mut(self.head).widths[level] = 1;
            }
        }

        // New node has 1 item
        let node = self.node_mut(new_idx);
        node.items[0] = MaybeUninit::new(item);
        node.len = 1;
        node.widths[0] = 1; // This node has 1 item
        for level in 1..height as usize {
            node.widths[level] = 1;
        }
        
        self.len = 1;
    }

    /// Find path through skip list, recording nodes for width updates.
    /// 
    /// Returns (update, remaining_at) where:
    /// - `update[level]` = the node whose span at this level contains the target
    /// - `remaining_at[level]` = offset from update[level] to target
    fn find_path(&self, target: usize) -> ([Idx; MAX_HEIGHT], [usize; MAX_HEIGHT]) {
        let mut update = [self.head; MAX_HEIGHT];
        let mut remaining_at = [target; MAX_HEIGHT];

        let mut idx = self.head;
        let mut remaining = target;

        for level in (0..MAX_HEIGHT).rev() {
            // Only record if current node has this level
            // (Initialize keeps HEAD as default, which is correct)
            if level < self.node(idx).height() {
                loop {
                    let node = self.node(idx);
                    let next = node.next[level];
                    if next == NULL {
                        break;
                    }
                    let width = node.widths[level] as usize;
                    if remaining >= width {
                        // Can skip this node's span
                        remaining -= width;
                        idx = next;
                    } else {
                        // Target is within this node's span
                        break;
                    }
                }
                update[level] = idx;
                remaining_at[level] = remaining;
            }
            // If level >= node.height(), we don't update - keep previous value
            // This means update[level] stays as the last tall-enough predecessor
        }

        (update, remaining_at)
    }

    /// Find the node and local index for insertion.
    fn find_insert_point(&self, update: &[Idx; MAX_HEIGHT], remaining: usize) -> (Idx, usize) {
        let mut idx = update[0];

        // If at head, move to first real node
        if idx == self.head {
            let next = self.node(self.head).next[0];
            if next != NULL {
                idx = next;
                // remaining stays the same since head has 0 items
            }
        }

        // remaining is the local index within this node
        (idx, remaining)
    }

    /// Insert item into node, splitting if necessary.
    fn insert_at(
        &mut self,
        node_idx: Idx,
        local_idx: usize,
        item: T,
        update: &[Idx; MAX_HEIGHT],
        update_pos: &[usize; MAX_HEIGHT],
        index: usize,
    ) {
        let node = self.node(node_idx);

        if node.has_room() {
            self.insert_in_node(node_idx, local_idx, item);
            // Increment widths, but handle the case where update[level] == node_idx
            // In that case, we're inserting INTO that node, so we need to increment
            // the predecessor's width, not the node's own width.
            self.increment_widths_for_insert(update, node_idx);
        } else {
            self.split_and_insert(node_idx, local_idx, item, update, update_pos, index);
        }
    }

    /// Insert into a node that has room.
    fn insert_in_node(&mut self, node_idx: Idx, local_idx: usize, item: T) {
        let node = self.node_mut(node_idx);
        let len = node.len();

        debug_assert!(len < CHUNK_SIZE, "node is full");
        debug_assert!(local_idx <= len, "local_idx {} > len {}", local_idx, len);

        // Shift items right
        for i in (local_idx..len).rev() {
            node.items[i + 1] = std::mem::replace(&mut node.items[i], MaybeUninit::uninit());
        }

        node.items[local_idx] = MaybeUninit::new(item);
        node.len += 1;
    }
    /// Increment widths after inserting into target_node.
    fn increment_widths_for_insert(&mut self, update: &[Idx; MAX_HEIGHT], target_node: Idx) {
        let target_height = self.node(target_node).height();
        
        // Increment target node's widths at all its levels
        for level in 0..target_height {
            let target = self.node_mut(target_node);
            target.widths[level] += 1;
        }
        
        // Increment predecessor's widths at levels above target's height
        for level in target_height..MAX_HEIGHT {
            let node_idx = update[level];
            let node = self.node_mut(node_idx);
            if level < node.height() {
                node.widths[level] += 1;
            }
        }
    }

    /// Split a full node and insert the item.
    fn split_and_insert(
        &mut self,
        node_idx: Idx,
        local_idx: usize,
        item: T,
        update: &[Idx; MAX_HEIGHT],
        remaining_at: &[usize; MAX_HEIGHT],
        _index: usize,
    ) {
        let new_height = self.random_height();
        let new_idx = self.alloc_node(new_height);
        let split = CHUNK_SIZE / 2;
        let insert_in_old = local_idx < split;

        // Move upper half to new node using raw pointers to avoid borrow issues
        unsafe {
            let nodes_ptr = self.nodes.as_mut_ptr();
            let old_ptr = nodes_ptr.add(node_idx as usize);
            let new_ptr = nodes_ptr.add(new_idx as usize);

            for i in split..CHUNK_SIZE {
                std::ptr::copy_nonoverlapping(
                    std::ptr::addr_of!((*old_ptr).items[i]),
                    std::ptr::addr_of_mut!((*new_ptr).items[i - split]),
                    1,
                );
            }
            (*new_ptr).len = (CHUNK_SIZE - split) as u16;
            (*old_ptr).len = split as u16;
        }

        // Insert into appropriate half
        let (old_final_len, new_final_len) = if insert_in_old {
            self.insert_in_node(node_idx, local_idx, item);
            (split + 1, CHUNK_SIZE - split)
        } else {
            self.insert_in_node(new_idx, local_idx - split, item);
            (split, CHUNK_SIZE - split + 1)
        };

        let old_height = self.node(node_idx).height();

        // Levels 0..old_height: wire old_node -> new_node
        for level in 0..old_height {
            let old_next = self.node(node_idx).next[level];
            
            self.node_mut(node_idx).next[level] = new_idx;
            self.node_mut(node_idx).widths[level] = old_final_len as u32;
            
            if level < new_height as usize {
                self.node_mut(new_idx).next[level] = old_next;
                self.node_mut(new_idx).widths[level] = new_final_len as u32;
            }
        }
        
        // Levels old_height..new_height: splice new_node into higher levels
        // At these levels, old_node is invisible, so we insert new_node between
        // the predecessor and whatever it previously pointed to.
        for level in old_height..(new_height as usize) {
            let pred_idx = update[level];
            let pred_height = self.node(pred_idx).height();
            
            if level < pred_height {
                let pred_old_width = self.node(pred_idx).widths[level];
                let pred_next = self.node(pred_idx).next[level];
                
                // pred_new_width = items from pred to new_node
                //                = (items before old_node) + (items in old_node after insert)
                let items_before_old_node = remaining_at[level] - local_idx;
                let pred_new_width = items_before_old_node + old_final_len;
                
                // new_node_width: total span increased by 1, so:
                // pred_new_width + new_node_width = pred_old_width + 1
                let new_node_width = (pred_old_width as usize) + 1 - pred_new_width;
                
                self.node_mut(new_idx).next[level] = pred_next;
                self.node_mut(new_idx).widths[level] = new_node_width as u32;
                
                self.node_mut(pred_idx).next[level] = new_idx;
                self.node_mut(pred_idx).widths[level] = pred_new_width as u32;
            }
        }
        
        // Levels >= max(new_height, old_height): just increment predecessor's width
        let max_involved_height = (new_height as usize).max(old_height);
        for level in max_involved_height..MAX_HEIGHT {
            let pred_idx = update[level];
            let pred = self.node_mut(pred_idx);
            if level < pred.height() {
                pred.widths[level] += 1;
            }
        }
    }

    /// Remove item at index.
    pub fn remove(&mut self, index: usize) -> T {
        assert!(index < self.len, "index out of bounds");
        self.check_invariants();

        // Find path to removal point
        let mut update = [self.head; MAX_HEIGHT];
        let mut idx = self.head;
        let mut remaining = index;

        for level in (0..MAX_HEIGHT).rev() {
            if level < self.node(idx).height() {
                loop {
                    let node = self.node(idx);
                    let next = node.next[level];
                    if next == NULL {
                        break;
                    }
                    let width = node.widths[level] as usize;
                    if remaining >= width {
                        remaining -= width;
                        idx = next;
                    } else {
                        break;
                    }
                }
                update[level] = idx;
            }
        }

        // Move from head to first real node if needed
        if idx == self.head {
            idx = self.node(self.head).next[0];
        }
        
        let target_idx = idx;
        let item = self.remove_from_node(target_idx, remaining);
        self.decrement_widths_for_remove(&update, target_idx);
        self.len -= 1;
        self.check_invariants();
        item
    }

    fn remove_from_node(&mut self, node_idx: Idx, local_idx: usize) -> T {
        let node = self.node_mut(node_idx);
        let len = node.len();

        let item = unsafe { node.items[local_idx].assume_init_read() };

        for i in local_idx..len - 1 {
            node.items[i] = std::mem::replace(&mut node.items[i + 1], MaybeUninit::uninit());
        }
        node.len -= 1;

        item
    }

    /// Decrement widths after removing from target_node.
    fn decrement_widths_for_remove(&mut self, update: &[Idx; MAX_HEIGHT], target_idx: Idx) {
        let target_height = self.node(target_idx).height();
        
        // Decrement target node's widths at all its levels
        for level in 0..target_height {
            let target = self.node_mut(target_idx);
            target.widths[level] = target.widths[level].saturating_sub(1);
        }
        
        // Decrement predecessor's widths at levels above target's height
        for level in target_height..MAX_HEIGHT {
            let pred_idx = update[level];
            let pred = self.node_mut(pred_idx);
            if level < pred.height() {
                pred.widths[level] = pred.widths[level].saturating_sub(1);
            }
        }
    }

    /// Iterate over all items.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        SkipListIter {
            list: self,
            node_idx: self.node(self.head).next[0],
            local_idx: 0,
        }
    }
}

impl<T> Default for SkipList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for SkipList<T> {
    fn drop(&mut self) {
        let mut idx = self.node(self.head).next[0];
        while idx != NULL {
            let node = self.node_mut(idx);
            for i in 0..node.len() {
                unsafe { node.items[i].assume_init_drop() };
            }
            idx = node.next[0];
        }
    }
}

struct SkipListIter<'a, T> {
    list: &'a SkipList<T>,
    node_idx: Idx,
    local_idx: usize,
}

impl<'a, T> Iterator for SkipListIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.node_idx == NULL {
            return None;
        }

        let node = self.list.node(self.node_idx);

        if self.local_idx >= node.len() {
            self.node_idx = node.next[0];
            self.local_idx = 0;
            return self.next();
        }

        let item = unsafe { node.items[self.local_idx].assume_init_ref() };
        self.local_idx += 1;
        Some(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_list() {
        let list: SkipList<i32> = SkipList::new();
        assert_eq!(list.len(), 0);
        assert!(list.is_empty());
        assert_eq!(list.get(0), None);
    }

    #[test]
    fn insert_one() {
        let mut list = SkipList::new();
        list.insert(0, 42);
        assert_eq!(list.len(), 1);
        assert_eq!(list.get(0), Some(&42));
    }

    #[test]
    fn insert_three_sequential() {
        let mut list = SkipList::new();
        list.insert(0, 0);
        assert_eq!(list.get(0), Some(&0));
        list.insert(1, 1);
        assert_eq!(list.get(0), Some(&0));
        assert_eq!(list.get(1), Some(&1));
        list.insert(2, 2);
        assert_eq!(list.get(0), Some(&0));
        assert_eq!(list.get(1), Some(&1));
        assert_eq!(list.get(2), Some(&2));
    }

    #[test]
    fn insert_fills_one_chunk() {
        let mut list = SkipList::new();
        for i in 0..CHUNK_SIZE {
            list.insert(i, i);
            assert_eq!(list.len(), i + 1);
        }
        for i in 0..CHUNK_SIZE {
            assert_eq!(list.get(i), Some(&i), "failed at index {}", i);
        }
    }

    #[test]
    fn insert_triggers_split() {
        let mut list = SkipList::new();
        // Fill first chunk
        for i in 0..CHUNK_SIZE {
            list.insert(i, i);
        }
        // This should trigger a split
        list.insert(CHUNK_SIZE, CHUNK_SIZE);
        assert_eq!(list.len(), CHUNK_SIZE + 1);
        
        // Verify all items
        for i in 0..=CHUNK_SIZE {
            assert_eq!(list.get(i), Some(&i), "failed at index {}", i);
        }
    }

    #[test]
    fn insert_many_sequential() {
        let mut list = SkipList::new();
        for i in 0..100 {
            list.insert(i, i);
            // Verify after each insert
            for j in 0..=i {
                assert_eq!(list.get(j), Some(&j), "failed at index {} after inserting {}", j, i);
            }
        }
        assert_eq!(list.len(), 100);
    }

    #[test]
    fn insert_at_beginning() {
        let mut list = SkipList::new();
        for i in 0..10 {
            list.insert(0, i);
        }
        // Should be 9, 8, 7, ..., 0
        for i in 0..10 {
            assert_eq!(list.get(i), Some(&(9 - i)));
        }
    }

    #[test]
    fn insert_in_middle() {
        let mut list = SkipList::new();
        list.insert(0, 0);
        list.insert(1, 2);
        list.insert(1, 1);
        assert_eq!(list.get(0), Some(&0));
        assert_eq!(list.get(1), Some(&1));
        assert_eq!(list.get(2), Some(&2));
    }

    #[test]
    fn remove_single() {
        let mut list = SkipList::new();
        list.insert(0, 42);
        let item = list.remove(0);
        assert_eq!(item, 42);
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn remove_middle() {
        let mut list = SkipList::new();
        for i in 0..5 {
            list.insert(i, i);
        }
        let item = list.remove(2);
        assert_eq!(item, 2);
        assert_eq!(list.len(), 4);
        assert_eq!(list.get(0), Some(&0));
        assert_eq!(list.get(1), Some(&1));
        assert_eq!(list.get(2), Some(&3));
        assert_eq!(list.get(3), Some(&4));
    }

    #[test]
    fn iterate() {
        let mut list = SkipList::new();
        for i in 0..10 {
            list.insert(i, i);
        }
        let items: Vec<_> = list.iter().copied().collect();
        assert_eq!(items, (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn remove_from_beginning() {
        let mut list = SkipList::new();
        for i in 0..10 {
            list.insert(i, i);
        }
        
        // Remove first item
        let item = list.remove(0);
        assert_eq!(item, 0);
        assert_eq!(list.len(), 9);
        
        // Verify remaining items shifted
        for i in 0..9 {
            assert_eq!(list.get(i), Some(&(i + 1)), "failed at index {}", i);
        }
    }

    #[test]
    fn remove_multiple() {
        let mut list = SkipList::new();
        for i in 0..10 {
            list.insert(i, i);
        }
        
        // Remove items 0, 2, 4 (removing from end of selection to preserve indices)
        list.remove(4);
        list.remove(2);
        list.remove(0);
        
        assert_eq!(list.len(), 7);
        // Remaining: 1, 3, 5, 6, 7, 8, 9
        let expected = [1, 3, 5, 6, 7, 8, 9];
        for (i, &exp) in expected.iter().enumerate() {
            assert_eq!(list.get(i), Some(&exp), "index {} expected {} got {:?}", i, exp, list.get(i));
        }
    }

    #[test]
    fn remove_across_chunks_100() {
        let mut list = SkipList::new();
        for i in 0..100 {
            list.insert(i, i);
        }
        
        for i in (0..50).rev() {
            list.remove(i * 2);
        }
        
        assert_eq!(list.len(), 50);
        for i in 0..50 {
            let expected = i * 2 + 1;
            assert_eq!(list.get(i), Some(&expected), "index {} expected {}", i, expected);
        }
    }

    #[test]
    fn trace_split() {
        let mut list = SkipList::new();
        
        // Insert 64 items (fills first chunk)
        for i in 0..64 {
            list.insert(i, i);
        }
        
        eprintln!("=== Before split (64 items) ===");
        list.debug_print();
        
        // Verify all values
        for i in 0..64 {
            assert_eq!(list.get(i), Some(&i), "pre-split: index {} wrong", i);
        }
        
        // Insert 65th item - triggers split
        eprintln!("\n=== Inserting item 64 (triggers split) ===");
        list.insert(64, 64);
        
        eprintln!("\n=== After split (65 items) ===");
        list.debug_print();
        
        // Verify all values
        for i in 0..65 {
            let got = list.get(i);
            if got != Some(&i) {
                eprintln!("ERROR: index {} expected {} got {:?}", i, i, got);
            }
        }
        
        for i in 0..65 {
            assert_eq!(list.get(i), Some(&i), "post-split: index {} wrong", i);
        }
    }
    
    #[test]
    fn trace_remove_200() {
        let mut list = SkipList::new();
        for i in 0..200 {
            list.insert(i, i);
        }
        
        // Remove every other item starting from the end
        for i in (0..100).rev() {
            let idx = i * 2;
            let removed = list.remove(idx);
            assert_eq!(removed, idx, "removed wrong value at index {}", idx);
        }
        
        assert_eq!(list.len(), 100);
        
        // Verify remaining items are odd numbers
        for i in 0..100 {
            let expected = i * 2 + 1;
            assert_eq!(list.get(i), Some(&expected), "index {} expected {}", i, expected);
        }
    }
    
    #[test]
    fn remove_across_chunks_200() {
        let mut list = SkipList::new();
        for i in 0..200 {
            list.insert(i, i);
        }
        
        for i in (0..100).rev() {
            list.remove(i * 2);
        }
        
        assert_eq!(list.len(), 100);
        for i in 0..100 {
            let expected = i * 2 + 1;
            assert_eq!(list.get(i), Some(&expected), "index {} expected {}", i, expected);
        }
    }

    #[test]
    fn remove_across_chunks_500() {
        let mut list = SkipList::new();
        for i in 0..500 {
            list.insert(i, i);
        }
        
        for i in (0..250).rev() {
            list.remove(i * 2);
        }
        
        assert_eq!(list.len(), 250);
        for i in 0..250 {
            let expected = i * 2 + 1;
            assert_eq!(list.get(i), Some(&expected), "index {} expected {}", i, expected);
        }
    }

    #[test]
    fn stress_test() {
        let mut list = SkipList::new();
        for i in 0..1000 {
            list.insert(i, i);
        }
        assert_eq!(list.len(), 1000);

        for i in 0..1000 {
            assert_eq!(list.get(i), Some(&i), "failed at {}", i);
        }

        // Remove every other item from the end
        for i in (0..500).rev() {
            list.remove(i * 2);
        }
        assert_eq!(list.len(), 500);
        
        // After removing 0, 2, 4, ..., 998, we should have 1, 3, 5, ..., 999
        for i in 0..500 {
            let expected = i * 2 + 1;
            assert_eq!(list.get(i), Some(&expected), "failed at index {}, expected {}", i, expected);
        }
    }
}

/// Minimal reproduction of mixed operations failure
#[test]
fn mixed_ops_repro() {
    let mut list = SkipList::new();
    let mut reference: Vec<u32> = Vec::new();
    
    // Pattern from proptest: many insert/remove at index 0
    for round in 0..100 {
        // Insert 10 items
        for i in 0..10 {
            list.insert(i, (round * 10 + i) as u32);
            reference.insert(i, (round * 10 + i) as u32);
        }
        
        // Insert/remove at index 0 a bunch of times
        for _ in 0..5 {
            list.insert(0, 999);
            reference.insert(0, 999);
            
            let skip_val = list.remove(0);
            let ref_val = reference.remove(0);
            assert_eq!(skip_val, ref_val, "remove mismatch");
        }
        
        // Check invariants
        let iter_count = list.iter().count();
        if iter_count != list.len() {
            eprintln!("!!! LENGTH MISMATCH at round {}", round);
            list.debug_print();
            panic!("iter()={} but len()={}", iter_count, list.len());
        }
        
        // Remove some items 
        while list.len() > 5 {
            let idx = list.len() / 2;
            let skip_val = list.remove(idx);
            let ref_val = reference.remove(idx);
            assert_eq!(skip_val, ref_val, "remove mismatch");
        }
        
        let iter_count = list.iter().count();
        if iter_count != list.len() {
            eprintln!("!!! LENGTH MISMATCH after removals at round {}", round);
            list.debug_print();
            panic!("iter()={} but len()={}", iter_count, list.len());
        }
    }
}

/// Minimal reproduction of proptest failure - random inserts
#[test]
fn proptest_minimal_repro() {
    // Inline the check function here since we can't easily import from sibling module
    fn check_all_invariants(list: &SkipList<u32>) -> Result<(), String> {
        // Check level 0 widths match node lengths
        let mut idx = list.node(list.head).next[0];
        while idx != NULL {
            let node = list.node(idx);
            if node.height() > 0 && node.widths[0] as usize != node.len() {
                return Err(format!("node {} widths[0]={} != len={}", idx, node.widths[0], node.len()));
            }
            idx = node.next[0];
        }
        
        // Check position consistency across levels
        let mut l0_positions: Vec<(Idx, usize)> = Vec::new();
        let mut pos = 0usize;
        let mut idx = list.node(list.head).next[0];
        while idx != NULL {
            l0_positions.push((idx, pos));
            let node = list.node(idx);
            pos += node.len();
            idx = node.next[0];
        }
        
        for level in 1..MAX_HEIGHT {
            let mut sum = 0usize;
            let mut idx = list.head;
            let mut path: Vec<(Idx, u32)> = Vec::new();
            while idx != NULL {
                let node = list.node(idx);
                if level < node.height() {
                    path.push((idx, node.widths[level]));
                    if idx != list.head {
                        if let Some(&(_, l0_pos)) = l0_positions.iter().find(|(n, _)| *n == idx) {
                            if sum != l0_pos {
                                eprintln!("=== INVARIANT FAILURE DEBUG ===");
                                eprintln!("level {} position mismatch at node {}: width_sum={} but l0_pos={}", level, idx, sum, l0_pos);
                                eprintln!("path at level {}: {:?}", level, path);
                                eprintln!("l0_positions: {:?}", l0_positions);
                                return Err(format!(
                                    "level {} position mismatch at node {}: width_sum={} but l0_pos={}",
                                    level, idx, sum, l0_pos
                                ));
                            }
                        }
                    }
                    sum += node.widths[level] as usize;
                    idx = node.next[level];
                } else {
                    break;
                }
            }
        }
        Ok(())
    }
    
    let mut list = SkipList::new();
    
    // Do many random inserts to trigger the bug
    let seed = 12345u64;
    let mut rng = seed;
    
    for i in 0..200 {
        // Simple xorshift for reproducibility
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        
        let index = if list.len() == 0 { 0 } else { (rng as usize) % (list.len() + 1) };
        let value = rng as u32;
        
        list.insert(index, value);
        
        if let Err(e) = check_all_invariants(&list) {
            panic!("Invariant failed after insert {} at index {} (len now {}): {}", i, index, list.len(), e);
        }
    }
}

/// Property-based tests using proptest.
/// 
/// These tests verify that all invariants (documented at module level) hold after
/// any sequence of operations. Uses a reference Vec<T> as oracle.
#[cfg(test)]
pub(super) mod proptests {
    use super::*;
    use proptest::prelude::*;
    
    /// Reference implementation: a simple Vec wrapper.
    /// This is obviously correct and serves as the oracle.
    #[derive(Clone, Debug, Default)]
    pub struct Reference<T>(Vec<T>);
    
    impl<T: Clone> Reference<T> {
        pub fn new() -> Self { Reference(Vec::new()) }
        pub fn len(&self) -> usize { self.0.len() }
        pub fn insert(&mut self, index: usize, item: T) { self.0.insert(index, item); }
        pub fn remove(&mut self, index: usize) -> T { self.0.remove(index) }
        pub fn get(&self, index: usize) -> Option<&T> { self.0.get(index) }
    }
    
    /// Check invariant 0: all nodes reachable at higher levels are also reachable at level 0
    fn check_level0_reachability<T>(list: &SkipList<T>) -> Result<(), String> {
        // Build set of nodes reachable via level 0
        let mut l0_reachable = std::collections::HashSet::new();
        let mut idx = list.head;
        while idx != NULL {
            l0_reachable.insert(idx);
            idx = list.node(idx).next[0];
        }
        
        // Check that all nodes pointed to at higher levels are in l0_reachable
        for level in 1..MAX_HEIGHT {
            let mut idx = list.head;
            while idx != NULL {
                let node = list.node(idx);
                if level < node.height() {
                    let next = node.next[level];
                    if next != NULL && !l0_reachable.contains(&next) {
                        return Err(format!(
                            "node {} at level {} points to node {} which is not reachable via level 0!\n\
                             level 0 reachable: {:?}",
                            idx, level, next, l0_reachable
                        ));
                    }
                    idx = next;
                } else {
                    break;
                }
            }
        }
        Ok(())
    }
    
    /// Check invariant 1: iter().count() == len()
    fn check_length_consistency<T>(list: &SkipList<T>) -> Result<(), String> {
        let iter_count = list.iter().count();
        if iter_count != list.len() {
            return Err(format!(
                "length mismatch: iter().count()={} but len()={}", 
                iter_count, list.len()
            ));
        }
        Ok(())
    }
    
    /// Check invariant 2: sum of widths at level 0 == len()
    /// Also check that each node's width[0] equals its len (item count)
    fn check_width_sum<T>(list: &SkipList<T>) -> Result<(), String> {
        let mut sum = 0usize;
        let mut idx = list.head;
        
        while idx != NULL {
            let node = list.node(idx);
            if node.height() > 0 {
                // Skip head node which has items.len = 0
                if idx != list.head {
                    // For non-head nodes: widths[0] should equal node.len()
                    // But wait - widths[0] is the distance to the NEXT node
                    // For the predecessor, pred.widths[0] = items in next node
                    // So node.widths[0] should NOT necessarily equal node.len()
                    // Let's just sum them
                }
                sum += node.widths[0] as usize;
                idx = node.next[0];
            } else {
                break;
            }
        }
        
        if sum != list.len() {
            return Err(format!(
                "width sum mismatch: sum of widths[0]={} but len()={}", 
                sum, list.len()
            ));
        }
        Ok(())
    }
    
    /// Check invariant 2b: node.widths[0] should equal node.len() (except for head)
    /// This is because widths[0] = items to skip when moving from this node to next
    fn check_width_len_agreement<T>(list: &SkipList<T>) -> Result<(), String> {
        // Skip head (it has no items but points to first real node)
        let mut idx = list.node(list.head).next[0];
        
        while idx != NULL {
            let node = list.node(idx);
            
            if node.height() > 0 {
                let node_width = node.widths[0] as usize;
                let node_len = node.len();
                
                if node_width != node_len {
                    return Err(format!(
                        "width/len mismatch: node {} widths[0]={} but len()={}", 
                        idx, node_width, node_len
                    ));
                }
            }
            
            idx = node.next[0];
        }
        Ok(())
    }
    
    /// Check invariant 2c: width sums at ALL levels equal len (when traversable)
    /// Also check that higher-level widths are consistent with lower-level widths
    fn check_all_level_widths<T>(list: &SkipList<T>) -> Result<(), String> {
        // First, build cumulative position map at level 0
        let mut l0_positions: Vec<(Idx, usize)> = Vec::new(); // (node_idx, start_position)
        let mut pos = 0usize;
        let mut idx = list.node(list.head).next[0];
        
        while idx != NULL {
            l0_positions.push((idx, pos));
            let node = list.node(idx);
            pos += node.len();
            idx = node.next[0];
        }
        
        // Check sum at each level
        for level in 0..MAX_HEIGHT {
            let mut sum = 0usize;
            let mut idx = list.head;
            let mut can_traverse = true;
            
            while idx != NULL && can_traverse {
                let node = list.node(idx);
                if level < node.height() {
                    // For nodes other than head, verify position matches level 0
                    if idx != list.head && level > 0 {
                        // Find this node's position in l0_positions
                        if let Some(&(_, l0_pos)) = l0_positions.iter().find(|(n, _)| *n == idx) {
                            if sum != l0_pos {
                                return Err(format!(
                                    "level {} position mismatch at node {}: width_sum={} but l0_pos={}",
                                    level, idx, sum, l0_pos
                                ));
                            }
                        }
                    }
                    
                    sum += node.widths[level] as usize;
                    idx = node.next[level];
                } else {
                    can_traverse = false;
                }
            }
            
            // Only check if we could fully traverse (reached NULL)
            if can_traverse && idx == NULL && sum != list.len() {
                return Err(format!(
                    "level {} width sum {} != len {}", level, sum, list.len()
                ));
            }
        }
        Ok(())
    }
    
    /// Check invariant 3: get(i) == iter().nth(i) for all valid i
    fn check_traversal_correctness<T: PartialEq + std::fmt::Debug>(list: &SkipList<T>) -> Result<(), String> {
        let items: Vec<_> = list.iter().collect();
        
        for i in 0..list.len() {
            let by_get = list.get(i);
            let by_iter = items.get(i).copied();
            
            if by_get != by_iter {
                // Debug: trace find_node for this index
                let (node_idx, local_idx) = list.find_node(i);
                let node = list.node(node_idx);
                
                // Walk level 0 to find position
                let mut l0_idx = list.node(list.head).next[0];
                let mut l0_pos = 0usize;
                let mut l0_node_idx_at_i = NULL;
                let mut l0_local_at_i = 0usize;
                
                while l0_idx != NULL {
                    let n = list.node(l0_idx);
                    if l0_pos + n.len() > i {
                        l0_node_idx_at_i = l0_idx;
                        l0_local_at_i = i - l0_pos;
                        break;
                    }
                    l0_pos += n.len();
                    l0_idx = n.next[0];
                }
                
                return Err(format!(
                    "traversal mismatch at index {}: get()={:?} but iter()={:?}\n\
                     find_node returned (node={}, local={}), node.len={}\n\
                     level0 walk says (node={}, local={})",
                    i, by_get, by_iter,
                    node_idx, local_idx, node.len(),
                    l0_node_idx_at_i, l0_local_at_i
                ));
            }
        }
        Ok(())
    }
    
    /// Check all invariants
    pub fn check_all_invariants<T: PartialEq + std::fmt::Debug>(list: &SkipList<T>) -> Result<(), String> {
        check_level0_reachability(list)?;
        check_length_consistency(list)?;
        check_width_sum(list)?;
        check_width_len_agreement(list)?;
        check_all_level_widths(list)?;
        check_traversal_correctness(list)?;
        Ok(())
    }
    
    /// Check that skip list agrees with reference implementation
    fn check_agreement<T: PartialEq + Clone + std::fmt::Debug>(
        list: &SkipList<T>, 
        reference: &Reference<T>
    ) -> Result<(), String> {
        if list.len() != reference.len() {
            return Err(format!(
                "length disagreement: skip_list.len()={} but reference.len()={}",
                list.len(), reference.len()
            ));
        }
        
        for i in 0..list.len() {
            let skip_val = list.get(i);
            let ref_val = reference.get(i);
            
            if skip_val != ref_val {
                return Err(format!(
                    "value disagreement at index {}: skip_list={:?} but reference={:?}",
                    i, skip_val, ref_val
                ));
            }
        }
        Ok(())
    }
    
    /// Operation that can be applied to both implementations
    #[derive(Clone, Debug)]
    enum Op {
        Insert { index: usize, value: u32 },
        Remove { index: usize },
    }
    
    /// Generate a sequence of operations
    fn ops_strategy(max_ops: usize) -> impl Strategy<Value = Vec<Op>> {
        // Start with some inserts to build up the list, then mix operations
        let initial_inserts: Vec<Op> = (0..10)
            .map(|i| Op::Insert { index: i, value: i as u32 })
            .collect();
        
        (0..max_ops).prop_flat_map(move |num_ops| {
            let initial = initial_inserts.clone();
            proptest::collection::vec(any::<(bool, u32)>(), num_ops)
                .prop_map(move |choices| {
                    let mut ops = initial.clone();
                    let mut current_len = ops.len();
                    
                    for (is_insert, value) in choices {
                        if current_len == 0 || is_insert {
                            let index = if current_len == 0 { 0 } else { (value as usize) % (current_len + 1) };
                            ops.push(Op::Insert { index, value });
                            current_len += 1;
                        } else {
                            let index = (value as usize) % current_len;
                            ops.push(Op::Remove { index });
                            current_len -= 1;
                        }
                    }
                    ops
                })
        })
    }
    
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]
        
        /// Test that sequential inserts maintain invariants
        #[test]
        fn sequential_inserts_maintain_invariants(count in 1usize..500) {
            let mut list = SkipList::new();
            let mut reference = Reference::new();
            
            for i in 0..count {
                list.insert(i, i as u32);
                reference.insert(i, i as u32);
                
                check_all_invariants(&list).map_err(|e| TestCaseError::fail(e))?;
                check_agreement(&list, &reference).map_err(|e| TestCaseError::fail(e))?;
            }
        }
        
        /// Test that inserts at random positions maintain invariants
        #[test]
        fn random_inserts_maintain_invariants(insertions in proptest::collection::vec((0usize..1000, any::<u32>()), 1..200)) {
            let mut list = SkipList::new();
            let mut reference = Reference::new();
            
            for (raw_index, value) in insertions {
                let index = if list.len() == 0 { 0 } else { raw_index % (list.len() + 1) };
                
                list.insert(index, value);
                reference.insert(index, value);
                
                check_all_invariants(&list).map_err(|e| TestCaseError::fail(e))?;
                check_agreement(&list, &reference).map_err(|e| TestCaseError::fail(e))?;
            }
        }
        
        /// Test that mixed insert/remove operations maintain invariants
        #[test]
        fn mixed_operations_maintain_invariants(ops in ops_strategy(100)) {
            let mut list = SkipList::new();
            let mut reference = Reference::new();
            
            for op in ops.iter() {
                // Check BEFORE the operation too
                check_level0_reachability(&list).map_err(|e| TestCaseError::fail(e))?;
                
                match op {
                    Op::Insert { index, value } => {
                        list.insert(*index, *value);
                        reference.insert(*index, *value);
                    }
                    Op::Remove { index } => {
                        let skip_val = list.remove(*index);
                        let ref_val = reference.remove(*index);
                        prop_assert_eq!(skip_val, ref_val, "remove returned different values");
                    }
                }
                
                check_all_invariants(&list).map_err(|e| TestCaseError::fail(e))?;
                check_agreement(&list, &reference).map_err(|e| TestCaseError::fail(e))?;
            }
        }
        
        /// Test that removing all items works correctly
        #[test]
        fn insert_then_remove_all(count in 1usize..200) {
            let mut list = SkipList::new();
            let mut reference = Reference::new();
            
            // Insert
            for i in 0..count {
                list.insert(i, i as u32);
                reference.insert(i, i as u32);
            }
            
            check_all_invariants(&list).map_err(|e| TestCaseError::fail(e))?;
            
            // Remove from end to avoid index shifting complexity
            for _ in 0..count {
                let idx = list.len() - 1;
                let skip_val = list.remove(idx);
                let ref_val = reference.remove(idx);
                prop_assert_eq!(skip_val, ref_val);
                
                check_all_invariants(&list).map_err(|e| TestCaseError::fail(e))?;
                check_agreement(&list, &reference).map_err(|e| TestCaseError::fail(e))?;
            }
            
            prop_assert_eq!(list.len(), 0);
        }
    }
}


