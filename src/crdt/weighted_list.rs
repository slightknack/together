// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-01-31"
// driver = "Isaac Clayton"

//! Chunked Weighted List
//!
//! A weighted list organized into chunks for O(sqrt(n)) operations.
//! Each chunk stores items with their weights. Chunks are sized around sqrt(n)
//! and we maintain cumulative weights per chunk for O(sqrt(n)) lookups.
//!
//! For n=20k items with sqrt(n)=140 chunk size:
//! - find_by_weight: O(sqrt(n)) to find chunk + O(sqrt(n)) within chunk
//! - insert: O(sqrt(n)) to find chunk + O(sqrt(n)) within chunk
//! - remove: O(sqrt(n)) to find chunk + O(sqrt(n)) within chunk

const TARGET_CHUNK_SIZE: usize = 64;
const MAX_CHUNK_SIZE: usize = 128;

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

/// A weighted list organized into chunks.
pub struct WeightedList<T> {
    chunks: Vec<Chunk<T>>,
    total_weight: u64,
    len: usize,
}

impl<T> WeightedList<T> {
    pub fn new() -> Self {
        WeightedList {
            chunks: vec![Chunk::new()],
            total_weight: 0,
            len: 0,
        }
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
    fn find_chunk_by_weight(&self, pos: u64) -> Option<(usize, u64, usize)> {
        let mut cumulative_weight = 0u64;
        let mut cumulative_items = 0usize;
        for (i, chunk) in self.chunks.iter().enumerate() {
            if cumulative_weight + chunk.total_weight > pos {
                return Some((i, pos - cumulative_weight, cumulative_items));
            }
            cumulative_weight += chunk.total_weight;
            cumulative_items += chunk.len();
        }
        None
    }

    /// Find the chunk containing the given index.
    /// Returns (chunk_index, index_within_chunk).
    fn find_chunk_by_index(&self, index: usize) -> (usize, usize) {
        let mut cumulative = 0usize;
        for (i, chunk) in self.chunks.iter().enumerate() {
            if cumulative + chunk.len() > index {
                return (i, index - cumulative);
            }
            cumulative += chunk.len();
        }
        // Insert at end
        let last = self.chunks.len().saturating_sub(1);
        (last, self.chunks.get(last).map_or(0, |c| c.len()))
    }

    /// Find the item containing the given weight position.
    /// Returns (item_index, offset_within_item) or None if pos >= total_weight.
    pub fn find_by_weight(&mut self, pos: u64) -> Option<(usize, u64)> {
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
        }

        let (chunk_idx, idx_in_chunk) = self.find_chunk_by_index(index);
        self.chunks[chunk_idx].insert(idx_in_chunk, item, weight);
        self.total_weight += weight;
        self.len += 1;

        // Split chunk if too large
        if self.chunks[chunk_idx].should_split() {
            let new_chunk = self.chunks[chunk_idx].split();
            self.chunks.insert(chunk_idx + 1, new_chunk);
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

    /// Update the weight of an item at the given index.
    pub fn update_weight(&mut self, index: usize, new_weight: u64) -> u64 {
        let (chunk_idx, idx_in_chunk) = self.find_chunk_by_index(index);
        let old_weight = self.chunks[chunk_idx].update_weight(idx_in_chunk, new_weight);
        self.total_weight = self.total_weight - old_weight + new_weight;
        old_weight
    }

    /// Remove an item at the given index.
    pub fn remove(&mut self, index: usize) -> T {
        let (chunk_idx, idx_in_chunk) = self.find_chunk_by_index(index);
        let (item, weight) = self.chunks[chunk_idx].remove(idx_in_chunk);
        self.total_weight -= weight;
        self.len -= 1;

        // Remove empty chunks (but keep at least one)
        if self.chunks[chunk_idx].is_empty() && self.chunks.len() > 1 {
            self.chunks.remove(chunk_idx);
        }

        item
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
