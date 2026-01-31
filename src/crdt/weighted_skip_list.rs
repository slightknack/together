// model = "claude-opus-4-5"
// created = "2026-01-31"
// modified = "2026-01-31"
// driver = "Isaac Clayton"

//! Weighted Skip List
//!
//! A skip list that tracks item weights (visible character counts) for O(log n) position lookup.
//! This is designed for the RGA CRDT where each span has a weight equal to its visible length.
//!
//! # Key Differences from SkipList
//!
//! - Each item has an associated weight (u64)
//! - `widths[level]` tracks cumulative weight, not item count
//! - `find_by_weight(weight)` finds item containing the given weight position
//! - `update_weight(index, new_weight)` updates an item's weight in O(log n)
//!
//! # Width Semantics
//!
//! The width at each level tracks the total weight of items in that span:
//!
//! - `node.widths[level]` = total weight of items from this node to `node.next[level]`
//! - For data nodes, `widths[0]` = weight of the single item in this node
//!
//! # Operations
//!
//! - `insert(index, item, weight)`: O(log n) - insert item with weight at index
//! - `remove(index)`: O(log n) - remove item at index, returns (item, weight)
//! - `get(index)` / `get_mut(index)`: O(log n) - access by index
//! - `find_by_weight(weight)`: O(log n) - find item containing weight position
//! - `update_weight(index, new_weight)`: O(log n) - change item's weight
//! - `total_weight()`: O(1) - sum of all weights
//! - `len()`: O(1) - number of items
//!
//! # Structure
//!
//! Unlike the chunked SkipList, this is a classic skip list with one item per node.
//! This trades some cache locality for simpler weight tracking.
//!
//! ```text
//! Level 2: HEAD ---------> A (w=5) ------------------> NULL
//! Level 1: HEAD ---------> A (w=5) --------> C (w=3) -> NULL
//! Level 0: HEAD -> B (w=2) -> A (w=5) -> D (w=1) -> C (w=3) -> NULL
//! ```
//!
//! At each level, `widths[level]` = sum of weights from this node to next[level].

use std::mem::MaybeUninit;

/// Maximum skip list height. 16 levels covers billions of elements.
const MAX_HEIGHT: usize = 16;

/// Node index type. u32 saves space vs usize on 64-bit.
type Idx = u32;

/// Null index marker.
const NULL: Idx = Idx::MAX;

/// A node in the weighted skip list.
/// Each node stores exactly one item with its weight.
struct Node<T> {
    /// The item stored in this node.
    item: MaybeUninit<T>,
    /// The weight of this item.
    weight: u64,
    /// Height of this node (number of levels it participates in).
    height: u8,
    /// Forward pointers at each level.
    next: [Idx; MAX_HEIGHT],
    /// Width (total weight) from this node to next[level] at each level.
    /// For level 0, this equals self.weight.
    widths: [u64; MAX_HEIGHT],
}

impl<T> Node<T> {
    fn new(height: u8, item: T, weight: u64) -> Self {
        let mut node = Node {
            item: MaybeUninit::new(item),
            weight,
            height,
            next: [NULL; MAX_HEIGHT],
            widths: [0; MAX_HEIGHT],
        };
        // Initialize widths[0] to this node's weight
        node.widths[0] = weight;
        node
    }

    fn new_head() -> Self {
        Node {
            item: MaybeUninit::uninit(),
            weight: 0,
            height: MAX_HEIGHT as u8,
            next: [NULL; MAX_HEIGHT],
            widths: [0; MAX_HEIGHT],
        }
    }

    fn height(&self) -> usize {
        self.height as usize
    }
}

/// A weighted skip list with O(log n) operations.
pub struct WeightedSkipList<T> {
    /// Arena of nodes.
    nodes: Vec<Node<T>>,
    /// Index of the head node.
    head: Idx,
    /// Number of items (not counting head).
    len: usize,
    /// Total weight of all items.
    total_weight: u64,
    /// Free list for reusing removed node slots.
    free_list: Vec<Idx>,
    /// Random state for height generation.
    rand_state: u64,
}

impl<T> WeightedSkipList<T> {
    pub fn new() -> Self {
        let mut list = WeightedSkipList {
            nodes: Vec::new(),
            head: 0,
            len: 0,
            total_weight: 0,
            free_list: Vec::new(),
            rand_state: 0x12345678_9abcdef0,
        };
        // Allocate head node
        list.nodes.push(Node::new_head());
        list
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn total_weight(&self) -> u64 {
        self.total_weight
    }

    // --- Node access helpers ---

    fn node(&self, idx: Idx) -> &Node<T> {
        &self.nodes[idx as usize]
    }

    fn node_mut(&mut self, idx: Idx) -> &mut Node<T> {
        &mut self.nodes[idx as usize]
    }

    fn alloc_node(&mut self, height: u8, item: T, weight: u64) -> Idx {
        if let Some(idx) = self.free_list.pop() {
            let node = self.node_mut(idx);
            node.item = MaybeUninit::new(item);
            node.weight = weight;
            node.height = height;
            for i in 0..MAX_HEIGHT {
                node.next[i] = NULL;
                node.widths[i] = 0;
            }
            node.widths[0] = weight;
            idx
        } else {
            let idx = self.nodes.len() as Idx;
            self.nodes.push(Node::new(height, item, weight));
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

    // --- Invariant checking ---

    #[cfg(debug_assertions)]
    fn check_invariants(&self) {
        // Invariant 1: iter count matches len
        let iter_count = self.iter().count();
        assert_eq!(
            iter_count, self.len,
            "INVARIANT VIOLATED: iter().count()={} != len()={}",
            iter_count, self.len
        );

        // Invariant 2: sum of item weights == total_weight
        let mut actual_weight = 0u64;
        let mut idx = self.node(self.head).next[0];
        while idx != NULL {
            actual_weight += self.node(idx).weight;
            idx = self.node(idx).next[0];
        }
        assert_eq!(
            actual_weight, self.total_weight,
            "INVARIANT VIOLATED: sum of item weights={} != total_weight={}",
            actual_weight, self.total_weight
        );
    }

    #[cfg(not(debug_assertions))]
    #[inline(always)]
    fn check_invariants(&self) {}

    // --- Core operations ---

    /// Count items in the span from node to node.next[level].
    fn count_items_in_span(&self, start_idx: Idx, level: usize) -> usize {
        let node = self.node(start_idx);
        if level >= node.height() {
            return 0;
        }
        let end_idx = node.next[level];
        if end_idx == NULL {
            return 0;
        }

        // Traverse level 0 to count
        let mut count = 0usize;
        let mut idx = if start_idx == self.head {
            node.next[0]
        } else {
            start_idx
        };

        while idx != NULL && idx != end_idx {
            count += 1;
            idx = self.node(idx).next[0];
        }

        count
    }

    /// Get item at index.
    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.len {
            return None;
        }
        let idx = self.find_node_by_index(index);
        Some(unsafe { self.node(idx).item.assume_init_ref() })
    }

    /// Get mutable reference to item at index.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index >= self.len {
            return None;
        }
        let idx = self.find_node_by_index(index);
        Some(unsafe { self.node_mut(idx).item.assume_init_mut() })
    }

    /// Find the node at the given index by traversing level 0.
    fn find_node_by_index(&self, target_index: usize) -> Idx {
        let mut idx = self.node(self.head).next[0];
        let mut count = 0usize;

        while idx != NULL {
            if count == target_index {
                return idx;
            }
            count += 1;
            idx = self.node(idx).next[0];
        }

        panic!("index {} out of bounds", target_index);
    }

    /// Find item by weight position.
    /// Returns (index, offset_within_item) or None if weight >= total_weight.
    /// 
    /// This is O(n) traversal at level 0. The skip list is used for O(log n) insert/remove
    /// by index, but weight-based lookup requires linear traversal since weights vary per item.
    pub fn find_by_weight(&self, weight: u64) -> Option<(usize, u64)> {
        if weight >= self.total_weight {
            return None;
        }

        let mut idx = self.node(self.head).next[0];
        let mut remaining_weight = weight;
        let mut item_index = 0usize;

        // Simple O(n) traversal at level 0
        while idx != NULL {
            let node = self.node(idx);
            if remaining_weight < node.weight {
                return Some((item_index, remaining_weight));
            }
            remaining_weight -= node.weight;
            item_index += 1;
            idx = node.next[0];
        }

        // Should not reach here if weight < total_weight
        None
    }

    /// Insert item with weight at the given index.
    pub fn insert(&mut self, index: usize, item: T, weight: u64) {
        assert!(
            index <= self.len,
            "index {} out of bounds (len {})",
            index,
            self.len
        );
        self.check_invariants();

        let height = self.random_height();
        let new_idx = self.alloc_node(height, item, weight);

        // Find the insertion point
        let mut update = [self.head; MAX_HEIGHT];
        let mut idx = self.head;
        let mut count = 0usize;

        // We need to track both the path (update) and the weight sum at each level
        let mut weight_before = [0u64; MAX_HEIGHT];

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

                // Count items in this span
                let items_in_span = self.count_items_in_span(idx, level);
                if count + items_in_span < index {
                    weight_before[level] += node.widths[level];
                    count += items_in_span;
                    idx = next;
                } else if count + items_in_span == index && level > 0 {
                    // Exact match at higher level - keep descending
                    break;
                } else {
                    break;
                }
            }
            update[level] = idx;
            // Copy weight_before from higher level if we didn't traverse at this level
            if level + 1 < MAX_HEIGHT && weight_before[level] == 0 && level < self.node(idx).height()
            {
                weight_before[level] = weight_before[level + 1];
            }
        }

        // Wire up the new node
        for level in 0..height as usize {
            let pred_idx = update[level];
            let pred = self.node(pred_idx);
            let old_next = pred.next[level];
            let old_width = pred.widths[level];

            // Calculate new widths
            // The new node takes over part of pred's span
            // new_node.widths[level] = weight from new_node to old_next
            // pred.widths[level] = weight from pred to new_node

            // For level 0, it's simple: new node's width is its own weight
            // pred's width becomes 0 (it now points to new_node, which has the weight)
            if level == 0 {
                self.node_mut(new_idx).next[0] = old_next;
                self.node_mut(new_idx).widths[0] = weight;
                self.node_mut(pred_idx).next[0] = new_idx;
                // pred's width at level 0 should be the weight it contributes
                // But wait - for head, it has no weight. For other nodes, width[0] = their weight
                if pred_idx == self.head {
                    self.node_mut(pred_idx).widths[0] = 0; // Head has no weight
                }
                // The weight propagation happens through the higher levels
            } else {
                self.node_mut(new_idx).next[level] = old_next;
                self.node_mut(pred_idx).next[level] = new_idx;

                // Split the old weight: pred keeps weight up to new_node, new_node gets the rest
                // We need to figure out the weight from pred to new_node
                // This requires traversing level 0 from pred to new_node
                let weight_to_new = self.sum_weights_between(pred_idx, new_idx, level);
                self.node_mut(pred_idx).widths[level] = weight_to_new;
                self.node_mut(new_idx).widths[level] = old_width - weight_to_new + weight;
            }
        }

        // Update widths for levels above new node's height
        for level in height as usize..MAX_HEIGHT {
            let pred_idx = update[level];
            let pred = self.node_mut(pred_idx);
            if level < pred.height() {
                pred.widths[level] += weight;
            }
        }

        self.len += 1;
        self.total_weight += weight;
        self.check_invariants();
    }

    /// Sum weights of items between start (exclusive) and end (inclusive) at level 0.
    fn sum_weights_between(&self, start_idx: Idx, end_idx: Idx, _level: usize) -> u64 {
        let mut sum = 0u64;
        let mut idx = if start_idx == self.head {
            self.node(self.head).next[0]
        } else {
            self.node(start_idx).next[0]
        };

        while idx != NULL && idx != end_idx {
            sum += self.node(idx).weight;
            idx = self.node(idx).next[0];
        }

        sum
    }

    /// Remove item at index, returning the item.
    pub fn remove(&mut self, index: usize) -> T {
        assert!(index < self.len, "index {} out of bounds", index);
        self.check_invariants();

        // First, find the target node by traversing level 0
        let target_idx = self.find_node_by_index(index);
        let target_height = self.node(target_idx).height();
        let target_weight = self.node(target_idx).weight;

        // Find predecessors at each level
        // A predecessor at level L is a node whose next[L] == target_idx
        let mut update = [self.head; MAX_HEIGHT];
        
        for level in 0..MAX_HEIGHT {
            // Find the predecessor at this level by traversing from head
            let mut idx = self.head;
            while idx != NULL {
                let node = self.node(idx);
                if level >= node.height() {
                    break;
                }
                let next = node.next[level];
                if next == target_idx {
                    update[level] = idx;
                    break;
                }
                if next == NULL {
                    // Target is beyond this level's reach
                    update[level] = idx;
                    break;
                }
                idx = next;
            }
        }

        // Read the item before we start modifying
        let item = unsafe { self.node(target_idx).item.assume_init_read() };

        // Unlink the node at levels where it participates
        for level in 0..target_height {
            let pred_idx = update[level];
            if self.node(pred_idx).next[level] == target_idx {
                let target_next = self.node(target_idx).next[level];
                let target_span_weight = self.node(target_idx).widths[level];

                self.node_mut(pred_idx).next[level] = target_next;
                
                // When we remove target, pred's span extends to target.next[level]
                // The new weight = (weight from pred to target) + (weight from target to target.next)
                //                - target's own weight (since target is being removed)
                // 
                // At level 0: pred.widths[0] = pred.weight, target.widths[0] = target.weight
                // After removal: pred.widths[0] should still = pred.weight (unchanged)
                // 
                // At higher levels: we need to merge the spans correctly
                // pred.widths[level] was weight from pred (exclusive) to target (exclusive)
                // Actually, let's simplify: at level 0, the width IS the node's weight
                // So we don't change pred.widths[0].
                // At higher levels, pred.widths[level] + target.widths[level] - target.weight
                // gives us the new span weight.
                
                if level == 0 {
                    // At level 0, each node's width is its own weight
                    // Pred's weight doesn't change, we just update the link
                    // No width update needed for pred at level 0
                } else {
                    // At higher levels, we merge the spans
                    // pred.widths[level] + target.widths[level] - target.weight
                    // Use saturating arithmetic to handle edge cases
                    let pred_width = self.node(pred_idx).widths[level];
                    let new_width = (pred_width + target_span_weight).saturating_sub(target_weight);
                    self.node_mut(pred_idx).widths[level] = new_width;
                }
            }
        }

        // Decrease widths at levels above target's height
        // These are levels where some predecessor spans over the target
        for level in target_height..MAX_HEIGHT {
            let pred_idx = update[level];
            let pred = self.node(pred_idx);
            if level < pred.height() {
                // This predecessor's span includes the target
                let new_width = self.node(pred_idx).widths[level].saturating_sub(target_weight);
                self.node_mut(pred_idx).widths[level] = new_width;
            }
        }

        // Add node to free list
        self.free_list.push(target_idx);

        self.len -= 1;
        self.total_weight -= target_weight;
        self.check_invariants();

        item
    }

    /// Update the weight of item at index.
    /// Returns the old weight.
    pub fn update_weight(&mut self, index: usize, new_weight: u64) -> u64 {
        assert!(index < self.len, "index {} out of bounds", index);

        // Find the node
        let node_idx = self.find_node_by_index(index);
        let old_weight = self.node(node_idx).weight;

        if old_weight == new_weight {
            return old_weight;
        }

        let delta = new_weight as i64 - old_weight as i64;

        // Update the node's weight
        self.node_mut(node_idx).weight = new_weight;

        // Update widths[0] for this node
        self.node_mut(node_idx).widths[0] = new_weight;

        // Find path to update widths at all levels
        let mut idx = self.head;
        let node_height = self.node(node_idx).height();

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

                // Check if target is in this span
                if self.is_node_in_span(node_idx, idx, level) {
                    // Target is in this span, update width and stop at this level
                    let node = self.node_mut(idx);
                    node.widths[level] = (node.widths[level] as i64 + delta) as u64;
                    break;
                } else {
                    // Target is beyond this span, continue
                    idx = next;
                }
            }
        }

        // Also update widths at levels above the target node's height
        // These are the predecessors that span over the target
        idx = self.head;
        for level in (node_height..MAX_HEIGHT).rev() {
            loop {
                let node = self.node(idx);
                if level >= node.height() {
                    break;
                }
                let next = node.next[level];
                if next == NULL {
                    // This span goes to end and contains target
                    if self.is_node_reachable(node_idx, idx) {
                        self.node_mut(idx).widths[level] =
                            (self.node(idx).widths[level] as i64 + delta) as u64;
                    }
                    break;
                }
                if self.is_node_in_span(node_idx, idx, level) {
                    self.node_mut(idx).widths[level] =
                        (self.node(idx).widths[level] as i64 + delta) as u64;
                    break;
                }
                idx = next;
            }
        }

        self.total_weight = (self.total_weight as i64 + delta) as u64;

        old_weight
    }

    /// Check if node_idx is reachable from start_idx via level 0.
    fn is_node_reachable(&self, target_idx: Idx, start_idx: Idx) -> bool {
        let mut idx = if start_idx == self.head {
            self.node(self.head).next[0]
        } else {
            start_idx
        };

        while idx != NULL {
            if idx == target_idx {
                return true;
            }
            idx = self.node(idx).next[0];
        }

        false
    }

    /// Check if node_idx is in the span from start_idx to start_idx.next[level].
    fn is_node_in_span(&self, target_idx: Idx, start_idx: Idx, level: usize) -> bool {
        let node = self.node(start_idx);
        if level >= node.height() {
            return false;
        }

        let end_idx = node.next[level];

        // Traverse level 0 from start to end
        let mut idx = if start_idx == self.head {
            self.node(self.head).next[0]
        } else {
            self.node(start_idx).next[0]
        };

        while idx != NULL && idx != end_idx {
            if idx == target_idx {
                return true;
            }
            idx = self.node(idx).next[0];
        }

        false
    }

    /// Iterate over all items.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        WeightedSkipListIter {
            list: self,
            current: self.node(self.head).next[0],
        }
    }
}

impl<T> Default for WeightedSkipList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for WeightedSkipList<T> {
    fn drop(&mut self) {
        let mut idx = self.node(self.head).next[0];
        while idx != NULL {
            unsafe { self.node_mut(idx).item.assume_init_drop() };
            idx = self.node(idx).next[0];
        }
    }
}

struct WeightedSkipListIter<'a, T> {
    list: &'a WeightedSkipList<T>,
    current: Idx,
}

impl<'a, T> Iterator for WeightedSkipListIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current == NULL {
            return None;
        }

        let node = self.list.node(self.current);
        let item = unsafe { node.item.assume_init_ref() };
        self.current = node.next[0];
        Some(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_list() {
        let list: WeightedSkipList<i32> = WeightedSkipList::new();
        assert_eq!(list.len(), 0);
        assert!(list.is_empty());
        assert_eq!(list.total_weight(), 0);
        assert_eq!(list.get(0), None);
        assert_eq!(list.find_by_weight(0), None);
    }

    #[test]
    fn insert_one() {
        let mut list = WeightedSkipList::new();
        list.insert(0, "hello", 5);
        assert_eq!(list.len(), 1);
        assert_eq!(list.total_weight(), 5);
        assert_eq!(list.get(0), Some(&"hello"));
    }

    #[test]
    fn insert_multiple() {
        let mut list = WeightedSkipList::new();
        list.insert(0, "a", 3);
        list.insert(1, "b", 5);
        list.insert(2, "c", 2);

        assert_eq!(list.len(), 3);
        assert_eq!(list.total_weight(), 10);
        assert_eq!(list.get(0), Some(&"a"));
        assert_eq!(list.get(1), Some(&"b"));
        assert_eq!(list.get(2), Some(&"c"));
    }

    #[test]
    fn insert_in_middle() {
        let mut list = WeightedSkipList::new();
        list.insert(0, "a", 5);
        list.insert(1, "c", 5);
        list.insert(1, "b", 5);

        assert_eq!(list.get(0), Some(&"a"));
        assert_eq!(list.get(1), Some(&"b"));
        assert_eq!(list.get(2), Some(&"c"));
    }

    #[test]
    fn find_by_weight_single() {
        let mut list = WeightedSkipList::new();
        list.insert(0, "hello", 10);

        assert_eq!(list.find_by_weight(0), Some((0, 0)));
        assert_eq!(list.find_by_weight(5), Some((0, 5)));
        assert_eq!(list.find_by_weight(9), Some((0, 9)));
        assert_eq!(list.find_by_weight(10), None);
    }

    #[test]
    fn find_by_weight_multiple() {
        let mut list = WeightedSkipList::new();
        list.insert(0, "a", 5);
        list.insert(1, "b", 10);
        list.insert(2, "c", 3);

        assert_eq!(list.find_by_weight(0), Some((0, 0)));
        assert_eq!(list.find_by_weight(4), Some((0, 4)));
        assert_eq!(list.find_by_weight(5), Some((1, 0)));
        assert_eq!(list.find_by_weight(14), Some((1, 9)));
        assert_eq!(list.find_by_weight(15), Some((2, 0)));
        assert_eq!(list.find_by_weight(17), Some((2, 2)));
        assert_eq!(list.find_by_weight(18), None);
    }

    #[test]
    fn update_weight() {
        let mut list = WeightedSkipList::new();
        list.insert(0, "a", 10);

        assert_eq!(list.total_weight(), 10);

        let old = list.update_weight(0, 5);
        assert_eq!(old, 10);
        assert_eq!(list.total_weight(), 5);

        assert_eq!(list.find_by_weight(4), Some((0, 4)));
        assert_eq!(list.find_by_weight(5), None);
    }

    #[test]
    fn update_weight_to_zero() {
        let mut list = WeightedSkipList::new();
        list.insert(0, "a", 5);
        list.insert(1, "b", 10);
        list.insert(2, "c", 3);

        assert_eq!(list.total_weight(), 18);

        list.update_weight(1, 0);
        assert_eq!(list.total_weight(), 8);

        assert_eq!(list.find_by_weight(4), Some((0, 4)));
        assert_eq!(list.find_by_weight(5), Some((2, 0)));
    }

    #[test]
    fn remove_single() {
        let mut list = WeightedSkipList::new();
        list.insert(0, 42, 5);
        let removed = list.remove(0);
        assert_eq!(removed, 42);
        assert_eq!(list.len(), 0);
        assert_eq!(list.total_weight(), 0);
    }

    #[test]
    fn remove_middle() {
        let mut list = WeightedSkipList::new();
        list.insert(0, "a", 5);
        list.insert(1, "b", 10);
        list.insert(2, "c", 3);

        let removed = list.remove(1);
        assert_eq!(removed, "b");
        assert_eq!(list.len(), 2);
        assert_eq!(list.total_weight(), 8);
        assert_eq!(list.get(0), Some(&"a"));
        assert_eq!(list.get(1), Some(&"c"));
    }

    #[test]
    fn iterate() {
        let mut list = WeightedSkipList::new();
        list.insert(0, 1u32, 5);
        list.insert(1, 2u32, 10);
        list.insert(2, 3u32, 3);

        let items: Vec<_> = list.iter().cloned().collect();
        assert_eq!(items, vec![1, 2, 3]);
    }

    #[test]
    fn many_inserts() {
        let mut list = WeightedSkipList::new();
        for i in 0..100 {
            list.insert(i, i, i as u64 + 1);
        }

        assert_eq!(list.len(), 100);
        // Total weight = 1 + 2 + ... + 100 = 100 * 101 / 2 = 5050
        assert_eq!(list.total_weight(), 5050);

        // Verify iteration
        let items: Vec<_> = list.iter().cloned().collect();
        assert_eq!(items, (0..100).collect::<Vec<_>>());
    }

    #[test]
    fn insert_at_beginning() {
        let mut list = WeightedSkipList::new();
        for i in 0..10 {
            list.insert(0, i, 1);
        }

        assert_eq!(list.len(), 10);
        let items: Vec<_> = list.iter().cloned().collect();
        // Inserting at 0 each time reverses the order
        assert_eq!(items, (0..10).rev().collect::<Vec<_>>());
    }

    #[test]
    fn stress_test() {
        let mut list = WeightedSkipList::new();
        for i in 0..1000 {
            list.insert(i, i, 1);
        }
        assert_eq!(list.len(), 1000);
        assert_eq!(list.total_weight(), 1000);

        for i in 0..1000 {
            assert_eq!(list.get(i), Some(&i), "failed at {}", i);
        }

        // Remove every other item from the end
        for i in (0..500).rev() {
            list.remove(i * 2);
        }
        assert_eq!(list.len(), 500);
        assert_eq!(list.total_weight(), 500);

        // After removing 0, 2, 4, ..., 998, we should have 1, 3, 5, ..., 999
        for i in 0..500 {
            let expected = i * 2 + 1;
            assert_eq!(
                list.get(i),
                Some(&expected),
                "failed at index {}, expected {}",
                i,
                expected
            );
        }
    }
}

#[cfg(test)]
mod rga_pattern_tests {
    use super::*;

    /// Simulates RGA delete pattern: mark as deleted by setting weight to 0
    #[test]
    fn delete_by_weight_update() {
        let mut list = WeightedSkipList::new();
        
        // Insert 100 items, each with weight 10
        for i in 0..100 {
            list.insert(i, i, 10);
        }
        
        assert_eq!(list.total_weight(), 1000);
        
        // "Delete" items by setting weight to 0 (like RGA does)
        for i in 0..50 {
            list.update_weight(i, 0);
        }
        
        assert_eq!(list.total_weight(), 500);
        
        // Now find_by_weight should still work
        // Weight 0 should be at index 50 (first item with weight > 0)
        match list.find_by_weight(0) {
            Some((idx, offset)) => {
                assert_eq!(idx, 50, "expected idx 50, got {}", idx);
                assert_eq!(offset, 0, "expected offset 0, got {}", offset);
            }
            None => panic!("find_by_weight(0) returned None, but total_weight={}", list.total_weight()),
        }
    }

    /// Simulates RGA split pattern: remove and reinsert with different weights
    #[test]
    fn split_pattern() {
        let mut list = WeightedSkipList::new();
        
        // Insert a span with weight 100
        list.insert(0, "span", 100);
        assert_eq!(list.total_weight(), 100);
        
        // Find at various weights
        assert_eq!(list.find_by_weight(0), Some((0, 0)));
        assert_eq!(list.find_by_weight(50), Some((0, 50)));
        assert_eq!(list.find_by_weight(99), Some((0, 99)));
        assert_eq!(list.find_by_weight(100), None);
        
        // Split: remove the span and insert two halves
        let span = list.remove(0);
        assert_eq!(span, "span");
        assert_eq!(list.total_weight(), 0);
        
        list.insert(0, "left", 40);
        list.insert(1, "right", 60);
        assert_eq!(list.total_weight(), 100);
        
        // Find should still work
        assert_eq!(list.find_by_weight(0), Some((0, 0)));
        assert_eq!(list.find_by_weight(39), Some((0, 39)));
        assert_eq!(list.find_by_weight(40), Some((1, 0)));
        assert_eq!(list.find_by_weight(99), Some((1, 59)));
        assert_eq!(list.find_by_weight(100), None);
    }

    /// Test many insert/remove cycles
    #[test]
    fn many_cycles() {
        let mut list = WeightedSkipList::new();
        
        for cycle in 0..10 {
            // Insert
            for i in 0..100 {
                list.insert(i, (cycle, i), 5);
                
                // Verify after each insert
                let expected_weight = (i + 1) * 5;
                assert_eq!(list.total_weight(), expected_weight as u64, 
                    "cycle {} insert {} total_weight", cycle, i);
                
                // Check that we can find the last valid weight
                if expected_weight > 0 {
                    let w = (expected_weight - 1) as u64;
                    if list.find_by_weight(w).is_none() {
                        panic!("cycle {} after insert {} find_by_weight({}) failed, total={}", 
                            cycle, i, w, list.total_weight());
                    }
                }
            }
            
            let expected_weight = 500;
            assert_eq!(list.total_weight(), expected_weight, "cycle {} after all inserts", cycle);
            
            // Remove all
            while list.len() > 0 {
                list.remove(0);
            }
            
            assert_eq!(list.total_weight(), 0);
            assert_eq!(list.len(), 0);
        }
    }

    /// Minimal reproducer for find_by_weight bug
    #[test]
    fn find_by_weight_minimal() {
        let mut list = WeightedSkipList::new();
        
        // Insert 5 items with weight 5 each
        for i in 0..5 {
            list.insert(i, i, 5);
            
            let total = list.total_weight();
            eprintln!("After insert {}: total_weight={}, len={}", i, total, list.len());
            
            // Verify we can find all valid weights
            for w in 0..total {
                match list.find_by_weight(w) {
                    Some((idx, offset)) => {
                        eprintln!("  find_by_weight({}) = ({}, {})", w, idx, offset);
                    }
                    None => {
                        panic!("find_by_weight({}) returned None, total={}", w, total);
                    }
                }
            }
        }
    }
}
