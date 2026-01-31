// model = "claude-opus-4-5"
// created = "2026-01-31"
// modified = "2026-01-31"
// driver = "Isaac Clayton"

//! B-tree Weighted List
//!
//! A weighted list implemented as a B-tree for O(log n) operations.
//! Inspired by diamond-types' ContentTree.
//!
//! Structure:
//! - Leaf nodes store up to LEAF_SIZE items with their weights
//! - Internal nodes store up to NODE_SIZE children with cumulative subtree weights
//! - All nodes are stored in Vecs (no raw pointers)
//!
//! Operations:
//! - find_by_weight: O(log n) - binary search through tree levels
//! - insert: O(log n) amortized - may trigger splits
//! - remove: O(log n) amortized - may trigger merges
//! - get/get_mut: O(log n) - traverse to leaf
//! - update_weight: O(log n) - traverse and update ancestors

const LEAF_SIZE: usize = 64;
const NODE_SIZE: usize = 32;

/// Index into the leaf array.
type LeafIdx = u32;
/// Index into the node array.
type NodeIdx = u32;
/// Sentinel value for no parent / no child.
const NONE: u32 = u32::MAX;

/// A leaf node containing items and their weights.
#[derive(Clone, Debug)]
struct Leaf<T> {
    /// Items stored in this leaf.
    items: Vec<(T, u64)>,
    /// Total weight of all items in this leaf.
    total_weight: u64,
    /// Parent node index (NONE for root leaf).
    parent: NodeIdx,
    /// Index of this leaf in the parent's children array.
    index_in_parent: u8,
}

impl<T> Leaf<T> {
    fn new() -> Leaf<T> {
        return Leaf {
            items: Vec::with_capacity(LEAF_SIZE),
            total_weight: 0,
            parent: NONE,
            index_in_parent: 0,
        };
    }

    #[inline(always)]
    fn len(&self) -> usize {
        return self.items.len();
    }

    #[inline(always)]
    fn is_full(&self) -> bool {
        return self.items.len() >= LEAF_SIZE;
    }

    #[inline(always)]
    #[allow(dead_code)]
    fn can_donate(&self) -> bool {
        return self.items.len() > LEAF_SIZE / 2;
    }

    /// Find item by weight within this leaf.
    /// Returns (index, offset_within_item).
    #[inline]
    fn find_by_weight(&self, pos: u64) -> Option<(usize, u64)> {
        let mut cumulative = 0u64;
        for (i, (_, weight)) in self.items.iter().enumerate() {
            let next = cumulative + weight;
            if next > pos {
                return Some((i, pos - cumulative));
            }
            cumulative = next;
        }
        return None;
    }

    /// Split this leaf, returning the right half.
    fn split(&mut self) -> Leaf<T> {
        let mid = self.items.len() / 2;
        let right_items: Vec<_> = self.items.drain(mid..).collect();
        let right_weight: u64 = right_items.iter().map(|(_, w)| *w).sum();
        self.total_weight -= right_weight;
        return Leaf {
            items: right_items,
            total_weight: right_weight,
            parent: NONE,
            index_in_parent: 0,
        };
    }
}

/// An internal node containing child indices and cumulative weights.
#[derive(Clone, Debug)]
struct Node {
    /// Child indices. For height > 1, these are NodeIdx into nodes array.
    /// For height == 1, these are LeafIdx into leaves array.
    children: Vec<u32>,
    /// Weight of each child's subtree.
    child_weights: Vec<u64>,
    /// Item count of each child's subtree.
    child_counts: Vec<usize>,
    /// Total weight of all children.
    total_weight: u64,
    /// Total item count in subtree.
    total_count: usize,
    /// Parent node index (NONE for root).
    parent: NodeIdx,
    /// Index of this node in the parent's children array.
    index_in_parent: u8,
}

impl Node {
    fn new() -> Node {
        return Node {
            children: Vec::with_capacity(NODE_SIZE),
            child_weights: Vec::with_capacity(NODE_SIZE),
            child_counts: Vec::with_capacity(NODE_SIZE),
            total_weight: 0,
            total_count: 0,
            parent: NONE,
            index_in_parent: 0,
        };
    }

    #[inline(always)]
    #[allow(dead_code)]
    fn len(&self) -> usize {
        return self.children.len();
    }

    #[inline(always)]
    fn is_full(&self) -> bool {
        return self.children.len() >= NODE_SIZE;
    }

    #[inline(always)]
    #[allow(dead_code)]
    fn can_donate(&self) -> bool {
        return self.children.len() > NODE_SIZE / 2;
    }

    /// Find the child containing the given weight position.
    /// Returns (child_index, offset_in_child, items_before_child).
    #[inline]
    fn find_child_by_weight(&self, pos: u64) -> Option<(usize, u64, usize)> {
        let mut weight_cumulative = 0u64;
        let mut count_cumulative = 0usize;
        for i in 0..self.child_weights.len() {
            let weight = self.child_weights[i];
            let next = weight_cumulative + weight;
            if next > pos {
                return Some((i, pos - weight_cumulative, count_cumulative));
            }
            weight_cumulative = next;
            count_cumulative += self.child_counts[i];
        }
        return None;
    }

    /// Find the child containing the given item index.
    /// Returns (child_index, offset_in_child).
    #[inline]
    fn find_child_by_index(&self, index: usize) -> (usize, usize) {
        let mut cumulative = 0usize;
        for (i, &count) in self.child_counts.iter().enumerate() {
            let next = cumulative + count;
            if next > index {
                return (i, index - cumulative);
            }
            cumulative = next;
        }
        // Return last child with the excess
        let last = self.children.len().saturating_sub(1);
        return (last, index - cumulative + self.child_counts[last]);
    }

    /// Split this node, returning the right half.
    fn split(&mut self) -> Node {
        let mid = self.children.len() / 2;
        let right_children: Vec<_> = self.children.drain(mid..).collect();
        let right_weights: Vec<_> = self.child_weights.drain(mid..).collect();
        let right_counts: Vec<_> = self.child_counts.drain(mid..).collect();
        let right_weight: u64 = right_weights.iter().sum();
        let right_count: usize = right_counts.iter().sum();
        self.total_weight -= right_weight;
        self.total_count -= right_count;
        
        return Node {
            children: right_children,
            child_weights: right_weights,
            child_counts: right_counts,
            total_weight: right_weight,
            total_count: right_count,
            parent: NONE,
            index_in_parent: 0,
        };
    }
}

/// A weighted list implemented as a B-tree.
pub struct BTreeList<T> {
    /// Leaf nodes.
    leaves: Vec<Leaf<T>>,
    /// Internal nodes.
    nodes: Vec<Node>,
    /// Root index. If height == 0, this is a LeafIdx. Otherwise NodeIdx.
    root: u32,
    /// Tree height. 0 means root is a leaf.
    height: usize,
    /// Total weight of all items.
    total_weight: u64,
    /// Total number of items.
    len: usize,
    /// Free list for leaves (indices of removed leaves).
    free_leaves: Vec<LeafIdx>,
    /// Free list for nodes (indices of removed nodes).
    free_nodes: Vec<NodeIdx>,
}

impl<T: Clone> BTreeList<T> {
    pub fn new() -> BTreeList<T> {
        let mut leaves = Vec::new();
        leaves.push(Leaf::new());
        return BTreeList {
            leaves,
            nodes: Vec::new(),
            root: 0,
            height: 0,
            total_weight: 0,
            len: 0,
            free_leaves: Vec::new(),
            free_nodes: Vec::new(),
        };
    }

    #[inline(always)]
    pub fn total_weight(&self) -> u64 {
        return self.total_weight;
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        return self.len;
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        return self.len == 0;
    }

    /// Allocate a new leaf, reusing from free list if available.
    fn alloc_leaf(&mut self) -> LeafIdx {
        if let Some(idx) = self.free_leaves.pop() {
            self.leaves[idx as usize] = Leaf::new();
            return idx;
        }
        let idx = self.leaves.len() as LeafIdx;
        self.leaves.push(Leaf::new());
        return idx;
    }

    /// Allocate a new node, reusing from free list if available.
    fn alloc_node(&mut self) -> NodeIdx {
        if let Some(idx) = self.free_nodes.pop() {
            self.nodes[idx as usize] = Node::new();
            return idx;
        }
        let idx = self.nodes.len() as NodeIdx;
        self.nodes.push(Node::new());
        return idx;
    }

    /// Find the leaf containing the given weight position.
    /// Returns (leaf_idx, offset_in_leaf, items_before_leaf).
    #[inline]
    fn find_leaf_by_weight(&self, pos: u64) -> Option<(LeafIdx, u64, usize)> {
        if pos >= self.total_weight {
            return None;
        }

        if self.height == 0 {
            return Some((self.root, pos, 0));
        }

        let mut node_idx = self.root;
        let mut offset = pos;
        let mut items_before = 0usize;
        let mut current_height = self.height;

        while current_height > 1 {
            let node = &self.nodes[node_idx as usize];
            let (child_idx, new_offset, count_before) = node.find_child_by_weight(offset)?;
            items_before += count_before;
            node_idx = node.children[child_idx];
            offset = new_offset;
            current_height -= 1;
        }

        // At height 1, children are leaves
        let node = &self.nodes[node_idx as usize];
        let (child_idx, new_offset, count_before) = node.find_child_by_weight(offset)?;
        items_before += count_before;

        return Some((node.children[child_idx], new_offset, items_before));
    }

    /// Find the leaf containing the given item index.
    /// Returns (leaf_idx, index_in_leaf).
    #[inline]
    fn find_leaf_by_index(&self, index: usize) -> (LeafIdx, usize) {
        if index >= self.len {
            // For insert at end
            if self.height == 0 {
                return (self.root, self.leaves[self.root as usize].len());
            }
            // Find the rightmost leaf
            let mut node_idx = self.root;
            let mut current_height = self.height;
            while current_height > 1 {
                let node = &self.nodes[node_idx as usize];
                node_idx = *node.children.last().unwrap();
                current_height -= 1;
            }
            let node = &self.nodes[node_idx as usize];
            let leaf_idx = *node.children.last().unwrap();
            return (leaf_idx, self.leaves[leaf_idx as usize].len());
        }

        if self.height == 0 {
            return (self.root, index);
        }

        let mut node_idx = self.root;
        let mut offset = index;
        let mut current_height = self.height;

        while current_height > 1 {
            let node = &self.nodes[node_idx as usize];
            let (child_idx, new_offset) = node.find_child_by_index(offset);
            node_idx = node.children[child_idx];
            offset = new_offset;
            current_height -= 1;
        }

        // At height 1, children are leaves
        let node = &self.nodes[node_idx as usize];
        let (child_idx, new_offset) = node.find_child_by_index(offset);
        return (node.children[child_idx], new_offset);
    }

    /// Find the item containing the given weight position.
    /// Returns (item_index, offset_within_item) or None if pos >= total_weight.
    #[inline]
    pub fn find_by_weight(&self, pos: u64) -> Option<(usize, u64)> {
        let (leaf_idx, offset_in_leaf, items_before) = self.find_leaf_by_weight(pos)?;
        let leaf = &self.leaves[leaf_idx as usize];
        let (idx_in_leaf, offset_in_item) = leaf.find_by_weight(offset_in_leaf)?;
        return Some((items_before + idx_in_leaf, offset_in_item));
    }

    /// Find by weight and return leaf location along with item info.
    /// Returns (item_index, offset_in_item, leaf_idx, idx_in_leaf).
    #[inline]
    pub fn find_by_weight_with_chunk(&self, pos: u64) -> Option<(usize, u64, usize, usize)> {
        let (leaf_idx, offset_in_leaf, items_before) = self.find_leaf_by_weight(pos)?;
        let leaf = &self.leaves[leaf_idx as usize];
        let (idx_in_leaf, offset_in_item) = leaf.find_by_weight(offset_in_leaf)?;
        return Some((items_before + idx_in_leaf, offset_in_item, leaf_idx as usize, idx_in_leaf));
    }

    /// Update weight in ancestors after a leaf weight change.
    /// Update both weight and count in ancestors after a leaf change.
    /// This combines what would be two traversals into one.
    #[inline]
    fn update_ancestors(&mut self, leaf_idx: LeafIdx, weight_delta: i64, count_delta: i64) {
        let leaf = &self.leaves[leaf_idx as usize];
        let mut parent = leaf.parent;
        let mut child_index = leaf.index_in_parent as usize;

        while parent != NONE {
            let node = &mut self.nodes[parent as usize];
            node.child_weights[child_index] = (node.child_weights[child_index] as i64 + weight_delta) as u64;
            node.total_weight = (node.total_weight as i64 + weight_delta) as u64;
            node.child_counts[child_index] = (node.child_counts[child_index] as i64 + count_delta) as usize;
            node.total_count = (node.total_count as i64 + count_delta) as usize;
            child_index = node.index_in_parent as usize;
            parent = node.parent;
        }
    }

    /// Update weight in ancestors (count unchanged).
    #[inline]
    fn update_ancestor_weights(&mut self, leaf_idx: LeafIdx, delta: i64) {
        let leaf = &self.leaves[leaf_idx as usize];
        let mut parent = leaf.parent;
        let mut child_index = leaf.index_in_parent as usize;

        while parent != NONE {
            let node = &mut self.nodes[parent as usize];
            node.child_weights[child_index] = (node.child_weights[child_index] as i64 + delta) as u64;
            node.total_weight = (node.total_weight as i64 + delta) as u64;
            child_index = node.index_in_parent as usize;
            parent = node.parent;
        }
    }

    /// Insert an item at the given index with the given weight.
    pub fn insert(&mut self, index: usize, item: T, weight: u64) {
        let (leaf_idx, idx_in_leaf) = self.find_leaf_by_index(index);
        
        // Insert into leaf
        let leaf = &mut self.leaves[leaf_idx as usize];
        leaf.items.insert(idx_in_leaf, (item, weight));
        leaf.total_weight += weight;
        
        self.total_weight += weight;
        self.len += 1;

        // Update ancestor weights and counts in a single traversal
        if self.height > 0 {
            self.update_ancestors(leaf_idx, weight as i64, 1);
        }

        // Split if necessary
        if self.leaves[leaf_idx as usize].is_full() {
            self.split_leaf(leaf_idx);
        }
    }

    /// Split a full leaf.
    fn split_leaf(&mut self, leaf_idx: LeafIdx) {
        let right = self.leaves[leaf_idx as usize].split();
        let right_weight = right.total_weight;
        let right_count = right.items.len();
        let right_idx = self.alloc_leaf();
        self.leaves[right_idx as usize] = right;

        if self.height == 0 {
            // Root is a leaf, need to create a new root node
            let new_root = self.alloc_node();
            let left_weight = self.leaves[leaf_idx as usize].total_weight;
            let left_count = self.leaves[leaf_idx as usize].len();
            
            self.nodes[new_root as usize].children.push(leaf_idx);
            self.nodes[new_root as usize].children.push(right_idx);
            self.nodes[new_root as usize].child_weights.push(left_weight);
            self.nodes[new_root as usize].child_weights.push(right_weight);
            self.nodes[new_root as usize].child_counts.push(left_count);
            self.nodes[new_root as usize].child_counts.push(right_count);
            self.nodes[new_root as usize].total_weight = left_weight + right_weight;
            self.nodes[new_root as usize].total_count = left_count + right_count;
            
            self.leaves[leaf_idx as usize].parent = new_root;
            self.leaves[leaf_idx as usize].index_in_parent = 0;
            self.leaves[right_idx as usize].parent = new_root;
            self.leaves[right_idx as usize].index_in_parent = 1;
            
            self.root = new_root;
            self.height = 1;
        } else {
            // Insert right leaf into parent
            let parent = self.leaves[leaf_idx as usize].parent;
            let idx_in_parent = self.leaves[leaf_idx as usize].index_in_parent as usize;
            
            // Update left leaf's weight and count in parent
            let left_weight = self.leaves[leaf_idx as usize].total_weight;
            let left_count = self.leaves[leaf_idx as usize].len();
            self.nodes[parent as usize].child_weights[idx_in_parent] = left_weight;
            self.nodes[parent as usize].child_counts[idx_in_parent] = left_count;
            
            // Insert right leaf
            self.nodes[parent as usize].children.insert(idx_in_parent + 1, right_idx);
            self.nodes[parent as usize].child_weights.insert(idx_in_parent + 1, right_weight);
            self.nodes[parent as usize].child_counts.insert(idx_in_parent + 1, right_count);
            
            // Update indices for siblings after the insertion
            for i in (idx_in_parent + 2)..self.nodes[parent as usize].children.len() {
                let child_idx = self.nodes[parent as usize].children[i];
                self.leaves[child_idx as usize].index_in_parent = i as u8;
            }
            
            self.leaves[right_idx as usize].parent = parent;
            self.leaves[right_idx as usize].index_in_parent = (idx_in_parent + 1) as u8;
            
            // Check if parent needs to split
            if self.nodes[parent as usize].is_full() {
                self.split_node(parent, 1);
            }
        }
    }

    /// Split a full internal node at the given height.
    fn split_node(&mut self, node_idx: NodeIdx, height: usize) {
        // Split the node - child_counts are already handled by Node::split()
        let right = self.nodes[node_idx as usize].split();
        let right_weight = right.total_weight;
        let right_count = right.total_count;
        let left_count = self.nodes[node_idx as usize].total_count;
        
        let right_idx = self.alloc_node();
        self.nodes[right_idx as usize] = right;

        // Update parent pointers for children in the right node
        // Collect children first to avoid borrow issues
        let right_children: Vec<u32> = self.nodes[right_idx as usize].children.clone();
        if height == 1 {
            for (i, &child_idx) in right_children.iter().enumerate() {
                self.leaves[child_idx as usize].parent = right_idx;
                self.leaves[child_idx as usize].index_in_parent = i as u8;
            }
        } else {
            for (i, &child_idx) in right_children.iter().enumerate() {
                self.nodes[child_idx as usize].parent = right_idx;
                self.nodes[child_idx as usize].index_in_parent = i as u8;
            }
        }

        if self.nodes[node_idx as usize].parent == NONE {
            // This is the root, create new root
            let new_root = self.alloc_node();
            let left_weight = self.nodes[node_idx as usize].total_weight;
            
            self.nodes[new_root as usize].children.push(node_idx);
            self.nodes[new_root as usize].children.push(right_idx);
            self.nodes[new_root as usize].child_weights.push(left_weight);
            self.nodes[new_root as usize].child_weights.push(right_weight);
            self.nodes[new_root as usize].child_counts.push(left_count);
            self.nodes[new_root as usize].child_counts.push(right_count);
            self.nodes[new_root as usize].total_weight = left_weight + right_weight;
            self.nodes[new_root as usize].total_count = left_count + right_count;
            
            self.nodes[node_idx as usize].parent = new_root;
            self.nodes[node_idx as usize].index_in_parent = 0;
            self.nodes[right_idx as usize].parent = new_root;
            self.nodes[right_idx as usize].index_in_parent = 1;
            
            self.root = new_root;
            self.height += 1;
        } else {
            // Insert right node into parent
            let parent = self.nodes[node_idx as usize].parent;
            let idx_in_parent = self.nodes[node_idx as usize].index_in_parent as usize;
            
            // Update left node's weight and count in parent
            let left_weight = self.nodes[node_idx as usize].total_weight;
            self.nodes[parent as usize].child_weights[idx_in_parent] = left_weight;
            self.nodes[parent as usize].child_counts[idx_in_parent] = left_count;
            
            // Insert right node
            self.nodes[parent as usize].children.insert(idx_in_parent + 1, right_idx);
            self.nodes[parent as usize].child_weights.insert(idx_in_parent + 1, right_weight);
            self.nodes[parent as usize].child_counts.insert(idx_in_parent + 1, right_count);
            
            // Update indices for siblings after the insertion
            for i in (idx_in_parent + 2)..self.nodes[parent as usize].children.len() {
                let child_idx = self.nodes[parent as usize].children[i];
                self.nodes[child_idx as usize].index_in_parent = i as u8;
            }
            
            self.nodes[right_idx as usize].parent = parent;
            self.nodes[right_idx as usize].index_in_parent = (idx_in_parent + 1) as u8;
            
            // Check if parent needs to split
            if self.nodes[parent as usize].is_full() {
                self.split_node(parent, height + 1);
            }
        }
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.len {
            return None;
        }
        let (leaf_idx, idx_in_leaf) = self.find_leaf_by_index(index);
        return self.leaves[leaf_idx as usize].items.get(idx_in_leaf).map(|(item, _)| item);
    }

    #[inline]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index >= self.len {
            return None;
        }
        let (leaf_idx, idx_in_leaf) = self.find_leaf_by_index(index);
        return self.leaves[leaf_idx as usize].items.get_mut(idx_in_leaf).map(|(item, _)| item);
    }

    /// Get a reference to an item using cached leaf location.
    #[inline]
    pub fn get_with_chunk_hint(&self, leaf_idx: usize, idx_in_leaf: usize) -> Option<&T> {
        return self.leaves.get(leaf_idx)?.items.get(idx_in_leaf).map(|(item, _)| item);
    }

    /// Update the weight of an item at the given index.
    pub fn update_weight(&mut self, index: usize, new_weight: u64) -> u64 {
        let (leaf_idx, idx_in_leaf) = self.find_leaf_by_index(index);
        let leaf = &mut self.leaves[leaf_idx as usize];
        let old_weight = leaf.items[idx_in_leaf].1;
        leaf.items[idx_in_leaf].1 = new_weight;
        leaf.total_weight = leaf.total_weight - old_weight + new_weight;
        self.total_weight = self.total_weight - old_weight + new_weight;

        // Update ancestor weights
        if self.height > 0 {
            let delta = new_weight as i64 - old_weight as i64;
            self.update_ancestor_weights(leaf_idx, delta);
        }

        return old_weight;
    }

    /// Modify an item and update its weight in a single operation.
    pub fn modify_and_update_weight<F>(&mut self, index: usize, f: F) -> Option<u64>
    where
        F: FnOnce(&mut T) -> u64,
    {
        if index >= self.len {
            return None;
        }
        let (leaf_idx, idx_in_leaf) = self.find_leaf_by_index(index);
        let leaf = &mut self.leaves[leaf_idx as usize];
        let (item, old_weight) = &mut leaf.items[idx_in_leaf];
        let old = *old_weight;
        
        let new_weight = f(item);
        
        *old_weight = new_weight;
        leaf.total_weight = leaf.total_weight - old + new_weight;
        self.total_weight = self.total_weight - old + new_weight;
        
        // Update ancestor weights
        if self.height > 0 {
            let delta = new_weight as i64 - old as i64;
            self.update_ancestor_weights(leaf_idx, delta);
        }
        
        return Some(new_weight);
    }

    /// Modify an item and update its weight using cached leaf location.
    #[inline]
    pub fn modify_and_update_weight_with_hint<F>(
        &mut self,
        leaf_idx: usize,
        idx_in_leaf: usize,
        f: F,
    ) -> Option<(u64, usize, usize)>
    where
        F: FnOnce(&mut T) -> u64,
    {
        let leaf = self.leaves.get_mut(leaf_idx)?;
        let (item, old_weight) = leaf.items.get_mut(idx_in_leaf)?;
        let old = *old_weight;
        
        let new_weight = f(item);
        
        *old_weight = new_weight;
        leaf.total_weight = leaf.total_weight - old + new_weight;
        self.total_weight = self.total_weight - old + new_weight;
        
        // Update ancestor weights
        if self.height > 0 {
            let delta = new_weight as i64 - old as i64;
            self.update_ancestor_weights(leaf_idx as LeafIdx, delta);
        }
        
        return Some((new_weight, leaf_idx, idx_in_leaf));
    }

    /// Remove an item at the given index.
    pub fn remove(&mut self, index: usize) -> T {
        let (leaf_idx, idx_in_leaf) = self.find_leaf_by_index(index);
        
        let leaf = &mut self.leaves[leaf_idx as usize];
        let (item, weight) = leaf.items.remove(idx_in_leaf);
        leaf.total_weight -= weight;
        
        self.total_weight -= weight;
        self.len -= 1;

        // Update ancestor weights and counts in a single traversal
        if self.height > 0 {
            self.update_ancestors(leaf_idx, -(weight as i64), -1);
        }

        // For simplicity, we don't merge underflowed nodes in this implementation.
        // The tree still works correctly, just with some wasted space.
        // This is acceptable for our use case where removes are rare compared to inserts.

        return item;
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        // Collect leaf indices in document order by traversing the tree
        let leaf_order = self.collect_leaves_in_order();
        return BTreeListIter {
            tree: self,
            leaf_order,
            leaf_pos: 0,
            item_idx: 0,
        };
    }

    /// Collect leaf indices in document order by traversing the tree.
    fn collect_leaves_in_order(&self) -> Vec<LeafIdx> {
        let mut result = Vec::new();
        if self.height == 0 {
            result.push(self.root);
        } else {
            self.collect_leaves_recursive(self.root, self.height, &mut result);
        }
        return result;
    }

    /// Recursively collect leaf indices from a subtree.
    fn collect_leaves_recursive(&self, node_idx: NodeIdx, height: usize, result: &mut Vec<LeafIdx>) {
        let node = &self.nodes[node_idx as usize];
        if height == 1 {
            // Children are leaves
            for &child_idx in &node.children {
                result.push(child_idx);
            }
        } else {
            // Children are internal nodes
            for &child_idx in &node.children {
                self.collect_leaves_recursive(child_idx, height - 1, result);
            }
        }
    }
}

impl<T: Clone> Default for BTreeList<T> {
    fn default() -> Self {
        return Self::new();
    }
}

/// Iterator over BTreeList items.
struct BTreeListIter<'a, T> {
    tree: &'a BTreeList<T>,
    /// Leaf indices in document order.
    leaf_order: Vec<LeafIdx>,
    /// Current position in leaf_order.
    leaf_pos: usize,
    /// Current position within the leaf.
    item_idx: usize,
}

impl<'a, T: Clone> Iterator for BTreeListIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        // Find the next valid item
        while self.leaf_pos < self.leaf_order.len() {
            let leaf_idx = self.leaf_order[self.leaf_pos] as usize;
            let leaf = &self.tree.leaves[leaf_idx];
            if self.item_idx < leaf.items.len() {
                let item = &leaf.items[self.item_idx].0;
                self.item_idx += 1;
                return Some(item);
            }
            self.leaf_pos += 1;
            self.item_idx = 0;
        }
        return None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_list() {
        let list: BTreeList<u32> = BTreeList::new();
        assert_eq!(list.len(), 0);
        assert_eq!(list.total_weight(), 0);
        assert!(list.is_empty());
    }

    #[test]
    fn insert_single() {
        let mut list = BTreeList::new();
        list.insert(0, "hello", 5);

        assert_eq!(list.len(), 1);
        assert_eq!(list.total_weight(), 5);
        assert_eq!(list.get(0), Some(&"hello"));
    }

    #[test]
    fn insert_multiple() {
        let mut list = BTreeList::new();
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
        let mut list = BTreeList::new();
        list.insert(0, "a", 5);
        list.insert(1, "c", 5);
        list.insert(1, "b", 5);

        assert_eq!(list.get(0), Some(&"a"));
        assert_eq!(list.get(1), Some(&"b"));
        assert_eq!(list.get(2), Some(&"c"));
    }

    #[test]
    fn find_by_weight_single() {
        let mut list = BTreeList::new();
        list.insert(0, "hello", 10);

        assert_eq!(list.find_by_weight(0), Some((0, 0)));
        assert_eq!(list.find_by_weight(5), Some((0, 5)));
        assert_eq!(list.find_by_weight(9), Some((0, 9)));
        assert_eq!(list.find_by_weight(10), None);
    }

    #[test]
    fn find_by_weight_multiple() {
        let mut list = BTreeList::new();
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
        let mut list = BTreeList::new();
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
        let mut list = BTreeList::new();
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
    fn remove_item() {
        let mut list = BTreeList::new();
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
        let mut list = BTreeList::new();
        list.insert(0, 1u32, 5);
        list.insert(1, 2u32, 10);
        list.insert(2, 3u32, 3);

        let items: Vec<_> = list.iter().cloned().collect();
        assert_eq!(items, vec![1, 2, 3]);
    }

    #[test]
    fn many_inserts() {
        let mut list = BTreeList::new();
        for i in 0..1000 {
            list.insert(i, i, i as u64 + 1);
        }

        assert_eq!(list.len(), 1000);
        // Total weight = 1 + 2 + ... + 1000 = 1000 * 1001 / 2 = 500500
        assert_eq!(list.total_weight(), 500500);

        // Verify some items
        assert_eq!(list.get(0), Some(&0));
        assert_eq!(list.get(500), Some(&500));
        assert_eq!(list.get(999), Some(&999));
    }

    #[test]
    fn many_inserts_at_beginning() {
        let mut list = BTreeList::new();
        for i in 0..1000 {
            list.insert(0, i, 1);
        }

        assert_eq!(list.len(), 1000);
        assert_eq!(list.total_weight(), 1000);

        // Items should be in reverse order
        assert_eq!(list.get(0), Some(&999));
        assert_eq!(list.get(999), Some(&0));
    }

    #[test]
    fn triggers_splits() {
        let mut list = BTreeList::new();
        // Insert enough to trigger multiple splits
        for i in 0..500 {
            list.insert(i, i, 1);
        }

        assert_eq!(list.len(), 500);
        assert!(list.height > 0, "should have non-zero height");

        // Verify all items are accessible
        for i in 0..500 {
            assert_eq!(list.get(i), Some(&i));
        }

        // Verify find_by_weight works
        for i in 0..500 {
            let result = list.find_by_weight(i as u64);
            assert_eq!(result, Some((i, 0)));
        }
    }

    #[test]
    fn modify_and_update_weight() {
        let mut list = BTreeList::new();
        list.insert(0, 10u32, 5);
        list.insert(1, 20u32, 10);

        let result = list.modify_and_update_weight(0, |item| {
            *item += 5;
            return 8;
        });

        assert_eq!(result, Some(8));
        assert_eq!(list.get(0), Some(&15));
        assert_eq!(list.total_weight(), 18);
    }

    #[test]
    fn find_by_weight_with_chunk() {
        let mut list = BTreeList::new();
        list.insert(0, "a", 5);
        list.insert(1, "b", 10);

        let result = list.find_by_weight_with_chunk(7);
        assert!(result.is_some());
        let (item_idx, offset, leaf_idx, idx_in_leaf) = result.unwrap();
        assert_eq!(item_idx, 1);
        assert_eq!(offset, 2);
        
        // Verify hint works
        assert_eq!(list.get_with_chunk_hint(leaf_idx, idx_in_leaf), Some(&"b"));
    }
}
