// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-01-30"
// driver = "Isaac Clayton"

//! Weighted Skip List
//!
//! A skip list where each item has an associated weight. Position lookups
//! use cumulative weights rather than item indices. This is designed for
//! RGA integration where spans have variable visible lengths.
//!
//! Key differences from the unrolled SkipList:
//! - One item per node (no chunking)
//! - Weights are u64 (to handle large character counts)
//! - Position lookup by cumulative weight
//! - Support for updating weights (e.g., when marking spans as deleted)

/// Maximum skip list height. 16 levels covers billions of items.
const MAX_HEIGHT: usize = 16;

/// Node index type.
type Idx = u32;

/// Null index marker.
const NULL: Idx = Idx::MAX;

/// A node in the weighted skip list.
struct Node<T> {
    /// The item stored in this node.
    item: Option<T>,
    /// Height of this node's tower (for future O(log n) optimization).
    #[allow(dead_code)]
    height: u8,
    /// Forward pointers at each level.
    next: [Idx; MAX_HEIGHT],
    /// Weight sums at each level (for future O(log n) optimization).
    /// widths[level] = total weight from this node to next[level] (exclusive).
    #[allow(dead_code)]
    widths: [u64; MAX_HEIGHT],
    /// This item's weight.
    weight: u64,
}

impl<T> Node<T> {
    fn new(item: T, weight: u64, height: u8) -> Self {
        let mut node = Node {
            item: Some(item),
            height,
            next: [NULL; MAX_HEIGHT],
            widths: [0; MAX_HEIGHT],
            weight,
        };
        // At level 0, width equals this item's weight
        node.widths[0] = weight;
        node
    }

    fn new_head() -> Self {
        Node {
            item: None,
            height: MAX_HEIGHT as u8,
            next: [NULL; MAX_HEIGHT],
            widths: [0; MAX_HEIGHT],
            weight: 0,
        }
    }

    #[allow(dead_code)]
    fn height(&self) -> usize {
        self.height as usize
    }
}

/// A weighted skip list with O(log n) operations.
pub struct WeightedList<T> {
    /// Arena of nodes.
    nodes: Vec<Node<T>>,
    /// Head node (sentinel).
    head: Idx,
    /// Total weight of all items.
    total_weight: u64,
    /// Number of items.
    len: usize,
    /// Free list for node reuse.
    free_list: Vec<Idx>,
    /// Random state for height generation.
    rand_state: u64,
}

impl<T> WeightedList<T> {
    /// Create a new empty weighted list.
    pub fn new() -> Self {
        let mut list = WeightedList {
            nodes: Vec::new(),
            head: 0,
            total_weight: 0,
            len: 0,
            free_list: Vec::new(),
            rand_state: 0x12345678_9abcdef0,
        };
        // Allocate head node
        list.nodes.push(Node::new_head());
        list
    }

    /// Get the total weight of all items.
    pub fn total_weight(&self) -> u64 {
        self.total_weight
    }

    /// Get the number of items.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    // --- Node access ---

    fn node(&self, idx: Idx) -> &Node<T> {
        &self.nodes[idx as usize]
    }

    fn node_mut(&mut self, idx: Idx) -> &mut Node<T> {
        &mut self.nodes[idx as usize]
    }

    fn alloc_node(&mut self, item: T, weight: u64, height: u8) -> Idx {
        if let Some(idx) = self.free_list.pop() {
            self.nodes[idx as usize] = Node::new(item, weight, height);
            idx
        } else {
            let idx = self.nodes.len() as Idx;
            self.nodes.push(Node::new(item, weight, height));
            idx
        }
    }

    fn random_height(&mut self) -> u8 {
        // xorshift64
        let mut x = self.rand_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rand_state = x;

        // Geometric distribution via trailing zeros
        let zeros = x.trailing_zeros() as u8;
        (zeros.min(MAX_HEIGHT as u8 - 1)) + 1
    }

    /// Find the item containing the given weight position.
    /// Returns (item_index, offset_within_item) or None if pos >= total_weight.
    ///
    /// Weight position semantics:
    /// - Position 0 is the start of the first item
    /// - Position W-1 is the last unit of an item with weight W
    /// - Position W is the start of the next item
    pub fn find_by_weight(&self, pos: u64) -> Option<(usize, u64)> {
        if pos >= self.total_weight {
            return None;
        }

        // Simple O(n) implementation for now - walk level 0
        // TODO: Use higher levels for O(log n) lookup
        let mut idx = self.node(self.head).next[0];
        let mut cumulative = 0u64;
        let mut item_index = 0usize;

        while idx != NULL {
            let weight = self.node(idx).weight;
            if cumulative + weight > pos {
                // Found the item containing pos
                return Some((item_index, pos - cumulative));
            }
            cumulative += weight;
            item_index += 1;
            idx = self.node(idx).next[0];
        }

        // Should not reach here if pos < total_weight
        None
    }

    /// Insert an item at the given index with the given weight.
    pub fn insert(&mut self, index: usize, item: T, weight: u64) {
        assert!(index <= self.len, "index out of bounds");

        let height = self.random_height();
        let new_idx = self.alloc_node(item, weight, height);

        // Simple O(n) implementation: walk level 0 to find insertion point
        // TODO: Use higher levels for O(log n) insertion
        
        // Find predecessor at level 0
        let mut pred_idx = self.head;
        for _ in 0..index {
            let next = self.node(pred_idx).next[0];
            if next == NULL {
                break;
            }
            pred_idx = next;
        }

        // Wire up at level 0
        let old_next = self.node(pred_idx).next[0];
        self.node_mut(new_idx).next[0] = old_next;
        self.node_mut(pred_idx).next[0] = new_idx;

        // For now, only use level 0 (degrades to linked list)
        // Higher level optimization can be added later

        self.total_weight += weight;
        self.len += 1;
    }

    /// Get the item at the given index.
    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.len {
            return None;
        }
        
        let mut idx = self.node(self.head).next[0];
        for _ in 0..index {
            if idx == NULL {
                return None;
            }
            idx = self.node(idx).next[0];
        }
        
        self.node(idx).item.as_ref()
    }

    /// Get a mutable reference to the item at the given index.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index >= self.len {
            return None;
        }
        
        let mut idx = self.node(self.head).next[0];
        for _ in 0..index {
            if idx == NULL {
                return None;
            }
            idx = self.node(idx).next[0];
        }
        
        self.node_mut(idx).item.as_mut()
    }

    /// Update the weight of an item at the given index.
    /// Returns the old weight.
    pub fn update_weight(&mut self, index: usize, new_weight: u64) -> u64 {
        assert!(index < self.len, "index out of bounds");

        // Find the node at the given index
        let mut idx = self.node(self.head).next[0];
        for _ in 0..index {
            idx = self.node(idx).next[0];
        }

        let old_weight = self.node(idx).weight;
        let delta = new_weight as i64 - old_weight as i64;

        // Update the node's weight
        self.node_mut(idx).weight = new_weight;

        // Update total weight
        self.total_weight = (self.total_weight as i64 + delta) as u64;
        
        old_weight
    }

    /// Iterate over all items.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        WeightedListIter {
            list: self,
            idx: self.node(self.head).next[0],
        }
    }

    /// Iterate over all items with their indices.
    pub fn iter_enumerate(&self) -> impl Iterator<Item = (usize, &T)> {
        self.iter().enumerate()
    }
}

impl<T> Default for WeightedList<T> {
    fn default() -> Self {
        Self::new()
    }
}

struct WeightedListIter<'a, T> {
    list: &'a WeightedList<T>,
    idx: Idx,
}

impl<'a, T> Iterator for WeightedListIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx == NULL {
            return None;
        }
        let node = self.list.node(self.idx);
        self.idx = node.next[0];
        node.item.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_list() {
        let list: WeightedList<u32> = WeightedList::new();
        assert_eq!(list.len(), 0);
        assert_eq!(list.total_weight(), 0);
        assert!(list.is_empty());
    }

    #[test]
    fn insert_single() {
        let mut list = WeightedList::new();
        list.insert(0, "hello", 5);
        
        assert_eq!(list.len(), 1);
        assert_eq!(list.total_weight(), 5);
        assert_eq!(list.get(0), Some(&"hello"));
    }

    #[test]
    fn insert_multiple() {
        let mut list = WeightedList::new();
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
    fn find_by_weight_single() {
        let mut list = WeightedList::new();
        list.insert(0, "hello", 10);
        
        // Position 0-9 should all map to item 0
        assert_eq!(list.find_by_weight(0), Some((0, 0)));
        assert_eq!(list.find_by_weight(5), Some((0, 5)));
        assert_eq!(list.find_by_weight(9), Some((0, 9)));
        
        // Position 10 is past the end
        assert_eq!(list.find_by_weight(10), None);
    }

    #[test]
    fn find_by_weight_multiple() {
        let mut list = WeightedList::new();
        list.insert(0, "a", 5);   // positions 0-4
        list.insert(1, "b", 10);  // positions 5-14
        list.insert(2, "c", 3);   // positions 15-17
        
        // First item
        assert_eq!(list.find_by_weight(0), Some((0, 0)));
        assert_eq!(list.find_by_weight(4), Some((0, 4)));
        
        // Second item
        assert_eq!(list.find_by_weight(5), Some((1, 0)));
        assert_eq!(list.find_by_weight(14), Some((1, 9)));
        
        // Third item
        assert_eq!(list.find_by_weight(15), Some((2, 0)));
        assert_eq!(list.find_by_weight(17), Some((2, 2)));
        
        // Past end
        assert_eq!(list.find_by_weight(18), None);
    }

    #[test]
    fn update_weight() {
        let mut list = WeightedList::new();
        list.insert(0, "a", 10);
        
        assert_eq!(list.total_weight(), 10);
        
        let old = list.update_weight(0, 5);
        assert_eq!(old, 10);
        assert_eq!(list.total_weight(), 5);
        
        // find_by_weight should reflect new weight
        assert_eq!(list.find_by_weight(4), Some((0, 4)));
        assert_eq!(list.find_by_weight(5), None);
    }

    #[test]
    fn update_weight_to_zero() {
        let mut list = WeightedList::new();
        list.insert(0, "a", 5);
        list.insert(1, "b", 10);
        list.insert(2, "c", 3);
        
        assert_eq!(list.total_weight(), 18);
        
        // "Delete" middle item by setting weight to 0
        list.update_weight(1, 0);
        assert_eq!(list.total_weight(), 8);
        
        // find_by_weight should skip the zero-weight item
        assert_eq!(list.find_by_weight(4), Some((0, 4)));
        // Position 5 should now be in item 2 (the third one)
        assert_eq!(list.find_by_weight(5), Some((2, 0)));
    }

    #[test]
    fn iterate() {
        let mut list = WeightedList::new();
        list.insert(0, 1u32, 5);
        list.insert(1, 2u32, 10);
        list.insert(2, 3u32, 3);
        
        let items: Vec<_> = list.iter().cloned().collect();
        assert_eq!(items, vec![1, 2, 3]);
    }
}
