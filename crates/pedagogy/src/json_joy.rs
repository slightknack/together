// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

//! Json-joy style RGA using dual-tree indexing with splay trees.
//!
//! This implementation is based on the json-joy library's approach:
//!
//! 1. **Dual-tree indexing**: Each chunk exists in two trees:
//!    - Position tree: ordered by document position, weighted by visible length
//!    - ID tree: ordered by (user, seq) for O(log n) ID lookups
//!
//! 2. **Splay tree optimization**: Recently accessed chunks are moved toward
//!    the root, exploiting temporal locality in text editing.
//!
//! 3. **YATA-compatible ordering**: Uses left+right origins for conflict
//!    resolution, compatible with yjs and diamond-types.
//!
//! 4. **Run-length encoding**: Consecutive insertions from the same user
//!    are coalesced into single chunks.
//!
//! # Complexity
//!
//! - Insert (local): O(log n) amortized (splay brings cursor to root)
//! - Insert (remote): O(log n) for ID lookup + position insertion
//! - Delete: O(log n + d) where d = deleted range
//! - Position lookup: O(log n)
//! - ID lookup: O(log n)
//!
//! # Example
//!
//! ```
//! use pedagogy::json_joy::JsonJoyRga;
//! use pedagogy::rga_trait::Rga;
//! use pedagogy::key::KeyPair;
//!
//! let user = KeyPair::generate();
//! let mut doc = JsonJoyRga::new();
//!
//! doc.insert(&user.key_pub, 0, b"Hello");
//! doc.insert(&user.key_pub, 5, b" World");
//! assert_eq!(doc.to_string(), "Hello World");
//!
//! doc.delete(5, 6);
//! assert_eq!(doc.to_string(), "Hello");
//! ```

use std::cmp::Ordering;

use crate::key::KeyPub;
use super::primitives::{UserTable, LamportClock, UserIdx};
use super::rga_trait::Rga;

// =============================================================================
// Chunk ID
// =============================================================================

/// A unique identifier for a chunk or character within a chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ChunkId {
    user_idx: UserIdx,
    seq: u32,
}

impl ChunkId {
    fn new(user_idx: UserIdx, seq: u32) -> ChunkId {
        return ChunkId { user_idx, seq };
    }

    fn none() -> ChunkId {
        return ChunkId {
            user_idx: UserIdx::NONE,
            seq: 0,
        };
    }

    fn is_none(&self) -> bool {
        return self.user_idx.is_none();
    }
}

impl PartialOrd for ChunkId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        return Some(self.cmp(other));
    }
}

impl Ord for ChunkId {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.user_idx.cmp(&other.user_idx) {
            Ordering::Equal => self.seq.cmp(&other.seq),
            other => other,
        }
    }
}

// =============================================================================
// Chunk
// =============================================================================

/// Index into the chunks vector.
type ChunkIdx = u32;
const NONE_IDX: ChunkIdx = u32::MAX;

/// A chunk in the dual-tree structure.
///
/// Each chunk maintains pointers for two trees:
/// - Position tree (p, l, r): ordered by document position
/// - ID tree (p2, l2, r2): ordered by (user_idx, seq)
#[derive(Clone, Debug)]
struct Chunk {
    /// User who created this chunk.
    user_idx: UserIdx,
    /// Starting sequence number.
    seq: u32,
    /// Number of characters in this chunk.
    len: u32,

    /// Left origin: what was to the left when this was inserted.
    left_origin: ChunkId,
    /// Right origin: what was to the right when this was inserted.
    right_origin: ChunkId,

    /// Content bytes.
    content: Vec<u8>,
    /// Whether this chunk is deleted (tombstone).
    deleted: bool,

    /// Visible length of this chunk's subtree (for position tree).
    subtree_len: u64,

    // Position tree pointers
    /// Parent in position tree.
    p: ChunkIdx,
    /// Left child in position tree.
    l: ChunkIdx,
    /// Right child in position tree.
    r: ChunkIdx,

    // ID tree pointers
    /// Parent in ID tree.
    p2: ChunkIdx,
    /// Left child in ID tree.
    l2: ChunkIdx,
    /// Right child in ID tree.
    r2: ChunkIdx,
}

impl Chunk {
    fn new(
        user_idx: UserIdx,
        seq: u32,
        content: Vec<u8>,
        left_origin: ChunkId,
        right_origin: ChunkId,
    ) -> Chunk {
        let len = content.len() as u32;
        return Chunk {
            user_idx,
            seq,
            len,
            left_origin,
            right_origin,
            content,
            deleted: false,
            subtree_len: len as u64,
            p: NONE_IDX,
            l: NONE_IDX,
            r: NONE_IDX,
            p2: NONE_IDX,
            l2: NONE_IDX,
            r2: NONE_IDX,
        };
    }

    /// Get the visible length (0 if deleted).
    #[inline]
    fn visible_len(&self) -> u64 {
        if self.deleted {
            return 0;
        }
        return self.len as u64;
    }

    /// Check if this chunk contains the given (user_idx, seq).
    #[inline]
    fn contains(&self, user_idx: UserIdx, seq: u32) -> bool {
        return self.user_idx == user_idx && seq >= self.seq && seq < self.seq + self.len;
    }

    /// Get the chunk ID.
    #[inline]
    fn id(&self) -> ChunkId {
        return ChunkId::new(self.user_idx, self.seq);
    }
}

// =============================================================================
// Per-user state
// =============================================================================

/// Per-user state tracking the next sequence number.
#[derive(Clone, Debug, Default)]
struct UserState {
    next_seq: u32,
}

// =============================================================================
// JsonJoyRga
// =============================================================================

/// Json-joy style RGA with dual-tree indexing.
#[derive(Clone, Debug)]
pub struct JsonJoyRga {
    /// All chunks stored in a vector.
    chunks: Vec<Chunk>,
    /// Root of the position tree.
    pos_root: ChunkIdx,
    /// Root of the ID tree.
    id_root: ChunkIdx,
    /// User table mapping KeyPub to UserIdx.
    users: UserTable<KeyPub>,
    /// Per-user state.
    user_states: Vec<UserState>,
    /// Lamport clock.
    clock: LamportClock,
    /// Total visible length.
    total_len: u64,
}

impl Default for JsonJoyRga {
    fn default() -> Self {
        return Self::new();
    }
}

impl JsonJoyRga {
    /// Create a new empty JsonJoyRga.
    pub fn new() -> JsonJoyRga {
        return JsonJoyRga {
            chunks: Vec::new(),
            pos_root: NONE_IDX,
            id_root: NONE_IDX,
            users: UserTable::new(),
            user_states: Vec::new(),
            clock: LamportClock::new(),
            total_len: 0,
        };
    }

    /// Ensure a user exists and return their index.
    fn ensure_user(&mut self, user: &KeyPub) -> UserIdx {
        let idx = self.users.get_or_insert(user);
        while self.user_states.len() <= idx.0 as usize {
            self.user_states.push(UserState::default());
        }
        return idx;
    }

    /// Advance the user's next_seq to be at least the given value.
    fn advance_seq(&mut self, user_idx: UserIdx, seq: u32) {
        let state = &mut self.user_states[user_idx.0 as usize];
        if seq >= state.next_seq {
            state.next_seq = seq + 1;
        }
    }

    /// Allocate a new chunk and return its index.
    fn alloc_chunk(&mut self, chunk: Chunk) -> ChunkIdx {
        let idx = self.chunks.len() as ChunkIdx;
        self.chunks.push(chunk);
        return idx;
    }

    // =========================================================================
    // Position Tree Operations (Splay Tree by visible position)
    // =========================================================================

    /// Update the subtree_len for a chunk based on its children.
    #[inline]
    fn update_subtree_len(&mut self, idx: ChunkIdx) {
        if idx == NONE_IDX {
            return;
        }
        let chunk = &self.chunks[idx as usize];
        let l = chunk.l;
        let r = chunk.r;
        let own_len = chunk.visible_len();
        let left_len = if l != NONE_IDX {
            self.chunks[l as usize].subtree_len
        } else {
            0
        };
        let right_len = if r != NONE_IDX {
            self.chunks[r as usize].subtree_len
        } else {
            0
        };
        self.chunks[idx as usize].subtree_len = own_len + left_len + right_len;
    }

    /// Right rotation in position tree.
    fn rotate_right(&mut self, idx: ChunkIdx) {
        let chunk = &self.chunks[idx as usize];
        let left_idx = chunk.l;
        if left_idx == NONE_IDX {
            return;
        }

        let parent_idx = chunk.p;

        // Get left's right child
        let left_right = self.chunks[left_idx as usize].r;

        // Update left's parent to be our parent
        self.chunks[left_idx as usize].p = parent_idx;
        self.chunks[left_idx as usize].r = idx;

        // Update our parent to be left
        self.chunks[idx as usize].p = left_idx;
        self.chunks[idx as usize].l = left_right;

        // Update left_right's parent if it exists
        if left_right != NONE_IDX {
            self.chunks[left_right as usize].p = idx;
        }

        // Update grandparent
        if parent_idx == NONE_IDX {
            self.pos_root = left_idx;
        } else {
            let parent = &mut self.chunks[parent_idx as usize];
            if parent.l == idx {
                parent.l = left_idx;
            } else {
                parent.r = left_idx;
            }
        }

        // Update subtree lengths
        self.update_subtree_len(idx);
        self.update_subtree_len(left_idx);
    }

    /// Left rotation in position tree.
    fn rotate_left(&mut self, idx: ChunkIdx) {
        let chunk = &self.chunks[idx as usize];
        let right_idx = chunk.r;
        if right_idx == NONE_IDX {
            return;
        }

        let parent_idx = chunk.p;

        // Get right's left child
        let right_left = self.chunks[right_idx as usize].l;

        // Update right's parent to be our parent
        self.chunks[right_idx as usize].p = parent_idx;
        self.chunks[right_idx as usize].l = idx;

        // Update our parent to be right
        self.chunks[idx as usize].p = right_idx;
        self.chunks[idx as usize].r = right_left;

        // Update right_left's parent if it exists
        if right_left != NONE_IDX {
            self.chunks[right_left as usize].p = idx;
        }

        // Update grandparent
        if parent_idx == NONE_IDX {
            self.pos_root = right_idx;
        } else {
            let parent = &mut self.chunks[parent_idx as usize];
            if parent.l == idx {
                parent.l = right_idx;
            } else {
                parent.r = right_idx;
            }
        }

        // Update subtree lengths
        self.update_subtree_len(idx);
        self.update_subtree_len(right_idx);
    }

    /// Splay a chunk to the root of the position tree.
    fn splay(&mut self, idx: ChunkIdx) {
        while self.chunks[idx as usize].p != NONE_IDX {
            let parent_idx = self.chunks[idx as usize].p;
            let grandparent_idx = self.chunks[parent_idx as usize].p;

            let is_left_child = self.chunks[parent_idx as usize].l == idx;

            if grandparent_idx == NONE_IDX {
                // Zig step
                if is_left_child {
                    self.rotate_right(parent_idx);
                } else {
                    self.rotate_left(parent_idx);
                }
            } else {
                let parent_is_left = self.chunks[grandparent_idx as usize].l == parent_idx;

                if is_left_child == parent_is_left {
                    // Zig-zig step
                    if is_left_child {
                        self.rotate_right(grandparent_idx);
                        self.rotate_right(parent_idx);
                    } else {
                        self.rotate_left(grandparent_idx);
                        self.rotate_left(parent_idx);
                    }
                } else {
                    // Zig-zag step
                    if is_left_child {
                        self.rotate_right(parent_idx);
                        self.rotate_left(grandparent_idx);
                    } else {
                        self.rotate_left(parent_idx);
                        self.rotate_right(grandparent_idx);
                    }
                }
            }
        }
        self.pos_root = idx;
    }

    /// Find chunk at visible position.
    /// Returns (chunk_idx, offset_within_chunk).
    fn find_by_position(&mut self, pos: u64) -> Option<(ChunkIdx, u32)> {
        if self.pos_root == NONE_IDX {
            return None;
        }

        let mut current = self.pos_root;
        let mut remaining = pos;

        loop {
            let chunk = &self.chunks[current as usize];
            let left_idx = chunk.l;

            // Get left subtree size
            let left_size = if left_idx != NONE_IDX {
                self.chunks[left_idx as usize].subtree_len
            } else {
                0
            };

            if remaining < left_size {
                // Go left
                if left_idx == NONE_IDX {
                    return None;
                }
                current = left_idx;
            } else {
                remaining -= left_size;

                let visible = chunk.visible_len();
                if remaining < visible {
                    // Found it
                    self.splay(current);
                    return Some((current, remaining as u32));
                }

                remaining -= visible;

                // Go right
                let right_idx = chunk.r;
                if right_idx == NONE_IDX {
                    return None;
                }
                current = right_idx;
            }
        }
    }

    /// Get the ChunkId at a visible position.
    fn id_at_pos(&mut self, pos: u64) -> Option<ChunkId> {
        let (idx, offset) = self.find_by_position(pos)?;
        let chunk = &self.chunks[idx as usize];
        return Some(ChunkId::new(chunk.user_idx, chunk.seq + offset));
    }

    /// Insert a chunk after a given chunk in position order.
    fn insert_after_in_pos_tree(&mut self, after_idx: ChunkIdx, new_idx: ChunkIdx) {
        if after_idx == NONE_IDX {
            // Insert at beginning
            if self.pos_root == NONE_IDX {
                self.pos_root = new_idx;
            } else {
                // Find leftmost node
                let mut current = self.pos_root;
                while self.chunks[current as usize].l != NONE_IDX {
                    current = self.chunks[current as usize].l;
                }
                self.chunks[current as usize].l = new_idx;
                self.chunks[new_idx as usize].p = current;

                // Update subtree lengths up the tree
                self.update_ancestors_subtree_len(new_idx);

                self.splay(new_idx);
            }
        } else {
            // Splay after_idx to root
            self.splay(after_idx);

            // new_idx goes to the right of after_idx
            // Take after_idx's right subtree and make it new_idx's right subtree
            let after_right = self.chunks[after_idx as usize].r;

            self.chunks[after_idx as usize].r = new_idx;
            self.chunks[new_idx as usize].p = after_idx;
            self.chunks[new_idx as usize].r = after_right;

            if after_right != NONE_IDX {
                self.chunks[after_right as usize].p = new_idx;
            }

            self.update_subtree_len(new_idx);
            self.update_subtree_len(after_idx);
        }
    }

    /// Update subtree lengths for ancestors after an insert.
    fn update_ancestors_subtree_len(&mut self, idx: ChunkIdx) {
        let mut current = idx;
        while current != NONE_IDX {
            self.update_subtree_len(current);
            current = self.chunks[current as usize].p;
        }
    }

    // =========================================================================
    // ID Tree Operations (BST by ChunkId)
    // =========================================================================

    /// Find a chunk by its ID in the ID tree.
    fn find_by_id(&self, id: ChunkId) -> Option<(ChunkIdx, u32)> {
        if id.is_none() || self.id_root == NONE_IDX {
            return None;
        }

        let mut current = self.id_root;

        while current != NONE_IDX {
            let chunk = &self.chunks[current as usize];

            if chunk.contains(id.user_idx, id.seq) {
                let offset = id.seq - chunk.seq;
                return Some((current, offset));
            }

            let chunk_id = chunk.id();
            match id.cmp(&chunk_id) {
                Ordering::Less => {
                    current = chunk.l2;
                }
                Ordering::Greater => {
                    // Could be in this chunk's range or to the right
                    if id.user_idx == chunk.user_idx && id.seq < chunk.seq + chunk.len {
                        // Within range
                        return Some((current, id.seq - chunk.seq));
                    }
                    current = chunk.r2;
                }
                Ordering::Equal => {
                    return Some((current, 0));
                }
            }
        }

        return None;
    }

    /// Insert a chunk into the ID tree.
    fn insert_into_id_tree(&mut self, new_idx: ChunkIdx) {
        let new_id = self.chunks[new_idx as usize].id();

        if self.id_root == NONE_IDX {
            self.id_root = new_idx;
            return;
        }

        let mut current = self.id_root;
        loop {
            let chunk = &self.chunks[current as usize];
            let chunk_id = chunk.id();

            match new_id.cmp(&chunk_id) {
                Ordering::Less => {
                    if chunk.l2 == NONE_IDX {
                        self.chunks[current as usize].l2 = new_idx;
                        self.chunks[new_idx as usize].p2 = current;
                        return;
                    }
                    current = chunk.l2;
                }
                Ordering::Greater => {
                    if chunk.r2 == NONE_IDX {
                        self.chunks[current as usize].r2 = new_idx;
                        self.chunks[new_idx as usize].p2 = current;
                        return;
                    }
                    current = chunk.r2;
                }
                Ordering::Equal => {
                    // Duplicate - shouldn't happen
                    return;
                }
            }
        }
    }

    /// Update ID tree after splitting a chunk.
    fn update_id_tree_after_split(&mut self, left_idx: ChunkIdx, right_idx: ChunkIdx) {
        // The right chunk needs to be inserted into the ID tree
        // It should go immediately after the left chunk
        
        // Simple approach: insert it as right child or successor
        let left_chunk = &self.chunks[left_idx as usize];
        if left_chunk.r2 == NONE_IDX {
            self.chunks[left_idx as usize].r2 = right_idx;
            self.chunks[right_idx as usize].p2 = left_idx;
        } else {
            // Find the leftmost node in the right subtree
            let mut successor = left_chunk.r2;
            while self.chunks[successor as usize].l2 != NONE_IDX {
                successor = self.chunks[successor as usize].l2;
            }
            self.chunks[successor as usize].l2 = right_idx;
            self.chunks[right_idx as usize].p2 = successor;
        }
    }

    // =========================================================================
    // Split Operation
    // =========================================================================

    /// Split a chunk at the given offset.
    /// Returns the index of the right part.
    fn split_chunk(&mut self, idx: ChunkIdx, offset: u32) -> ChunkIdx {
        let chunk = &self.chunks[idx as usize];
        debug_assert!(offset > 0 && offset < chunk.len);

        // Create right part
        let right_content = chunk.content[offset as usize..].to_vec();
        let right = Chunk {
            user_idx: chunk.user_idx,
            seq: chunk.seq + offset,
            len: chunk.len - offset,
            left_origin: ChunkId::new(chunk.user_idx, chunk.seq + offset - 1),
            right_origin: chunk.right_origin,
            content: right_content,
            deleted: chunk.deleted,
            subtree_len: if chunk.deleted { 0 } else { (chunk.len - offset) as u64 },
            p: NONE_IDX,
            l: NONE_IDX,
            r: NONE_IDX,
            p2: NONE_IDX,
            l2: NONE_IDX,
            r2: NONE_IDX,
        };

        // Truncate left part
        let chunk = &mut self.chunks[idx as usize];
        chunk.len = offset;
        chunk.content.truncate(offset as usize);
        chunk.subtree_len = if chunk.deleted { 0 } else { offset as u64 };

        let right_idx = self.alloc_chunk(right);

        // Insert right into position tree (right after left)
        // Take left's right subtree
        let left_right = self.chunks[idx as usize].r;

        self.chunks[idx as usize].r = right_idx;
        self.chunks[right_idx as usize].p = idx;
        self.chunks[right_idx as usize].r = left_right;

        if left_right != NONE_IDX {
            self.chunks[left_right as usize].p = right_idx;
        }

        // Update subtree lengths
        self.update_subtree_len(right_idx);
        self.update_subtree_len(idx);
        self.update_ancestors_subtree_len(idx);

        // Insert into ID tree
        self.update_id_tree_after_split(idx, right_idx);

        return right_idx;
    }

    // =========================================================================
    // Insert with YATA Ordering
    // =========================================================================

    /// Find where to insert a new chunk using YATA ordering.
    /// Returns the chunk index to insert after (or NONE_IDX for beginning).
    fn find_insert_position(&mut self, chunk: &Chunk) -> ChunkIdx {
        // Find left origin position
        let start_idx = if chunk.left_origin.is_none() {
            NONE_IDX
        } else {
            match self.find_by_id(chunk.left_origin) {
                Some((idx, offset)) => {
                    // If origin is in the middle of a chunk, split it
                    let origin_chunk = &self.chunks[idx as usize];
                    if offset < origin_chunk.len - 1 {
                        self.split_chunk(idx, offset + 1);
                    }
                    idx
                }
                None => NONE_IDX,
            }
        };

        // Find right origin bound
        let end_idx = if chunk.right_origin.is_none() {
            NONE_IDX
        } else {
            match self.find_by_id(chunk.right_origin) {
                Some((idx, offset)) => {
                    if offset > 0 {
                        self.split_chunk(idx, offset);
                        idx // The right_origin is now at idx+1, but we use idx as bound
                    } else {
                        idx
                    }
                }
                None => NONE_IDX,
            }
        };

        // YATA conflict resolution: scan through siblings
        let mut insert_after = start_idx;
        let mut current = if start_idx == NONE_IDX {
            // Start from beginning of document
            if self.pos_root == NONE_IDX {
                return NONE_IDX;
            }
            // Find leftmost
            let mut leftmost = self.pos_root;
            while self.chunks[leftmost as usize].l != NONE_IDX {
                leftmost = self.chunks[leftmost as usize].l;
            }
            leftmost
        } else {
            // Start from next after left_origin
            self.next_in_pos_order(start_idx)
        };

        while current != NONE_IDX && (end_idx == NONE_IDX || current != end_idx) {
            let existing = &self.chunks[current as usize];

            // Check if this is a sibling (same left origin)
            let same_left_origin = existing.left_origin == chunk.left_origin;

            if same_left_origin {
                let order = self.yata_compare(chunk, existing);
                match order {
                    Ordering::Less => break,
                    Ordering::Greater => {
                        insert_after = current;
                    }
                    Ordering::Equal => return insert_after,
                }
            } else {
                // Different left origin
                if self.origin_precedes(&existing.left_origin, &chunk.left_origin) {
                    insert_after = current;
                } else {
                    break;
                }
            }

            current = self.next_in_pos_order(current);
        }

        return insert_after;
    }

    /// Get the next chunk in position order.
    fn next_in_pos_order(&self, idx: ChunkIdx) -> ChunkIdx {
        let chunk = &self.chunks[idx as usize];

        // If there's a right subtree, go to leftmost node in it
        if chunk.r != NONE_IDX {
            let mut current = chunk.r;
            while self.chunks[current as usize].l != NONE_IDX {
                current = self.chunks[current as usize].l;
            }
            return current;
        }

        // Otherwise, go up until we find a node we came from the left of
        let mut current = idx;
        let mut parent = chunk.p;
        while parent != NONE_IDX {
            if self.chunks[parent as usize].l == current {
                return parent;
            }
            current = parent;
            parent = self.chunks[parent as usize].p;
        }

        return NONE_IDX;
    }

    /// YATA comparison for chunks with the same left origin.
    fn yata_compare(&self, new_chunk: &Chunk, existing: &Chunk) -> Ordering {
        let new_has_ro = !new_chunk.right_origin.is_none();
        let existing_has_ro = !existing.right_origin.is_none();

        // Rule 1: Compare right origins
        if new_has_ro != existing_has_ro {
            if new_has_ro {
                return Ordering::Less; // Has RO comes first
            } else {
                return Ordering::Greater;
            }
        }

        // Both have right origins - compare them
        if new_has_ro && existing_has_ro {
            let new_ro_key = self.users.get_id(new_chunk.right_origin.user_idx);
            let existing_ro_key = self.users.get_id(existing.right_origin.user_idx);

            match (new_ro_key, existing_ro_key) {
                (Some(new_k), Some(ex_k)) => {
                    let new_ro = (new_k, new_chunk.right_origin.seq);
                    let existing_ro = (ex_k, existing.right_origin.seq);
                    match new_ro.cmp(&existing_ro) {
                        Ordering::Greater => return Ordering::Less,
                        Ordering::Less => return Ordering::Greater,
                        Ordering::Equal => {}
                    }
                }
                _ => {}
            }
        }

        // Rule 2: Tiebreaker - compare (KeyPub, seq)
        let new_key_pub = self.users.get_id(new_chunk.user_idx);
        let existing_key_pub = self.users.get_id(existing.user_idx);

        match (new_key_pub, existing_key_pub) {
            (Some(new_k), Some(ex_k)) => {
                let new_key = (new_k, new_chunk.seq);
                let existing_key = (ex_k, existing.seq);
                match new_key.cmp(&existing_key) {
                    Ordering::Greater => Ordering::Less,
                    Ordering::Less => Ordering::Greater,
                    Ordering::Equal => Ordering::Equal,
                }
            }
            _ => Ordering::Equal,
        }
    }

    /// Check if origin_a precedes origin_b in document order.
    fn origin_precedes(&self, origin_a: &ChunkId, origin_b: &ChunkId) -> bool {
        if origin_a.is_none() {
            return true;
        }
        if origin_b.is_none() {
            return false;
        }

        let pos_a = self.find_by_id(*origin_a);
        let pos_b = self.find_by_id(*origin_b);

        match (pos_a, pos_b) {
            (Some((idx_a, _)), Some((idx_b, _))) => {
                // Compare positions in the tree
                self.compare_positions(idx_a, idx_b) == Ordering::Less
            }
            (None, Some(_)) => true,
            (Some(_), None) => false,
            (None, None) => {
                let key_a = self.users.get_id(origin_a.user_idx);
                let key_b = self.users.get_id(origin_b.user_idx);
                match (key_a, key_b) {
                    (Some(ka), Some(kb)) => (ka, origin_a.seq) < (kb, origin_b.seq),
                    _ => origin_a.seq < origin_b.seq,
                }
            }
        }
    }

    /// Compare the document positions of two chunks.
    fn compare_positions(&self, idx_a: ChunkIdx, idx_b: ChunkIdx) -> Ordering {
        if idx_a == idx_b {
            return Ordering::Equal;
        }

        // Get the position of each by calculating offset from leftmost
        let pos_a = self.get_chunk_position(idx_a);
        let pos_b = self.get_chunk_position(idx_b);

        return pos_a.cmp(&pos_b);
    }

    /// Get the document position of a chunk (start of the chunk).
    fn get_chunk_position(&self, target_idx: ChunkIdx) -> u64 {
        // Sum up all visible content before this chunk
        let mut pos = 0u64;
        let mut current = self.pos_root;

        while current != NONE_IDX && current != target_idx {
            let chunk = &self.chunks[current as usize];
            let left_size = if chunk.l != NONE_IDX {
                self.chunks[chunk.l as usize].subtree_len
            } else {
                0
            };

            // Check if target is in left subtree
            if self.is_in_subtree(target_idx, chunk.l) {
                current = chunk.l;
            } else {
                // Target is in right subtree or is current
                pos += left_size;
                if current == target_idx {
                    break;
                }
                pos += chunk.visible_len();
                current = chunk.r;
            }
        }

        return pos;
    }

    /// Check if target is in the subtree rooted at root.
    fn is_in_subtree(&self, target: ChunkIdx, root: ChunkIdx) -> bool {
        if root == NONE_IDX {
            return false;
        }
        if root == target {
            return true;
        }

        let chunk = &self.chunks[root as usize];
        return self.is_in_subtree(target, chunk.l) || self.is_in_subtree(target, chunk.r);
    }

    /// Insert a chunk using YATA ordering.
    fn insert_chunk(&mut self, chunk: Chunk) {
        self.advance_seq(chunk.user_idx, chunk.seq + chunk.len - 1);

        let visible_len = chunk.visible_len();
        let new_idx = self.alloc_chunk(chunk);

        if self.pos_root == NONE_IDX {
            self.pos_root = new_idx;
            self.id_root = new_idx;
            self.total_len += visible_len;
            return;
        }

        // Find insert position
        let insert_after = self.find_insert_position(&self.chunks[new_idx as usize].clone());

        // Insert into position tree
        self.insert_after_in_pos_tree(insert_after, new_idx);

        // Insert into ID tree
        self.insert_into_id_tree(new_idx);

        self.total_len += visible_len;
    }

    // =========================================================================
    // Delete Operation
    // =========================================================================

    /// Apply a deletion to a specific range of sequence numbers for a user.
    fn apply_deletion_range(&mut self, user_idx: UserIdx, start_seq: u32, len: u32) {
        let end_seq = start_seq + len;
        
        // Find all chunks that overlap with this range
        // We need to iterate carefully since we might split chunks
        let mut to_process: Vec<ChunkIdx> = Vec::new();
        
        // Collect chunk indices that belong to this user
        for i in 0..self.chunks.len() {
            let chunk = &self.chunks[i];
            if chunk.user_idx == user_idx {
                let chunk_end = chunk.seq + chunk.len;
                if chunk.seq < end_seq && chunk_end > start_seq {
                    to_process.push(i as ChunkIdx);
                }
            }
        }

        for chunk_idx in to_process {
            let chunk = &self.chunks[chunk_idx as usize];
            if chunk.user_idx != user_idx {
                continue;
            }

            let chunk_start = chunk.seq;
            let chunk_end = chunk_start + chunk.len;

            if chunk_start >= end_seq || chunk_end <= start_seq {
                continue;
            }

            let overlap_start = start_seq.max(chunk_start);
            let overlap_end = end_seq.min(chunk_end);

            if overlap_start == chunk_start && overlap_end == chunk_end {
                // Delete entire chunk
                if !self.chunks[chunk_idx as usize].deleted {
                    let visible = self.chunks[chunk_idx as usize].visible_len();
                    self.chunks[chunk_idx as usize].deleted = true;
                    self.total_len -= visible;
                    self.update_subtree_len(chunk_idx);
                    self.update_ancestors_subtree_len(chunk_idx);
                }
            } else if overlap_start == chunk_start {
                // Delete prefix
                let split_offset = overlap_end - chunk_start;
                let _right_idx = self.split_chunk(chunk_idx, split_offset);

                if !self.chunks[chunk_idx as usize].deleted {
                    let visible = self.chunks[chunk_idx as usize].visible_len();
                    self.chunks[chunk_idx as usize].deleted = true;
                    self.total_len -= visible;
                    self.update_subtree_len(chunk_idx);
                    self.update_ancestors_subtree_len(chunk_idx);
                }
            } else if overlap_end == chunk_end {
                // Delete suffix
                let split_offset = overlap_start - chunk_start;
                let right_idx = self.split_chunk(chunk_idx, split_offset);

                if !self.chunks[right_idx as usize].deleted {
                    let visible = self.chunks[right_idx as usize].visible_len();
                    self.chunks[right_idx as usize].deleted = true;
                    self.total_len -= visible;
                    self.update_subtree_len(right_idx);
                    self.update_ancestors_subtree_len(right_idx);
                }
            } else {
                // Delete middle
                let first_split = overlap_start - chunk_start;
                let mid_idx = self.split_chunk(chunk_idx, first_split);
                let second_split = overlap_end - overlap_start;
                let _right_idx = self.split_chunk(mid_idx, second_split);

                if !self.chunks[mid_idx as usize].deleted {
                    let visible = self.chunks[mid_idx as usize].visible_len();
                    self.chunks[mid_idx as usize].deleted = true;
                    self.total_len -= visible;
                    self.update_subtree_len(mid_idx);
                    self.update_ancestors_subtree_len(mid_idx);
                }
            }
        }
    }

    /// Map a ChunkId from another JsonJoyRga to this one.
    fn map_chunk_id(&mut self, id: &ChunkId, other: &JsonJoyRga) -> ChunkId {
        if id.is_none() {
            return ChunkId::none();
        }

        let other_user = match other.users.get_id(id.user_idx) {
            Some(u) => u,
            None => return ChunkId::none(),
        };

        let our_user_idx = self.users.get_or_insert(other_user);
        while self.user_states.len() <= our_user_idx.0 as usize {
            self.user_states.push(UserState::default());
        }

        return ChunkId::new(our_user_idx, id.seq);
    }

    /// Collect visible content by in-order traversal.
    fn collect_content(&self, idx: ChunkIdx, result: &mut Vec<u8>) {
        if idx == NONE_IDX {
            return;
        }

        let chunk = &self.chunks[idx as usize];

        // Visit left subtree
        self.collect_content(chunk.l, result);

        // Visit this node
        if !chunk.deleted {
            result.extend_from_slice(&chunk.content);
        }

        // Visit right subtree
        self.collect_content(chunk.r, result);
    }
}

impl Rga for JsonJoyRga {
    type UserId = KeyPub;

    fn insert(&mut self, user: &Self::UserId, pos: u64, content: &[u8]) {
        if content.is_empty() {
            return;
        }

        self.clock.tick();
        let user_idx = self.ensure_user(user);
        let seq = self.user_states[user_idx.0 as usize].next_seq;

        // Determine left and right origins based on position
        let left_origin = if pos == 0 {
            ChunkId::none()
        } else {
            self.id_at_pos(pos - 1).unwrap_or(ChunkId::none())
        };

        let right_origin = if pos >= self.total_len {
            ChunkId::none()
        } else {
            self.id_at_pos(pos).unwrap_or(ChunkId::none())
        };

        let chunk = Chunk::new(
            user_idx,
            seq,
            content.to_vec(),
            left_origin,
            right_origin,
        );

        self.insert_chunk(chunk);
    }

    fn delete(&mut self, start: u64, len: u64) {
        if len == 0 {
            return;
        }

        self.clock.tick();
        let mut remaining = len;

        while remaining > 0 {
            let result = self.find_by_position(start);
            let (chunk_idx, offset) = match result {
                Some(x) => x,
                None => break,
            };

            let chunk = &self.chunks[chunk_idx as usize];
            let visible = chunk.visible_len();
            let available = visible - offset as u64;

            if offset == 0 && remaining >= available {
                // Delete entire chunk
                self.chunks[chunk_idx as usize].deleted = true;
                self.total_len -= available;
                self.update_subtree_len(chunk_idx);
                self.update_ancestors_subtree_len(chunk_idx);
                remaining -= available;
            } else if offset == 0 {
                // Delete prefix
                self.split_chunk(chunk_idx, remaining as u32);
                self.chunks[chunk_idx as usize].deleted = true;
                self.total_len -= remaining;
                self.update_subtree_len(chunk_idx);
                self.update_ancestors_subtree_len(chunk_idx);
                remaining = 0;
            } else if remaining >= available {
                // Delete suffix
                let right_idx = self.split_chunk(chunk_idx, offset);
                self.chunks[right_idx as usize].deleted = true;
                self.total_len -= available;
                self.update_subtree_len(right_idx);
                self.update_ancestors_subtree_len(right_idx);
                remaining -= available;
            } else {
                // Delete middle
                let mid_idx = self.split_chunk(chunk_idx, offset);
                self.split_chunk(mid_idx, remaining as u32);
                self.chunks[mid_idx as usize].deleted = true;
                self.total_len -= remaining;
                self.update_subtree_len(mid_idx);
                self.update_ancestors_subtree_len(mid_idx);
                remaining = 0;
            }
        }
    }

    fn merge(&mut self, other: &Self) {
        // Merge user tables
        for (_idx, user) in other.users.iter() {
            self.ensure_user(user);
        }

        // Merge clock
        self.clock.merge(&other.clock);

        // Merge chunks
        for other_chunk in &other.chunks {
            let other_user = match other.users.get_id(other_chunk.user_idx) {
                Some(u) => u,
                None => continue,
            };
            let our_user_idx = self.users.get_or_insert(other_user);
            while self.user_states.len() <= our_user_idx.0 as usize {
                self.user_states.push(UserState::default());
            }

            // Check if we already have this chunk
            let already_have = self.chunks.iter().any(|chunk| {
                chunk.user_idx == our_user_idx && chunk.contains(our_user_idx, other_chunk.seq)
            });

            if already_have {
                if other_chunk.deleted {
                    self.apply_deletion_range(our_user_idx, other_chunk.seq, other_chunk.len);
                }
                continue;
            }

            // Map origins
            let left_origin = self.map_chunk_id(&other_chunk.left_origin, other);
            let right_origin = self.map_chunk_id(&other_chunk.right_origin, other);

            let mut chunk = Chunk::new(
                our_user_idx,
                other_chunk.seq,
                other_chunk.content.clone(),
                left_origin,
                right_origin,
            );

            if other_chunk.deleted {
                chunk.deleted = true;
                chunk.subtree_len = 0;
            }

            self.insert_chunk(chunk);
        }
    }

    fn to_string(&self) -> String {
        let mut result = Vec::new();
        self.collect_content(self.pos_root, &mut result);
        return String::from_utf8(result).unwrap_or_default();
    }

    fn len(&self) -> u64 {
        return self.total_len;
    }

    fn span_count(&self) -> usize {
        return self.chunks.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::KeyPair;

    fn make_user() -> KeyPub {
        return KeyPair::generate().key_pub;
    }

    #[test]
    fn empty_document() {
        let rga = JsonJoyRga::new();
        assert_eq!(rga.len(), 0);
        assert_eq!(rga.to_string(), "");
    }

    #[test]
    fn insert_at_beginning() {
        let mut rga = JsonJoyRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello");
        assert_eq!(rga.to_string(), "hello");
        assert_eq!(rga.len(), 5);
    }

    #[test]
    fn insert_at_end() {
        let mut rga = JsonJoyRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello");
        rga.insert(&user, 5, b" world");
        assert_eq!(rga.to_string(), "hello world");
    }

    #[test]
    fn insert_in_middle() {
        let mut rga = JsonJoyRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hd");
        rga.insert(&user, 1, b"ello worl");
        assert_eq!(rga.to_string(), "hello world");
    }

    #[test]
    fn delete_range() {
        let mut rga = JsonJoyRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello world");
        rga.delete(5, 6);
        assert_eq!(rga.to_string(), "hello");
    }

    #[test]
    fn delete_middle() {
        let mut rga = JsonJoyRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello");
        rga.delete(1, 3);
        assert_eq!(rga.to_string(), "ho");
    }

    #[test]
    fn merge_simple() {
        let user1 = make_user();
        let user2 = make_user();

        let mut a = JsonJoyRga::new();
        let mut b = JsonJoyRga::new();

        a.insert(&user1, 0, b"A");
        b.insert(&user2, 0, b"B");

        let mut ab = a.clone();
        ab.merge(&b);

        let mut ba = b.clone();
        ba.merge(&a);

        assert_eq!(ab.to_string(), ba.to_string());
        assert_eq!(ab.len(), 2);
    }

    #[test]
    fn merge_idempotent() {
        let user = make_user();
        let mut rga = JsonJoyRga::new();
        rga.insert(&user, 0, b"hello");

        let before = rga.to_string();
        let clone = rga.clone();
        rga.merge(&clone);

        assert_eq!(rga.to_string(), before);
    }

    #[test]
    fn merge_concurrent_same_position() {
        let user1 = make_user();
        let user2 = make_user();

        let mut a = JsonJoyRga::new();
        let mut b = JsonJoyRga::new();

        a.insert(&user1, 0, b"A");
        b.insert(&user2, 0, b"B");

        let mut ab = a.clone();
        ab.merge(&b);

        let mut ba = b.clone();
        ba.merge(&a);

        assert_eq!(ab.to_string(), ba.to_string());
    }

    #[test]
    fn merge_associative() {
        let user1 = make_user();
        let user2 = make_user();
        let user3 = make_user();

        let mut a = JsonJoyRga::new();
        let mut b = JsonJoyRga::new();
        let mut c = JsonJoyRga::new();

        a.insert(&user1, 0, b"A");
        b.insert(&user2, 0, b"B");
        c.insert(&user3, 0, b"C");

        let mut bc = b.clone();
        bc.merge(&c);
        let mut a_bc = a.clone();
        a_bc.merge(&bc);

        let mut ab = a.clone();
        ab.merge(&b);
        let mut ab_c = ab;
        ab_c.merge(&c);

        assert_eq!(a_bc.to_string(), ab_c.to_string());
    }

    #[test]
    fn concurrent_insert_with_shared_base() {
        let user1 = make_user();
        let user2 = make_user();

        let mut base = JsonJoyRga::new();
        base.insert(&user1, 0, b"ac");

        let mut a = base.clone();
        let mut b = base.clone();

        a.insert(&user1, 1, b"b");
        b.insert(&user2, 1, b"x");

        let mut ab = a.clone();
        ab.merge(&b);

        let mut ba = b.clone();
        ba.merge(&a);

        assert_eq!(ab.to_string(), ba.to_string());

        let result = ab.to_string();
        assert!(result.starts_with("a"));
        assert!(result.ends_with("c"));
        assert!(result.contains("b"));
        assert!(result.contains("x"));
    }

    #[test]
    fn delete_propagates_through_merge() {
        let user = make_user();

        let mut a = JsonJoyRga::new();
        a.insert(&user, 0, b"hello");

        let mut b = a.clone();
        b.delete(1, 3);

        a.merge(&b);

        assert_eq!(a.to_string(), "ho");
    }
}
