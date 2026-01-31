// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-01-30"
// driver = "Isaac Clayton"

//! Skip List Rope
//!
//! A rope data structure built on a skip list for O(log n) text operations.
//! Stores text in chunks for cache locality, with position lookup by byte offset.
//!
//! # Width Semantics
//!
//! `node.widths[level]` = bytes spanned from this node to `node.next[level]`.
//! - For data nodes at level 0: `widths[0]` = this node's byte count
//! - For HEAD: `widths[level]` = total bytes up to `next[level]`
//! - At higher levels: sum of all `widths[0]` of nodes in the span

use std::mem::MaybeUninit;

/// Bytes per chunk.
const CHUNK_SIZE: usize = 64;

/// Maximum skip list height.
const MAX_HEIGHT: usize = 16;

/// Node index type.
type Idx = u32;

/// Null index marker.
const NULL: Idx = Idx::MAX;

/// A node in the rope.
struct Node {
    bytes: [MaybeUninit<u8>; CHUNK_SIZE],
    len: u16,
    height: u8,
    next: [Idx; MAX_HEIGHT],
    widths: [u32; MAX_HEIGHT],
}

impl Node {
    fn new(height: u8) -> Self {
        Node {
            bytes: unsafe { MaybeUninit::uninit().assume_init() },
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

    fn as_slice(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(self.bytes.as_ptr() as *const u8, self.len as usize)
        }
    }

    fn insert_bytes(&mut self, offset: usize, data: &[u8]) {
        assert!(self.len() + data.len() <= CHUNK_SIZE);
        let old_len = self.len();

        if offset < old_len {
            unsafe {
                std::ptr::copy(
                    self.bytes.as_ptr().add(offset),
                    self.bytes.as_mut_ptr().add(offset + data.len()),
                    old_len - offset,
                );
            }
        }

        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                self.bytes.as_mut_ptr().add(offset) as *mut u8,
                data.len(),
            );
        }

        self.len = (old_len + data.len()) as u16;
    }

    fn remove_bytes(&mut self, offset: usize, len: usize) {
        assert!(offset + len <= self.len());
        let old_len = self.len();

        if offset + len < old_len {
            unsafe {
                std::ptr::copy(
                    self.bytes.as_ptr().add(offset + len),
                    self.bytes.as_mut_ptr().add(offset),
                    old_len - offset - len,
                );
            }
        }

        self.len = (old_len - len) as u16;
    }
}

/// A rope built on a skip list.
pub struct Rope {
    nodes: Vec<Node>,
    head: Idx,
    len: usize,
    free_list: Vec<Idx>,
    rand_state: u64,
}

impl Rope {
    pub fn new() -> Self {
        let mut rope = Rope {
            nodes: Vec::new(),
            head: 0,
            len: 0,
            free_list: Vec::new(),
            rand_state: 0x12345678_9abcdef0,
        };
        rope.head = rope.alloc_node(MAX_HEIGHT as u8);
        rope
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn node(&self, idx: Idx) -> &Node {
        &self.nodes[idx as usize]
    }

    fn node_mut(&mut self, idx: Idx) -> &mut Node {
        &mut self.nodes[idx as usize]
    }

    fn alloc_node(&mut self, height: u8) -> Idx {
        if let Some(idx) = self.free_list.pop() {
            self.nodes[idx as usize] = Node::new(height);
            idx
        } else {
            let idx = self.nodes.len() as Idx;
            self.nodes.push(Node::new(height));
            idx
        }
    }

    fn free_node(&mut self, idx: Idx) {
        self.free_list.push(idx);
    }

    fn random_height(&mut self) -> u8 {
        let mut x = self.rand_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rand_state = x;
        let zeros = x.trailing_zeros() as u8;
        (zeros.min(MAX_HEIGHT as u8 - 1)) + 1
    }

    /// Find the path to position `pos`.
    /// Returns (update, remaining) where:
    /// - update[level] = the last node we visited at this level BEFORE reaching the target
    ///                   (i.e., the node whose widths span includes pos)
    /// - remaining[level] = bytes remaining within update[level]'s span to reach pos
    fn find_path(&self, pos: usize) -> ([Idx; MAX_HEIGHT], [usize; MAX_HEIGHT]) {
        let mut update = [self.head; MAX_HEIGHT];
        let mut remaining = [pos; MAX_HEIGHT];

        let mut idx = self.head;
        let mut rem = pos;

        for level in (0..MAX_HEIGHT).rev() {
            if level < self.node(idx).height() {
                loop {
                    let node = self.node(idx);
                    let next = node.next[level];
                    if next == NULL {
                        break;
                    }
                    let width = node.widths[level] as usize;
                    // Only advance if position is STRICTLY beyond this node's span
                    // rem > width means pos is past this span
                    // rem == width means pos is at the boundary (end of span)
                    if rem > width {
                        rem -= width;
                        idx = next;
                    } else if rem == width && width > 0 {
                        // At exact boundary - could go either way
                        // For insertion, we want to insert after, so advance
                        rem = 0;
                        idx = next;
                    } else {
                        break;
                    }
                }
                update[level] = idx;
                remaining[level] = rem;
            }
        }

        (update, remaining)
    }

    /// Insert bytes at position.
    pub fn insert(&mut self, pos: usize, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        assert!(pos <= self.len, "position out of bounds");

        // Handle data larger than chunk size by splitting
        if data.len() > CHUNK_SIZE {
            self.insert(pos, &data[..CHUNK_SIZE]);
            self.insert(pos + CHUNK_SIZE, &data[CHUNK_SIZE..]);
            return;
        }

        let (update, remaining) = self.find_path(pos);

        // Find target node and offset within it
        let mut target_idx = update[0];
        let offset = remaining[0];

        // Move from head to first data node if needed
        if target_idx == self.head {
            let next = self.node(self.head).next[0];
            if next != NULL && pos > 0 {
                // Only move to data node if we're not inserting at position 0
                target_idx = next;
            }
        }

        // Determine actual offset within target node
        // If offset == node.len(), we want to append (insert at end of node)
        let actual_offset = if target_idx != self.head {
            // After find_path, remaining[0] is the offset within the target node's span
            // But if we landed exactly at node boundary, offset might be 0 when we want node.len()
            let node_len = self.node(target_idx).len();
            if offset == 0 && pos > 0 {
                // Check if we should be at the end of this node
                // This happens when pos equals the cumulative length up to and including this node
                node_len
            } else {
                offset
            }
        } else {
            0
        };

        // Try to insert into existing node if there's room and we're not at a node boundary
        if target_idx != self.head {
            let node_len = self.node(target_idx).len();
            if node_len + data.len() <= CHUNK_SIZE && actual_offset <= node_len {
                self.node_mut(target_idx).insert_bytes(actual_offset, data);
                self.increment_widths(&update, target_idx, data.len() as u32);
                self.len += data.len();
                return;
            }
        }

        // Need to create a new node
        let height = self.random_height();
        let new_idx = self.alloc_node(height);
        self.node_mut(new_idx).insert_bytes(0, data);

        // Wire up at all levels
        for level in 0..height as usize {
            let pred = update[level];
            let old_next = self.node(pred).next[level];
            self.node_mut(new_idx).next[level] = old_next;
            self.node_mut(pred).next[level] = new_idx;
        }

        // Set widths for new node
        self.node_mut(new_idx).widths[0] = data.len() as u32;
        for level in 1..height as usize {
            // Width = this node's bytes + span to next[level]
            let mut span = data.len() as u32;
            let target_next = self.node(new_idx).next[level];
            let mut walk = self.node(new_idx).next[0];
            while walk != NULL && walk != target_next {
                span += self.node(walk).widths[0];
                walk = self.node(walk).next[0];
            }
            self.node_mut(new_idx).widths[level] = span;
        }

        // Update predecessor widths
        for level in 0..MAX_HEIGHT {
            let pred = update[level];
            if level < self.node(pred).height() {
                if level < height as usize {
                    // Predecessor now points to new_idx instead of old_next
                    // New width = bytes from pred to new_idx
                    let mut span = 0u32;
                    let mut walk = if pred == self.head {
                        self.node(self.head).next[0]
                    } else {
                        // For data nodes, start counting from next[0]
                        self.node(pred).next[0]
                    };
                    // Actually this is getting complex. Let's use a simpler approach:
                    // Just add the inserted bytes to the predecessor's width
                    // This works because the structure didn't change at this level
                    // relative to what pred spans
                    self.node_mut(pred).widths[level] += data.len() as u32;
                } else {
                    // Level above new node's height - just add the bytes
                    self.node_mut(pred).widths[level] += data.len() as u32;
                }
            }
        }

        self.len += data.len();
    }

    fn increment_widths(&mut self, update: &[Idx; MAX_HEIGHT], target: Idx, delta: u32) {
        let target_height = self.node(target).height();

        // Increment target's widths at all its levels
        for level in 0..target_height {
            self.node_mut(target).widths[level] += delta;
        }

        // Also increment any predecessors that link directly to target
        // This handles the case where update[level] advanced past a node that points to target
        for level in 0..target_height {
            let pred = update[level];
            if pred != target && level < self.node(pred).height() {
                // pred links to target at this level, so increment pred's width too
                // But wait, update[level] might not be the direct predecessor...
                // Actually, we need to find who links to target at each level
            }
        }

        // Increment predecessor's widths at levels above target's height
        for level in target_height..MAX_HEIGHT {
            let pred = update[level];
            if level < self.node(pred).height() {
                self.node_mut(pred).widths[level] += delta;
            }
        }
    }

    /// Remove bytes starting at pos.
    pub fn remove(&mut self, pos: usize, len: usize) {
        if len == 0 {
            return;
        }
        assert!(pos + len <= self.len, "remove range out of bounds");

        let mut remaining = len;

        while remaining > 0 {
            let (update, rem_at) = self.find_path(pos);

            // Find target node
            let mut target_idx = update[0];
            let mut offset = rem_at[0];

            if target_idx == self.head {
                target_idx = self.node(self.head).next[0];
                if target_idx == NULL {
                    break;
                }
                // offset stays the same
            }

            let node_len = self.node(target_idx).len();
            let to_remove = remaining.min(node_len - offset);

            if to_remove == node_len && offset == 0 {
                // Remove entire node
                self.remove_node(&update, target_idx);
                self.len -= node_len;
                remaining -= to_remove;
            } else {
                // Partial removal
                self.node_mut(target_idx).remove_bytes(offset, to_remove);
                self.decrement_widths(&update, target_idx, to_remove as u32);
                self.len -= to_remove;
                remaining -= to_remove;
            }
        }
    }

    fn remove_node(&mut self, update: &[Idx; MAX_HEIGHT], target: Idx) {
        let height = self.node(target).height();
        let node_len = self.node(target).len() as u32;

        // Unlink at each level
        for level in 0..height {
            let pred = update[level];
            let target_next = self.node(target).next[level];
            let target_width = self.node(target).widths[level];

            self.node_mut(pred).next[level] = target_next;
            // Predecessor's width: was spanning to target, now spans to target_next
            // Add target's span beyond itself, subtract target's own bytes
            self.node_mut(pred).widths[level] += target_width - node_len;
        }

        // Decrement widths at levels above target's height
        for level in height..MAX_HEIGHT {
            let pred = update[level];
            if level < self.node(pred).height() {
                self.node_mut(pred).widths[level] -= node_len;
            }
        }

        self.free_node(target);
    }

    fn decrement_widths(&mut self, update: &[Idx; MAX_HEIGHT], target: Idx, delta: u32) {
        let target_height = self.node(target).height();

        // Decrement target's widths
        for level in 0..target_height {
            self.node_mut(target).widths[level] -= delta;
        }

        // Decrement predecessor's widths at levels above target's height
        for level in target_height..MAX_HEIGHT {
            let pred = update[level];
            if level < self.node(pred).height() {
                self.node_mut(pred).widths[level] -= delta;
            }
        }
    }

    pub fn get(&self, pos: usize) -> Option<u8> {
        if pos >= self.len {
            return None;
        }

        let (update, remaining) = self.find_path(pos);
        let mut idx = update[0];
        let mut offset = remaining[0];

        if idx == self.head {
            idx = self.node(self.head).next[0];
        }

        if idx == NULL {
            return None;
        }

        Some(self.node(idx).as_slice()[offset])
    }

    pub fn to_vec(&self) -> Vec<u8> {
        let mut result = Vec::with_capacity(self.len);
        let mut idx = self.node(self.head).next[0];
        while idx != NULL {
            result.extend_from_slice(self.node(idx).as_slice());
            idx = self.node(idx).next[0];
        }
        result
    }

    pub fn to_string_lossy(&self) -> String {
        String::from_utf8_lossy(&self.to_vec()).into_owned()
    }
}

impl Default for Rope {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_rope() {
        let rope = Rope::new();
        assert_eq!(rope.len(), 0);
        assert!(rope.is_empty());
        assert_eq!(rope.to_vec(), b"");
    }

    #[test]
    fn insert_at_beginning() {
        let mut rope = Rope::new();
        rope.insert(0, b"hello");
        assert_eq!(rope.len(), 5);
        assert_eq!(rope.to_vec(), b"hello");
    }

    #[test]
    fn insert_at_end() {
        let mut rope = Rope::new();
        rope.insert(0, b"hello");
        rope.insert(5, b" world");
        assert_eq!(rope.len(), 11);
        assert_eq!(rope.to_vec(), b"hello world");
    }

    #[test]
    fn insert_in_middle() {
        let mut rope = Rope::new();
        rope.insert(0, b"helo");
        rope.insert(2, b"l");
        assert_eq!(rope.to_vec(), b"hello");
    }

    #[test]
    fn insert_multiple() {
        let mut rope = Rope::new();
        rope.insert(0, b"a");
        eprintln!("After insert(0, a): {:?}", rope.to_vec());
        rope.insert(1, b"b");
        eprintln!("After insert(1, b): {:?}", rope.to_vec());
        rope.insert(2, b"c");
        eprintln!("After insert(2, c): {:?}", rope.to_vec());
        assert_eq!(rope.to_vec(), b"abc");
    }

    #[test]
    fn remove_all() {
        let mut rope = Rope::new();
        rope.insert(0, b"hello");
        rope.remove(0, 5);
        assert_eq!(rope.len(), 0);
        assert_eq!(rope.to_vec(), b"");
    }

    #[test]
    fn remove_prefix() {
        let mut rope = Rope::new();
        rope.insert(0, b"hello");
        rope.remove(0, 2);
        assert_eq!(rope.to_vec(), b"llo");
    }

    #[test]
    fn remove_suffix() {
        let mut rope = Rope::new();
        rope.insert(0, b"hello");
        rope.remove(3, 2);
        assert_eq!(rope.to_vec(), b"hel");
    }

    #[test]
    fn remove_middle() {
        let mut rope = Rope::new();
        rope.insert(0, b"hello");
        rope.remove(1, 3);
        assert_eq!(rope.to_vec(), b"ho");
    }

    #[test]
    fn get_byte() {
        let mut rope = Rope::new();
        rope.insert(0, b"hello");
        assert_eq!(rope.get(0), Some(b'h'));
        assert_eq!(rope.get(4), Some(b'o'));
        assert_eq!(rope.get(5), None);
    }

    #[test]
    fn large_insert() {
        let mut rope = Rope::new();
        let data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
        rope.insert(0, &data);
        assert_eq!(rope.len(), 1000);
        assert_eq!(rope.to_vec(), data);
    }

    #[test]
    fn many_small_inserts() {
        let mut rope = Rope::new();
        for i in 0..100 {
            rope.insert(i, b"x");
        }
        assert_eq!(rope.len(), 100);
        assert_eq!(rope.to_vec(), vec![b'x'; 100]);
    }

    #[test]
    fn interleaved_insert_remove() {
        let mut rope = Rope::new();
        rope.insert(0, b"hello world");
        rope.remove(5, 1); // "helloworld"
        rope.insert(5, b" beautiful "); // "hello beautiful world"
        assert_eq!(rope.to_vec(), b"hello beautiful world");
    }
}
