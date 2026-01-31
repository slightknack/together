// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-01-31"
// driver = "Isaac Clayton"

//! Chunked Weighted List with Fenwick Tree
//!
//! A weighted list organized into chunks with O(log n) weight lookup via Fenwick tree.
//! Each chunk stores items with their weights. Chunks are sized around sqrt(n).
//!
//! The Fenwick tree (Binary Indexed Tree) maintains prefix sums of chunk weights,
//! enabling O(log n) chunk lookup by weight position.
//!
//! For n=20k items with sqrt(n)=140 chunk size:
//! - find_by_weight: O(log chunks) to find chunk + O(sqrt(n)) within chunk
//! - insert: O(log chunks) for weight update + O(sqrt(n)) within chunk
//! - remove: O(log chunks) for weight update + O(sqrt(n)) within chunk

const TARGET_CHUNK_SIZE: usize = 64;
const MAX_CHUNK_SIZE: usize = 128;

/// Threshold for switching from linear scan to Fenwick tree.
/// For small chunk counts, O(n) linear scan with good constants beats O(log n) with overhead.
/// With TARGET_CHUNK_SIZE=64: sveltecomponent ~92 chunks, rustcode ~196 chunks, seph-blog1 ~243 chunks.
/// Linear scan is competitive up to ~128 chunks due to Fenwick tree overhead.
const FENWICK_THRESHOLD: usize = 128;

/// Fenwick Tree (Binary Indexed Tree) for O(log n) prefix sum queries and updates.
///
/// The tree uses 1-based indexing internally. Each position i stores the sum of
/// elements from (i - lowbit(i) + 1) to i, where lowbit(i) = i & -i.
///
/// This enables:
/// - prefix_sum(i): O(log n) sum of elements 0..=i
/// - update(i, delta): O(log n) add delta to element i
/// - find_first_exceeding(target): O(log n) binary search for first index where prefix_sum > target
#[derive(Clone, Debug)]
struct FenwickTree {
    /// 1-indexed tree array. tree[0] is unused.
    tree: Vec<u64>,
}

impl FenwickTree {
    /// Create a new Fenwick tree with capacity for n elements.
    fn new(n: usize) -> FenwickTree {
        return FenwickTree {
            tree: vec![0; n + 1],
        };
    }

    /// Compute prefix sum of elements 0..=i.
    fn prefix_sum(&self, i: usize) -> u64 {
        let mut idx = i + 1; // Convert to 1-indexed
        let mut sum = 0u64;
        while idx > 0 {
            sum += self.tree[idx];
            idx -= idx & idx.wrapping_neg(); // idx -= lowbit(idx)
        }
        return sum;
    }

    /// Add delta to element at index i.
    fn update(&mut self, i: usize, delta: i64) {
        let mut idx = i + 1; // Convert to 1-indexed
        while idx < self.tree.len() {
            self.tree[idx] = (self.tree[idx] as i64 + delta) as u64;
            idx += idx & idx.wrapping_neg(); // idx += lowbit(idx)
        }
    }

    /// Find the first index where prefix_sum(index) > target.
    /// Returns (index, prefix_sum_before_index).
    /// If no such index exists, returns (n, total_sum).
    ///
    /// This uses a more efficient O(log n) algorithm that descends the tree
    /// directly rather than binary searching with prefix_sum calls.
    /// By returning the prefix sum, we avoid a second O(log n) query.
    fn find_first_exceeding_with_sum(&self, target: u64) -> (usize, u64) {
        if self.tree.len() <= 1 {
            return (0, 0);
        }

        let n = self.tree.len() - 1;
        let mut sum = 0u64;
        let mut pos = 0usize;

        // Find the largest power of 2 <= n
        let mut bit = 1usize;
        while bit * 2 <= n {
            bit *= 2;
        }

        // Descend the tree
        while bit > 0 {
            let next = pos + bit;
            if next <= n && sum + self.tree[next] <= target {
                sum += self.tree[next];
                pos = next;
            }
            bit /= 2;
        }

        // pos is now the largest index where prefix_sum(pos) <= target
        // sum is the prefix_sum at pos (which is the sum BEFORE the result index)
        // The result index is pos (0-indexed)
        return (pos, sum);
    }

    /// Build a Fenwick tree from an iterator of values.
    fn from_iter<I: Iterator<Item = u64>>(iter: I) -> FenwickTree {
        let values: Vec<u64> = iter.collect();
        let n = values.len();
        let mut tree = vec![0u64; n + 1];

        // Copy values to 1-indexed positions
        for (i, &v) in values.iter().enumerate() {
            tree[i + 1] = v;
        }

        // Build tree in O(n) by propagating values upward
        for i in 1..=n {
            let parent = i + (i & i.wrapping_neg());
            if parent <= n {
                tree[parent] += tree[i];
            }
        }

        return FenwickTree { tree };
    }
}

/// A chunk of items with their weights.
struct Chunk<T> {
    items: Vec<(T, u64)>,
    total_weight: u64,
}

impl<T> Chunk<T> {
    fn new() -> Self {
        Chunk {
            items: Vec::with_capacity(TARGET_CHUNK_SIZE),
            total_weight: 0,
        }
    }

    fn len(&self) -> usize {
        self.items.len()
    }

    fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn should_split(&self) -> bool {
        self.items.len() >= MAX_CHUNK_SIZE
    }

    fn insert(&mut self, index: usize, item: T, weight: u64) {
        self.items.insert(index, (item, weight));
        self.total_weight += weight;
    }

    fn remove(&mut self, index: usize) -> (T, u64) {
        let (item, weight) = self.items.remove(index);
        self.total_weight -= weight;
        (item, weight)
    }

    fn split(&mut self) -> Chunk<T> {
        let mid = self.items.len() / 2;
        let right_items: Vec<_> = self.items.drain(mid..).collect();
        let right_weight: u64 = right_items.iter().map(|(_, w)| *w).sum();
        self.total_weight -= right_weight;
        Chunk {
            items: right_items,
            total_weight: right_weight,
        }
    }

    /// Find item by weight within this chunk.
    fn find_by_weight(&self, pos: u64) -> Option<(usize, u64)> {
        let mut cumulative = 0u64;
        for (i, (_, weight)) in self.items.iter().enumerate() {
            if cumulative + weight > pos {
                return Some((i, pos - cumulative));
            }
            cumulative += weight;
        }
        None
    }

    fn get(&self, index: usize) -> Option<&T> {
        self.items.get(index).map(|(item, _)| item)
    }

    fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.items.get_mut(index).map(|(item, _)| item)
    }

    fn update_weight(&mut self, index: usize, new_weight: u64) -> u64 {
        let old_weight = self.items[index].1;
        self.items[index].1 = new_weight;
        self.total_weight = self.total_weight - old_weight + new_weight;
        old_weight
    }
}

/// A weighted list organized into chunks with optional Fenwick trees for O(log n) lookups.
/// Uses linear scan for small chunk counts (< FENWICK_THRESHOLD) and Fenwick tree for larger counts.
pub struct WeightedList<T> {
    chunks: Vec<Chunk<T>>,
    /// Fenwick tree tracking chunk weights for O(log n) weight prefix sum queries.
    /// Only valid when `use_fenwick` is true.
    chunk_weights: FenwickTree,
    /// Fenwick tree tracking chunk item counts for O(log n) index prefix sum queries.
    /// Only valid when `use_fenwick` is true.
    chunk_counts: FenwickTree,
    /// Whether to use Fenwick trees for lookups.
    /// False for small chunk counts where linear scan is faster.
    use_fenwick: bool,
    total_weight: u64,
    len: usize,
}

impl<T> WeightedList<T> {
    pub fn new() -> Self {
        WeightedList {
            chunks: vec![Chunk::new()],
            chunk_weights: FenwickTree::new(1),
            chunk_counts: FenwickTree::new(1),
            use_fenwick: false, // Start with linear scan, only 1 chunk
            total_weight: 0,
            len: 0,
        }
    }

    /// Rebuild the Fenwick trees from current chunk state.
    /// Called when chunks are added or removed.
    fn rebuild_fenwick(&mut self) {
        self.chunk_weights = FenwickTree::from_iter(
            self.chunks.iter().map(|c| c.total_weight)
        );
        self.chunk_counts = FenwickTree::from_iter(
            self.chunks.iter().map(|c| c.len() as u64)
        );
    }

    pub fn total_weight(&self) -> u64 {
        self.total_weight
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Find the chunk containing the given weight position.
    /// Returns (chunk_index, weight_offset_in_chunk, items_before_chunk).
    /// Uses linear scan for small chunk counts, Fenwick tree for larger counts.
    fn find_chunk_by_weight(&self, pos: u64) -> Option<(usize, u64, usize)> {
        if pos >= self.total_weight {
            return None;
        }

        if self.use_fenwick {
            // Use Fenwick tree to find the chunk in O(log n)
            // This returns both the chunk index and the weight before it in one traversal
            let (chunk_idx, weight_before) = self.chunk_weights.find_first_exceeding_with_sum(pos);

            // Compute items before this chunk using Fenwick tree in O(log n)
            let items_before = if chunk_idx == 0 {
                0
            } else {
                self.chunk_counts.prefix_sum(chunk_idx - 1) as usize
            };

            return Some((chunk_idx, pos - weight_before, items_before));
        } else {
            // Linear scan for small chunk counts - O(n) but with good constants
            let mut weight_before = 0u64;
            let mut items_before = 0usize;
            for (idx, chunk) in self.chunks.iter().enumerate() {
                if weight_before + chunk.total_weight > pos {
                    return Some((idx, pos - weight_before, items_before));
                }
                weight_before += chunk.total_weight;
                items_before += chunk.len();
            }
            return None;
        }
    }

    /// Find the chunk containing the given index.
    /// Returns (chunk_index, index_within_chunk).
    /// Uses linear scan for small chunk counts, Fenwick tree for larger counts.
    fn find_chunk_by_index(&self, index: usize) -> (usize, usize) {
        if index >= self.len {
            // Insert at end
            let last = self.chunks.len().saturating_sub(1);
            return (last, self.chunks.get(last).map_or(0, |c| c.len()));
        }

        if self.use_fenwick {
            // Use Fenwick tree to find the chunk in O(log n)
            // This returns both the chunk index and the items before it in one traversal
            let (chunk_idx, items_before) = self.chunk_counts.find_first_exceeding_with_sum(index as u64);

            return (chunk_idx, index - items_before as usize);
        } else {
            // Linear scan for small chunk counts - O(n) but with good constants
            let mut items_before = 0usize;
            for (idx, chunk) in self.chunks.iter().enumerate() {
                if items_before + chunk.len() > index {
                    return (idx, index - items_before);
                }
                items_before += chunk.len();
            }
            // Should not reach here if index < self.len
            let last = self.chunks.len().saturating_sub(1);
            return (last, self.chunks.get(last).map_or(0, |c| c.len()));
        }
    }

    /// Find the item containing the given weight position.
    /// Returns (item_index, offset_within_item) or None if pos >= total_weight.
    pub fn find_by_weight(&self, pos: u64) -> Option<(usize, u64)> {
        if pos >= self.total_weight {
            return None;
        }

        let (chunk_idx, offset_in_chunk, items_before) = self.find_chunk_by_weight(pos)?;
        let chunk = &self.chunks[chunk_idx];
        let (idx_in_chunk, offset_in_item) = chunk.find_by_weight(offset_in_chunk)?;

        Some((items_before + idx_in_chunk, offset_in_item))
    }

    /// Insert an item at the given index with the given weight.
    pub fn insert(&mut self, index: usize, item: T, weight: u64) {
        if self.chunks.is_empty() {
            self.chunks.push(Chunk::new());
            if self.use_fenwick {
                self.rebuild_fenwick();
            }
        }

        let (chunk_idx, idx_in_chunk) = self.find_chunk_by_index(index);
        self.chunks[chunk_idx].insert(idx_in_chunk, item, weight);
        self.total_weight += weight;
        self.len += 1;

        // Update Fenwick trees with the weight and count changes (only if using Fenwick)
        if self.use_fenwick {
            self.chunk_weights.update(chunk_idx, weight as i64);
            self.chunk_counts.update(chunk_idx, 1);
        }

        // Split chunk if too large
        if self.chunks[chunk_idx].should_split() {
            let new_chunk = self.chunks[chunk_idx].split();
            self.chunks.insert(chunk_idx + 1, new_chunk);

            // Check if we should switch to Fenwick mode
            if !self.use_fenwick && self.chunks.len() >= FENWICK_THRESHOLD {
                self.use_fenwick = true;
                self.rebuild_fenwick();
            } else if self.use_fenwick {
                // Rebuild Fenwick trees after structural change
                self.rebuild_fenwick();
            }
        }
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.len {
            return None;
        }
        let (chunk_idx, idx_in_chunk) = self.find_chunk_by_index(index);
        self.chunks[chunk_idx].get(idx_in_chunk)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index >= self.len {
            return None;
        }
        let (chunk_idx, idx_in_chunk) = self.find_chunk_by_index(index);
        self.chunks[chunk_idx].get_mut(idx_in_chunk)
    }

    /// Get a mutable reference and update the weight in a single operation.
    /// This avoids the overhead of two separate chunk lookups.
    /// The callback receives a mutable reference to the item and should return the new weight.
    /// Returns the new weight on success, None if index is out of bounds.
    pub fn modify_and_update_weight<F>(&mut self, index: usize, f: F) -> Option<u64>
    where
        F: FnOnce(&mut T) -> u64,
    {
        if index >= self.len {
            return None;
        }
        let (chunk_idx, idx_in_chunk) = self.find_chunk_by_index(index);
        let chunk = &mut self.chunks[chunk_idx];
        let (item, old_weight) = &mut chunk.items[idx_in_chunk];
        let old = *old_weight;
        
        // Call the callback to modify the item and get the new weight
        let new_weight = f(item);
        
        // Update weights
        *old_weight = new_weight;
        chunk.total_weight = chunk.total_weight - old + new_weight;
        self.total_weight = self.total_weight - old + new_weight;
        
        // Update Fenwick tree with the weight delta (only if using Fenwick)
        if self.use_fenwick {
            let delta = new_weight as i64 - old as i64;
            self.chunk_weights.update(chunk_idx, delta);
        }
        
        return Some(new_weight);
    }

    /// Update the weight of an item at the given index.
    pub fn update_weight(&mut self, index: usize, new_weight: u64) -> u64 {
        let (chunk_idx, idx_in_chunk) = self.find_chunk_by_index(index);
        let old_weight = self.chunks[chunk_idx].update_weight(idx_in_chunk, new_weight);
        self.total_weight = self.total_weight - old_weight + new_weight;

        // Update Fenwick tree with the weight delta (only if using Fenwick)
        if self.use_fenwick {
            let delta = new_weight as i64 - old_weight as i64;
            self.chunk_weights.update(chunk_idx, delta);
        }

        return old_weight;
    }

    /// Remove an item at the given index.
    pub fn remove(&mut self, index: usize) -> T {
        let (chunk_idx, idx_in_chunk) = self.find_chunk_by_index(index);
        let (item, weight) = self.chunks[chunk_idx].remove(idx_in_chunk);
        self.total_weight -= weight;
        self.len -= 1;

        // Update Fenwick trees with negative weight and count (only if using Fenwick)
        if self.use_fenwick {
            self.chunk_weights.update(chunk_idx, -(weight as i64));
            self.chunk_counts.update(chunk_idx, -1);
        }

        // Remove empty chunks (but keep at least one)
        if self.chunks[chunk_idx].is_empty() && self.chunks.len() > 1 {
            self.chunks.remove(chunk_idx);

            // Check if we should switch back to linear scan mode
            if self.use_fenwick && self.chunks.len() < FENWICK_THRESHOLD {
                self.use_fenwick = false;
                // No need to rebuild Fenwick trees when switching to linear scan
            } else if self.use_fenwick {
                // Rebuild Fenwick trees after structural change
                self.rebuild_fenwick();
            }
        }

        return item;
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.chunks.iter().flat_map(|c| c.items.iter().map(|(item, _)| item))
    }
}

impl<T> Default for WeightedList<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- FenwickTree tests ---

    #[test]
    fn fenwick_empty() {
        let tree = FenwickTree::new(0);
        assert_eq!(tree.tree.len(), 1); // Only the unused index 0
    }

    #[test]
    fn fenwick_single_element() {
        let mut tree = FenwickTree::new(1);
        tree.update(0, 5);
        assert_eq!(tree.prefix_sum(0), 5);
        assert_eq!(tree.find_first_exceeding_with_sum(4), (0, 0));
        assert_eq!(tree.find_first_exceeding_with_sum(5), (1, 5));
    }

    #[test]
    fn fenwick_multiple_elements() {
        let mut tree = FenwickTree::new(5);
        // Set weights: [3, 5, 2, 7, 1]
        tree.update(0, 3);
        tree.update(1, 5);
        tree.update(2, 2);
        tree.update(3, 7);
        tree.update(4, 1);

        // Prefix sums: [3, 8, 10, 17, 18]
        assert_eq!(tree.prefix_sum(0), 3);
        assert_eq!(tree.prefix_sum(1), 8);
        assert_eq!(tree.prefix_sum(2), 10);
        assert_eq!(tree.prefix_sum(3), 17);
        assert_eq!(tree.prefix_sum(4), 18);
    }

    #[test]
    fn fenwick_find_first_exceeding() {
        let mut tree = FenwickTree::new(5);
        // Set weights: [3, 5, 2, 7, 1]
        // Prefix sums: [3, 8, 10, 17, 18]
        tree.update(0, 3);
        tree.update(1, 5);
        tree.update(2, 2);
        tree.update(3, 7);
        tree.update(4, 1);

        // find_first_exceeding_with_sum(target) returns (first i where prefix_sum(i) > target, sum_before_i)
        assert_eq!(tree.find_first_exceeding_with_sum(0).0, 0);   // prefix_sum(0)=3 > 0
        assert_eq!(tree.find_first_exceeding_with_sum(2).0, 0);   // prefix_sum(0)=3 > 2
        assert_eq!(tree.find_first_exceeding_with_sum(3).0, 1);   // prefix_sum(1)=8 > 3
        assert_eq!(tree.find_first_exceeding_with_sum(7).0, 1);   // prefix_sum(1)=8 > 7
        assert_eq!(tree.find_first_exceeding_with_sum(8).0, 2);   // prefix_sum(2)=10 > 8
        assert_eq!(tree.find_first_exceeding_with_sum(9).0, 2);   // prefix_sum(2)=10 > 9
        assert_eq!(tree.find_first_exceeding_with_sum(10).0, 3);  // prefix_sum(3)=17 > 10
        assert_eq!(tree.find_first_exceeding_with_sum(16).0, 3);  // prefix_sum(3)=17 > 16
        assert_eq!(tree.find_first_exceeding_with_sum(17).0, 4);  // prefix_sum(4)=18 > 17
        assert_eq!(tree.find_first_exceeding_with_sum(18).0, 5);  // no index exceeds 18
    }

    #[test]
    fn fenwick_update_negative() {
        let mut tree = FenwickTree::new(3);
        tree.update(0, 10);
        tree.update(1, 5);
        tree.update(2, 3);

        assert_eq!(tree.prefix_sum(2), 18);

        // Decrease middle element by 3
        tree.update(1, -3);
        assert_eq!(tree.prefix_sum(0), 10);
        assert_eq!(tree.prefix_sum(1), 12);
        assert_eq!(tree.prefix_sum(2), 15);
    }

    #[test]
    fn fenwick_from_iter() {
        let tree = FenwickTree::from_iter([3u64, 5, 2, 7, 1].into_iter());

        // Prefix sums: [3, 8, 10, 17, 18]
        assert_eq!(tree.prefix_sum(0), 3);
        assert_eq!(tree.prefix_sum(1), 8);
        assert_eq!(tree.prefix_sum(2), 10);
        assert_eq!(tree.prefix_sum(3), 17);
        assert_eq!(tree.prefix_sum(4), 18);
    }

    #[test]
    fn fenwick_from_iter_large() {
        // Test with 1000 elements
        let values: Vec<u64> = (1..=1000).collect();
        let tree = FenwickTree::from_iter(values.iter().copied());

        // Sum of 1..=n is n*(n+1)/2
        assert_eq!(tree.prefix_sum(0), 1);
        assert_eq!(tree.prefix_sum(99), 5050);  // 100*101/2
        assert_eq!(tree.prefix_sum(999), 500500);  // 1000*1001/2
    }

    #[test]
    fn fenwick_find_with_zeros() {
        // Test with some zero-weight elements
        let tree = FenwickTree::from_iter([5u64, 0, 0, 3, 0, 2].into_iter());

        // Prefix sums: [5, 5, 5, 8, 8, 10]
        assert_eq!(tree.prefix_sum(0), 5);
        assert_eq!(tree.prefix_sum(1), 5);
        assert_eq!(tree.prefix_sum(2), 5);
        assert_eq!(tree.prefix_sum(3), 8);
        assert_eq!(tree.prefix_sum(4), 8);
        assert_eq!(tree.prefix_sum(5), 10);

        // Find should skip zero-weight elements correctly
        assert_eq!(tree.find_first_exceeding_with_sum(4).0, 0);  // prefix_sum(0)=5 > 4
        assert_eq!(tree.find_first_exceeding_with_sum(5).0, 3);  // prefix_sum(3)=8 > 5
        assert_eq!(tree.find_first_exceeding_with_sum(7).0, 3);  // prefix_sum(3)=8 > 7
        assert_eq!(tree.find_first_exceeding_with_sum(8).0, 5);  // prefix_sum(5)=10 > 8
    }

    // --- WeightedList tests ---

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
    fn insert_in_middle() {
        let mut list = WeightedList::new();
        list.insert(0, "a", 5);
        list.insert(1, "c", 5);
        list.insert(1, "b", 5);

        assert_eq!(list.get(0), Some(&"a"));
        assert_eq!(list.get(1), Some(&"b"));
        assert_eq!(list.get(2), Some(&"c"));
    }

    #[test]
    fn find_by_weight_single() {
        let mut list = WeightedList::new();
        list.insert(0, "hello", 10);

        assert_eq!(list.find_by_weight(0), Some((0, 0)));
        assert_eq!(list.find_by_weight(5), Some((0, 5)));
        assert_eq!(list.find_by_weight(9), Some((0, 9)));
        assert_eq!(list.find_by_weight(10), None);
    }

    #[test]
    fn find_by_weight_multiple() {
        let mut list = WeightedList::new();
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
        let mut list = WeightedList::new();
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
        let mut list = WeightedList::new();
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
        let mut list = WeightedList::new();
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
        let mut list = WeightedList::new();
        list.insert(0, 1u32, 5);
        list.insert(1, 2u32, 10);
        list.insert(2, 3u32, 3);

        let items: Vec<_> = list.iter().cloned().collect();
        assert_eq!(items, vec![1, 2, 3]);
    }

    #[test]
    fn remove_and_reinsert() {
        let mut list = WeightedList::new();
        list.insert(0, "a", 5);
        list.insert(1, "b", 10);

        let b = list.remove(1);
        assert_eq!(b, "b");
        assert_eq!(list.total_weight(), 5);

        list.insert(1, "c", 3);
        assert_eq!(list.total_weight(), 8);
        assert_eq!(list.get(1), Some(&"c"));

        assert_eq!(list.find_by_weight(4), Some((0, 4)));
        assert_eq!(list.find_by_weight(5), Some((1, 0)));
        assert_eq!(list.find_by_weight(7), Some((1, 2)));
        assert_eq!(list.find_by_weight(8), None);
    }

    #[test]
    fn many_inserts() {
        let mut list = WeightedList::new();
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
        let mut list = WeightedList::new();
        for i in 0..10 {
            list.insert(0, i, 1);
        }

        assert_eq!(list.len(), 10);
        let items: Vec<_> = list.iter().cloned().collect();
        // Inserting at 0 each time reverses the order
        assert_eq!(items, (0..10).rev().collect::<Vec<_>>());
    }

    #[test]
    fn triggers_split() {
        let mut list = WeightedList::new();
        for i in 0..300 {
            list.insert(i, i, 1);
        }

        assert_eq!(list.len(), 300);
        assert!(list.chunks.len() > 1, "should have multiple chunks");

        // Verify all items are accessible
        for i in 0..300 {
            assert_eq!(list.get(i), Some(&i));
        }
    }
}
