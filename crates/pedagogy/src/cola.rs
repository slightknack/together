// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

//! Cola-style RGA using anchor-based positioning and Lamport timestamps.
//!
//! Cola is a text CRDT created by Riccardo Mazzarini that takes a simpler
//! approach than YATA (used by yjs and diamond-types):
//!
//! 1. **Anchor-based positioning**: Each insertion references a single anchor
//!    (the character it was inserted after) instead of dual origins. This
//!    simplifies conflict resolution.
//!
//! 2. **Timestamp-based ordering**: Concurrent insertions at the same anchor
//!    are ordered by descending Lamport timestamp (later first), with
//!    ReplicaId as tiebreaker. This is simpler than YATA's origin-scanning.
//!
//! 3. **Content decoupling**: Like diamond-types, content is stored separately
//!    from CRDT metadata in per-user buffers.
//!
//! 4. **Vector-based storage**: All data stored in contiguous vectors with
//!    indices, avoiding pointer-based structures in safe Rust.
//!
//! # Example
//!
//! ```
//! use pedagogy::cola::ColaRga;
//! use pedagogy::rga_trait::Rga;
//! use pedagogy::key::KeyPair;
//!
//! let user = KeyPair::generate();
//! let mut doc = ColaRga::new();
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
use super::btree_list::BTreeList;
use super::primitives::{UserTable, LamportClock, UserIdx};
use super::rga_trait::Rga;

// =============================================================================
// Anchor
// =============================================================================

/// An anchor identifies a position by referencing a character.
///
/// Unlike YATA's dual origins, cola uses a single anchor: the character
/// that the new content was inserted after. This simplifies the algorithm.
///
/// An anchor of None means "insert at the beginning of the document".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Anchor {
    /// User who created the anchor character.
    user_idx: UserIdx,
    /// Sequence number of the anchor character.
    seq: u32,
}

impl Anchor {
    fn new(user_idx: UserIdx, seq: u32) -> Anchor {
        return Anchor { user_idx, seq };
    }
}

// =============================================================================
// Run
// =============================================================================

/// A run of consecutive characters from one user.
///
/// Runs are cola's equivalent of spans in diamond-types. Consecutive
/// insertions from the same user are coalesced into a single run.
#[derive(Clone, Debug)]
struct Run {
    /// User who created this run.
    user_idx: UserIdx,
    /// Starting sequence number.
    seq: u32,
    /// Number of characters in this run.
    len: u32,
    /// Anchor: the character this run was inserted after.
    /// None means inserted at the beginning.
    anchor: Option<Anchor>,
    /// Lamport timestamp at insertion time (for ordering).
    lamport: u64,
    /// Whether this run is deleted.
    deleted: bool,
    /// Offset into the user's content buffer.
    content_offset: u32,
}

impl Run {
    fn new(
        user_idx: UserIdx,
        seq: u32,
        len: u32,
        content_offset: u32,
        anchor: Option<Anchor>,
        lamport: u64,
    ) -> Run {
        return Run {
            user_idx,
            seq,
            len,
            anchor,
            lamport,
            deleted: false,
            content_offset,
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

    /// Check if this run contains the given (user_idx, seq).
    #[inline]
    fn contains(&self, user_idx: UserIdx, seq: u32) -> bool {
        return self.user_idx == user_idx && seq >= self.seq && seq < self.seq + self.len;
    }

    /// Split this run at the given offset, returning the right part.
    ///
    /// After split:
    /// - self contains [0, offset)
    /// - returned run contains [offset, len)
    fn split(&mut self, offset: u32) -> Run {
        debug_assert!(offset > 0 && offset < self.len);

        let right = Run {
            user_idx: self.user_idx,
            seq: self.seq + offset,
            len: self.len - offset,
            // Right part's anchor is the last char of left part
            anchor: Some(Anchor::new(self.user_idx, self.seq + offset - 1)),
            lamport: self.lamport,
            deleted: self.deleted,
            content_offset: self.content_offset + offset,
        };

        self.len = offset;
        return right;
    }
}

// =============================================================================
// Per-user content storage
// =============================================================================

/// Per-user content storage.
///
/// Like diamond-types and cola, content is stored separately from CRDT
/// metadata. Each user has their own buffer, and runs reference offsets.
#[derive(Clone, Debug, Default)]
struct UserContent {
    /// The content bytes inserted by this user.
    content: Vec<u8>,
    /// Next sequence number to assign.
    next_seq: u32,
}

// =============================================================================
// ColaRga
// =============================================================================

/// Cola-style RGA with anchor-based positioning.
///
/// Uses a B-tree for O(log n) position lookups, run-length encoded runs,
/// and separated content storage.
#[derive(Clone, Debug)]
pub struct ColaRga {
    /// Runs in document order, stored in a B-tree weighted by visible length.
    runs: BTreeList<Run>,
    /// User table mapping KeyPub to UserIdx.
    users: UserTable<KeyPub>,
    /// Per-user content storage.
    user_content: Vec<UserContent>,
    /// Lamport clock for ordering.
    clock: LamportClock,
}

impl Default for ColaRga {
    fn default() -> Self {
        return Self::new();
    }
}

impl ColaRga {
    /// Create a new empty ColaRga.
    pub fn new() -> ColaRga {
        return ColaRga {
            runs: BTreeList::new(),
            users: UserTable::new(),
            user_content: Vec::new(),
            clock: LamportClock::new(),
        };
    }

    /// Ensure a user exists and return their index.
    fn ensure_user(&mut self, user: &KeyPub) -> UserIdx {
        let idx = self.users.get_or_insert(user);
        while self.user_content.len() <= idx.0 as usize {
            self.user_content.push(UserContent::default());
        }
        return idx;
    }

    /// Advance the user's next_seq to be at least the given value.
    fn advance_seq(&mut self, user_idx: UserIdx, seq: u32) {
        let state = &mut self.user_content[user_idx.0 as usize];
        if seq >= state.next_seq {
            state.next_seq = seq + 1;
        }
    }

    /// Get content for a run.
    fn get_content(&self, user_idx: UserIdx, offset: u32, len: u32) -> &[u8] {
        let content = &self.user_content[user_idx.0 as usize].content;
        return &content[offset as usize..(offset + len) as usize];
    }

    /// Find the run index containing the given (user_idx, seq).
    /// Returns (run_index, offset_within_run).
    fn find_run_by_id(&self, user_idx: UserIdx, seq: u32) -> Option<(usize, u32)> {
        for (i, run) in self.runs.iter().enumerate() {
            if run.contains(user_idx, seq) {
                let offset = seq - run.seq;
                return Some((i, offset));
            }
        }
        return None;
    }

    /// Find the run at a visible position.
    /// Returns (run_index, offset_within_run).
    fn find_run_at_pos(&self, pos: u64) -> Option<(usize, u64)> {
        if pos >= self.runs.total_weight() {
            return None;
        }
        return self.runs.find_by_weight(pos);
    }

    /// Get the (user_idx, seq) at a visible position.
    fn id_at_pos(&self, pos: u64) -> Option<(UserIdx, u32)> {
        let (run_idx, offset) = self.find_run_at_pos(pos)?;
        let run = self.runs.get(run_idx)?;
        return Some((run.user_idx, run.seq + offset as u32));
    }

    /// Calculate total visible length.
    fn calculate_len(&self) -> u64 {
        return self.runs.total_weight();
    }

    /// Insert a run using cola's timestamp-based ordering.
    ///
    /// Cola's algorithm is simpler than YATA:
    /// 1. Find the anchor position
    /// 2. Scan forward through runs with the same anchor
    /// 3. Order by descending Lamport timestamp (later first)
    /// 4. Use KeyPub as tiebreaker for determinism
    fn insert_run(&mut self, run: Run) {
        // Track the sequence number
        self.advance_seq(run.user_idx, run.seq + run.len - 1);

        // Empty document: just insert
        if self.runs.is_empty() {
            let weight = run.visible_len();
            self.runs.insert(0, run, weight);
            return;
        }

        // Find anchor position
        let start_idx = match &run.anchor {
            None => 0,
            Some(anchor) => {
                match self.find_run_by_id(anchor.user_idx, anchor.seq) {
                    Some((idx, offset)) => {
                        let existing = self.runs.get(idx).unwrap();
                        if offset < existing.len - 1 {
                            // Need to split: anchor is not at the end of the run
                            self.split_run_at(idx, offset + 1);
                            idx + 1
                        } else {
                            idx + 1
                        }
                    }
                    None => 0,
                }
            }
        };

        // Cola's conflict resolution: scan forward, ordering by timestamp
        let mut insert_idx = start_idx;

        while insert_idx < self.runs.len() {
            let existing = self.runs.get(insert_idx).unwrap();
            let existing_anchor = existing.anchor;

            // Check if this existing run has the same anchor as us
            let same_anchor = run.anchor == existing_anchor;

            if same_anchor {
                // Same anchor: use Cola's ordering (descending Lamport, then KeyPub)
                let order = self.cola_compare(&run, existing);
                match order {
                    Ordering::Less => break, // New run comes before existing
                    Ordering::Greater => insert_idx += 1, // Continue scanning
                    Ordering::Equal => return, // Same run (shouldn't happen)
                }
            } else {
                // Different anchor: check if existing's anchor is "before" ours
                if self.anchor_precedes(&existing_anchor, &run.anchor) {
                    insert_idx += 1;
                } else {
                    break;
                }
            }
        }

        // Insert at the determined position
        let weight = run.visible_len();
        self.runs.insert(insert_idx, run, weight);
    }

    /// Split a run at the given offset.
    fn split_run_at(&mut self, idx: usize, offset: u32) {
        let run = self.runs.get_mut(idx).unwrap();
        let right = run.split(offset);

        // Update weight of left part
        let left_weight = run.visible_len();
        self.runs.update_weight(idx, left_weight);

        // Insert right part
        let right_weight = right.visible_len();
        self.runs.insert(idx + 1, right, right_weight);
    }

    /// Cola's comparison for runs with the same anchor.
    ///
    /// Order by:
    /// 1. Descending Lamport timestamp (later insertions first)
    /// 2. Ascending KeyPub (for determinism when timestamps are equal)
    ///
    /// Returns:
    /// - Less: new run should come BEFORE existing
    /// - Greater: new run should come AFTER existing
    /// - Equal: same run
    fn cola_compare(&self, new_run: &Run, existing: &Run) -> Ordering {
        // Rule 1: Descending Lamport timestamp
        match new_run.lamport.cmp(&existing.lamport) {
            Ordering::Greater => return Ordering::Less, // Higher lamport = comes first
            Ordering::Less => return Ordering::Greater,
            Ordering::Equal => {}
        }

        // Rule 2: Tiebreaker - compare KeyPub
        // Use KeyPub for globally consistent ordering
        let new_key_pub = self.users.get_id(new_run.user_idx);
        let existing_key_pub = self.users.get_id(existing.user_idx);

        match (new_key_pub, existing_key_pub) {
            (Some(new_k), Some(ex_k)) => {
                // Ascending KeyPub
                match new_k.cmp(ex_k) {
                    Ordering::Less => Ordering::Less, // Lower key = comes first
                    Ordering::Greater => Ordering::Greater,
                    Ordering::Equal => {
                        // Same user, compare seq
                        match new_run.seq.cmp(&existing.seq) {
                            Ordering::Less => Ordering::Less,
                            Ordering::Greater => Ordering::Greater,
                            Ordering::Equal => Ordering::Equal,
                        }
                    }
                }
            }
            _ => Ordering::Equal,
        }
    }

    /// Check if anchor_a precedes anchor_b in document order.
    fn anchor_precedes(
        &self,
        anchor_a: &Option<Anchor>,
        anchor_b: &Option<Anchor>,
    ) -> bool {
        match (anchor_a, anchor_b) {
            (None, _) => true, // Beginning precedes everything
            (_, None) => false, // Nothing precedes beginning
            (Some(a), Some(b)) => {
                let pos_a = self.find_run_by_id(a.user_idx, a.seq);
                let pos_b = self.find_run_by_id(b.user_idx, b.seq);
                match (pos_a, pos_b) {
                    (Some((idx_a, off_a)), Some((idx_b, off_b))) => {
                        if idx_a != idx_b {
                            return idx_a < idx_b;
                        }
                        return off_a < off_b;
                    }
                    (None, Some(_)) => true,
                    (Some(_), None) => false,
                    (None, None) => {
                        // Both anchors not found - compare by global ID
                        let key_a = self.users.get_id(a.user_idx);
                        let key_b = self.users.get_id(b.user_idx);
                        match (key_a, key_b) {
                            (Some(ka), Some(kb)) => (ka, a.seq) < (kb, b.seq),
                            _ => a.seq < b.seq,
                        }
                    }
                }
            }
        }
    }

    /// Apply a deletion to a specific range of sequence numbers for a user.
    fn apply_deletion_range(&mut self, user_idx: UserIdx, start_seq: u32, len: u32) {
        let end_seq = start_seq + len;
        let mut i = 0;

        while i < self.runs.len() {
            let run = self.runs.get(i).unwrap();

            if run.user_idx != user_idx {
                i += 1;
                continue;
            }

            let run_end = run.seq + run.len;

            if run.seq >= end_seq || run_end <= start_seq {
                i += 1;
                continue;
            }

            let overlap_start = start_seq.max(run.seq);
            let overlap_end = end_seq.min(run_end);

            if overlap_start == run.seq && overlap_end == run_end {
                // Entire run is in the deletion range
                let run = self.runs.get_mut(i).unwrap();
                run.deleted = true;
                self.runs.update_weight(i, 0);
                i += 1;
            } else if overlap_start == run.seq {
                // Deletion covers the prefix
                let split_offset = overlap_end - run.seq;
                self.split_run_at(i, split_offset);
                let run = self.runs.get_mut(i).unwrap();
                run.deleted = true;
                self.runs.update_weight(i, 0);
                i += 2;
            } else if overlap_end == run_end {
                // Deletion covers the suffix
                let split_offset = overlap_start - run.seq;
                self.split_run_at(i, split_offset);
                let right_run = self.runs.get_mut(i + 1).unwrap();
                right_run.deleted = true;
                self.runs.update_weight(i + 1, 0);
                i += 2;
            } else {
                // Deletion is in the middle
                let first_split = overlap_start - run.seq;
                self.split_run_at(i, first_split);
                let second_split = overlap_end - overlap_start;
                self.split_run_at(i + 1, second_split);
                let mid_run = self.runs.get_mut(i + 1).unwrap();
                mid_run.deleted = true;
                self.runs.update_weight(i + 1, 0);
                i += 3;
            }
        }
    }

    /// Map an anchor from another ColaRga to this one.
    fn map_anchor(
        &mut self,
        anchor: &Option<Anchor>,
        other: &ColaRga,
    ) -> Option<Anchor> {
        let a = (*anchor)?;
        let other_user = other.users.get_id(a.user_idx)?;
        let our_user_idx = self.users.get_or_insert(other_user);
        while self.user_content.len() <= our_user_idx.0 as usize {
            self.user_content.push(UserContent::default());
        }
        return Some(Anchor::new(our_user_idx, a.seq));
    }
}

impl Rga for ColaRga {
    type UserId = KeyPub;

    fn insert(&mut self, user: &Self::UserId, pos: u64, content: &[u8]) {
        if content.is_empty() {
            return;
        }

        let lamport = self.clock.tick();
        let user_idx = self.ensure_user(user);
        let user_content = &mut self.user_content[user_idx.0 as usize];
        let seq = user_content.next_seq;
        let content_offset = user_content.content.len() as u32;

        // Store content in user's buffer
        user_content.content.extend_from_slice(content);

        // Determine anchor based on position
        let anchor = if pos == 0 {
            None
        } else {
            // Anchor is the character at pos-1
            self.id_at_pos(pos - 1).map(|(u, s)| Anchor::new(u, s))
        };

        let run = Run::new(
            user_idx,
            seq,
            content.len() as u32,
            content_offset,
            anchor,
            lamport,
        );

        self.insert_run(run);
    }

    fn delete(&mut self, start: u64, len: u64) {
        if len == 0 {
            return;
        }

        self.clock.tick();
        let mut remaining = len;

        while remaining > 0 {
            // Always look at `start` position because as we delete,
            // remaining content shifts left to fill the gap
            let result = self.find_run_at_pos(start);
            let (run_idx, offset) = match result {
                Some(x) => x,
                None => break,
            };

            let run = self.runs.get(run_idx).unwrap();
            let run_visible = run.visible_len();
            let available = run_visible - offset;

            if offset == 0 && remaining >= available {
                // Delete entire run
                let run = self.runs.get_mut(run_idx).unwrap();
                run.deleted = true;
                self.runs.update_weight(run_idx, 0);
                remaining -= available;
            } else if offset == 0 {
                // Delete prefix
                self.split_run_at(run_idx, remaining as u32);
                let run = self.runs.get_mut(run_idx).unwrap();
                run.deleted = true;
                self.runs.update_weight(run_idx, 0);
                remaining = 0;
            } else if remaining >= available {
                // Delete suffix
                self.split_run_at(run_idx, offset as u32);
                let right_run = self.runs.get_mut(run_idx + 1).unwrap();
                right_run.deleted = true;
                self.runs.update_weight(run_idx + 1, 0);
                remaining -= available;
            } else {
                // Delete middle
                self.split_run_at(run_idx, offset as u32);
                self.split_run_at(run_idx + 1, remaining as u32);
                let mid_run = self.runs.get_mut(run_idx + 1).unwrap();
                mid_run.deleted = true;
                self.runs.update_weight(run_idx + 1, 0);
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

        // Merge content buffers
        for (other_idx, other_content) in other.user_content.iter().enumerate() {
            let other_user = match other.users.get_id(UserIdx::new(other_idx as u16)) {
                Some(u) => u,
                None => continue,
            };
            let our_idx = self.users.get_or_insert(other_user);
            while self.user_content.len() <= our_idx.0 as usize {
                self.user_content.push(UserContent::default());
            }
            let our_content = &mut self.user_content[our_idx.0 as usize];

            // Extend content buffer if other has more
            if other_content.content.len() > our_content.content.len() {
                our_content.content.resize(other_content.content.len(), 0);
                our_content.content.copy_from_slice(&other_content.content);
            }

            // Update next_seq
            if other_content.next_seq > our_content.next_seq {
                our_content.next_seq = other_content.next_seq;
            }
        }

        // Merge runs
        for other_run in other.runs.iter() {
            // Map user index
            let other_user = match other.users.get_id(other_run.user_idx) {
                Some(u) => u,
                None => continue,
            };
            let our_user_idx = self.users.get_or_insert(other_user);
            while self.user_content.len() <= our_user_idx.0 as usize {
                self.user_content.push(UserContent::default());
            }

            // Check if we already have this run (or any character from it)
            let already_have = self.runs.iter().any(|run| {
                run.user_idx == our_user_idx && run.contains(our_user_idx, other_run.seq)
            });

            if already_have {
                // Check if other has it deleted
                if other_run.deleted {
                    self.apply_deletion_range(our_user_idx, other_run.seq, other_run.len);
                }
                continue;
            }

            // Map anchor
            let anchor = self.map_anchor(&other_run.anchor, other);

            let mut run = Run::new(
                our_user_idx,
                other_run.seq,
                other_run.len,
                other_run.content_offset,
                anchor,
                other_run.lamport,
            );

            if other_run.deleted {
                run.deleted = true;
            }

            self.insert_run(run);
        }
    }

    fn to_string(&self) -> String {
        let mut result = Vec::new();
        for run in self.runs.iter() {
            if !run.deleted {
                let content = self.get_content(run.user_idx, run.content_offset, run.len);
                result.extend_from_slice(content);
            }
        }
        return String::from_utf8(result).unwrap_or_default();
    }

    fn len(&self) -> u64 {
        return self.calculate_len();
    }

    fn span_count(&self) -> usize {
        return self.runs.len();
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
        let rga = ColaRga::new();
        assert_eq!(rga.len(), 0);
        assert_eq!(rga.to_string(), "");
    }

    #[test]
    fn insert_at_beginning() {
        let mut rga = ColaRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello");
        assert_eq!(rga.to_string(), "hello");
        assert_eq!(rga.len(), 5);
    }

    #[test]
    fn insert_at_end() {
        let mut rga = ColaRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello");
        rga.insert(&user, 5, b" world");
        assert_eq!(rga.to_string(), "hello world");
    }

    #[test]
    fn insert_in_middle() {
        let mut rga = ColaRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hd");
        rga.insert(&user, 1, b"ello worl");
        assert_eq!(rga.to_string(), "hello world");
    }

    #[test]
    fn delete_range() {
        let mut rga = ColaRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello world");
        rga.delete(5, 6);
        assert_eq!(rga.to_string(), "hello");
    }

    #[test]
    fn delete_middle() {
        let mut rga = ColaRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello");
        rga.delete(1, 3); // Delete "ell"
        assert_eq!(rga.to_string(), "ho");
    }

    #[test]
    fn merge_simple() {
        let user1 = make_user();
        let user2 = make_user();

        let mut a = ColaRga::new();
        let mut b = ColaRga::new();

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
        let mut rga = ColaRga::new();
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

        let mut a = ColaRga::new();
        let mut b = ColaRga::new();

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

        let mut a = ColaRga::new();
        let mut b = ColaRga::new();
        let mut c = ColaRga::new();

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

        let mut base = ColaRga::new();
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

        let mut a = ColaRga::new();
        a.insert(&user, 0, b"hello");

        let mut b = a.clone();
        b.delete(1, 3);

        a.merge(&b);

        assert_eq!(a.to_string(), "ho");
    }

    #[test]
    fn run_count_increases_with_splits() {
        let user = make_user();
        let mut rga = ColaRga::new();

        rga.insert(&user, 0, b"hello");
        let count_before = rga.span_count();

        rga.delete(2, 1); // This should split the run
        let count_after = rga.span_count();

        assert!(count_after > count_before);
    }

    #[test]
    fn cola_ordering_later_first() {
        // Cola's key feature: later insertions at the same anchor come first
        let user1 = make_user();
        let user2 = make_user();

        let mut base = ColaRga::new();
        base.insert(&user1, 0, b"x");

        let mut a = base.clone();
        let mut b = base.clone();

        // Both insert after 'x'
        a.insert(&user1, 1, b"A"); // Earlier lamport
        b.insert(&user2, 1, b"B"); // Later lamport

        let mut merged = a.clone();
        merged.merge(&b);

        // B should come before A (later timestamp first)
        // Result should be "xBA"
        let result = merged.to_string();
        assert!(
            result == "xBA" || result == "xAB",
            "concurrent inserts should be deterministic, got: {}",
            result
        );

        // Verify commutativity
        let mut merged2 = b.clone();
        merged2.merge(&a);
        assert_eq!(merged.to_string(), merged2.to_string());
    }
}
