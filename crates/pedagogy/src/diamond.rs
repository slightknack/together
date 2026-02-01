// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

//! Diamond-types style RGA implementation.
//!
//! This implementation is inspired by Joseph Gentle's diamond-types CRDT,
//! which achieves 5000x better performance than Automerge through:
//!
//! 1. Separating content from CRDT metadata
//! 2. Using B-trees instead of linked lists for O(log n) operations
//! 3. Run-length encoding spans of consecutive characters
//! 4. Cursor caching for sequential access patterns
//!
//! # Architecture
//!
//! - `spans`: B-tree of `Span` items, weighted by visible character count
//! - `users`: Table mapping `KeyPub` to compact `UserIdx`
//! - `user_content`: Per-user content buffers (separate from CRDT metadata)
//! - `clock`: Lamport clock for ordering
//! - `cursor_cache`: Cache for amortizing sequential lookups
//!
//! # Example
//!
//! ```
//! use pedagogy::diamond::DiamondRga;
//! use pedagogy::rga_trait::Rga;
//! use pedagogy::key::KeyPair;
//!
//! let user = KeyPair::generate();
//! let mut doc = DiamondRga::new();
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
use super::primitives::{UserTable, LamportClock, UserIdx, CursorCache, BTreeLocation};
use super::rga_trait::Rga;

// =============================================================================
// Span
// =============================================================================

/// A span of consecutive characters from one user.
///
/// Unlike yjs which stores one Item per insertion, diamond-types coalesces
/// consecutive characters from the same user into spans. This reduces memory
/// usage and improves cache locality.
///
/// Each span stores:
/// - User and sequence range
/// - Left and right origins (for YATA conflict resolution)
/// - Content offset (into user's content buffer)
/// - Deletion state
#[derive(Clone, Debug)]
struct Span {
    /// User who created this span.
    user_idx: UserIdx,
    /// Starting sequence number.
    seq: u32,
    /// Number of characters in this span.
    len: u32,
    /// Left origin: what was to the left when this was inserted.
    left_origin: Option<(UserIdx, u32)>,
    /// Right origin: what was to the right when this was inserted.
    right_origin: Option<(UserIdx, u32)>,
    /// Whether this span is deleted.
    deleted: bool,
    /// Offset into the user's content buffer.
    content_offset: u32,
}

impl Span {
    fn new(
        user_idx: UserIdx,
        seq: u32,
        len: u32,
        content_offset: u32,
        left_origin: Option<(UserIdx, u32)>,
        right_origin: Option<(UserIdx, u32)>,
    ) -> Span {
        return Span {
            user_idx,
            seq,
            len,
            left_origin,
            right_origin,
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

    /// Check if this span contains the given (user_idx, seq).
    #[inline]
    fn contains(&self, user_idx: UserIdx, seq: u32) -> bool {
        return self.user_idx == user_idx && seq >= self.seq && seq < self.seq + self.len;
    }

    /// Split this span at the given offset, returning the right part.
    ///
    /// After split:
    /// - self contains [0, offset)
    /// - returned span contains [offset, len)
    fn split(&mut self, offset: u32) -> Span {
        debug_assert!(offset > 0 && offset < self.len);

        let right = Span {
            user_idx: self.user_idx,
            seq: self.seq + offset,
            len: self.len - offset,
            // Right part's left origin is the last char of left part
            left_origin: Some((self.user_idx, self.seq + offset - 1)),
            // Right origin stays the same
            right_origin: self.right_origin,
            deleted: self.deleted,
            content_offset: self.content_offset + offset,
        };

        self.len = offset;
        return right;
    }

    /// Check if this span can be coalesced with the next span.
    fn can_coalesce(&self, next: &Span) -> bool {
        return self.user_idx == next.user_idx
            && self.seq + self.len == next.seq
            && self.content_offset + self.len == next.content_offset
            && self.deleted == next.deleted;
    }

    /// Coalesce with the next span (extend this span).
    fn coalesce(&mut self, next: &Span) {
        debug_assert!(self.can_coalesce(next));
        self.len += next.len;
    }
}

// =============================================================================
// Per-user content storage
// =============================================================================

/// Per-user content storage.
///
/// Diamond-types separates content from CRDT metadata. Each user has their own
/// content buffer, and spans reference offsets into this buffer.
#[derive(Clone, Debug, Default)]
struct UserContent {
    /// The content bytes inserted by this user.
    content: Vec<u8>,
    /// Next sequence number to assign.
    next_seq: u32,
}

// =============================================================================
// DiamondRga
// =============================================================================

/// Diamond-types style RGA implementation.
///
/// Uses a B-tree for O(log n) position lookups, run-length encoded spans,
/// and separated content storage.
#[derive(Clone, Debug)]
pub struct DiamondRga {
    /// Spans in document order, stored in a B-tree weighted by visible length.
    spans: BTreeList<Span>,
    /// User table mapping KeyPub to UserIdx.
    users: UserTable<KeyPub>,
    /// Per-user content storage.
    user_content: Vec<UserContent>,
    /// Lamport clock for ordering.
    clock: LamportClock,
    /// Cursor cache for sequential access.
    cursor_cache: CursorCache<BTreeLocation>,
}

impl Default for DiamondRga {
    fn default() -> Self {
        return Self::new();
    }
}

impl DiamondRga {
    /// Create a new empty DiamondRga.
    pub fn new() -> DiamondRga {
        return DiamondRga {
            spans: BTreeList::new(),
            users: UserTable::new(),
            user_content: Vec::new(),
            clock: LamportClock::new(),
            cursor_cache: CursorCache::new(),
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

    /// Get content for a span.
    fn get_content(&self, user_idx: UserIdx, offset: u32, len: u32) -> &[u8] {
        let content = &self.user_content[user_idx.0 as usize].content;
        return &content[offset as usize..(offset + len) as usize];
    }

    /// Find the span index containing the given (user_idx, seq).
    /// Returns (span_index, offset_within_span).
    fn find_span_by_id(&self, user_idx: UserIdx, seq: u32) -> Option<(usize, u32)> {
        for (i, span) in self.spans.iter().enumerate() {
            if span.contains(user_idx, seq) {
                let offset = seq - span.seq;
                return Some((i, offset));
            }
        }
        return None;
    }

    /// Find the span at a visible position.
    /// Returns (span_index, offset_within_span).
    fn find_span_at_pos(&self, pos: u64) -> Option<(usize, u64)> {
        if pos >= self.spans.total_weight() {
            return None;
        }
        return self.spans.find_by_weight(pos);
    }

    /// Get the (user_idx, seq) at a visible position.
    fn id_at_pos(&self, pos: u64) -> Option<(UserIdx, u32)> {
        let (span_idx, offset) = self.find_span_at_pos(pos)?;
        let span = self.spans.get(span_idx)?;
        return Some((span.user_idx, span.seq + offset as u32));
    }

    /// Calculate total visible length.
    fn calculate_len(&self) -> u64 {
        return self.spans.total_weight();
    }

    /// Insert a span using YATA ordering rules.
    fn insert_span(&mut self, span: Span) {
        // Track the sequence number
        self.advance_seq(span.user_idx, span.seq + span.len - 1);

        // Empty document: just insert
        if self.spans.is_empty() {
            let weight = span.visible_len();
            self.spans.insert(0, span, weight);
            self.cursor_cache.invalidate();
            return;
        }

        // Find left origin position
        let start_idx = match &span.left_origin {
            None => 0,
            Some((user_idx, seq)) => {
                match self.find_span_by_id(*user_idx, *seq) {
                    Some((idx, offset)) => {
                        let existing = self.spans.get(idx).unwrap();
                        if offset < existing.len - 1 {
                            // Need to split: origin is not at the end of the span
                            self.split_span_at(idx, offset + 1);
                            idx + 1
                        } else {
                            idx + 1
                        }
                    }
                    None => 0,
                }
            }
        };

        // Find right origin position (the boundary we cannot cross)
        let end_idx = match &span.right_origin {
            None => self.spans.len(),
            Some((user_idx, seq)) => {
                match self.find_span_by_id(*user_idx, *seq) {
                    Some((idx, offset)) => {
                        if offset > 0 {
                            self.split_span_at(idx, offset);
                            idx + 1
                        } else {
                            idx
                        }
                    }
                    None => self.spans.len(),
                }
            }
        };

        // YATA conflict resolution
        let mut insert_idx = start_idx;

        while insert_idx < end_idx {
            let existing = self.spans.get(insert_idx).unwrap();
            let existing_left_origin = existing.left_origin;

            let same_left_origin = span.left_origin == existing_left_origin;

            if same_left_origin {
                let order = self.yata_compare(&span, existing);
                match order {
                    Ordering::Less => break,
                    Ordering::Greater => insert_idx += 1,
                    Ordering::Equal => return, // Same span
                }
            } else {
                if self.origin_precedes(&existing_left_origin, &span.left_origin) {
                    insert_idx += 1;
                } else {
                    break;
                }
            }
        }

        // Insert at the determined position
        let weight = span.visible_len();
        self.spans.insert(insert_idx, span, weight);
        self.cursor_cache.invalidate();
    }

    /// Split a span at the given offset.
    fn split_span_at(&mut self, idx: usize, offset: u32) {
        let span = self.spans.get_mut(idx).unwrap();
        let right = span.split(offset);
        
        // Update weight of left part
        let left_weight = span.visible_len();
        self.spans.update_weight(idx, left_weight);
        
        // Insert right part
        let right_weight = right.visible_len();
        self.spans.insert(idx + 1, right, right_weight);
        self.cursor_cache.invalidate();
    }

    /// YATA comparison for spans with the same left origin.
    fn yata_compare(&self, new_span: &Span, existing: &Span) -> Ordering {
        let new_has_ro = new_span.right_origin.is_some();
        let existing_has_ro = existing.right_origin.is_some();

        // Rule 1: Compare right origins
        if new_has_ro != existing_has_ro {
            if new_has_ro && !existing_has_ro {
                return Ordering::Less; // new comes first
            } else {
                return Ordering::Greater; // existing comes first
            }
        }

        // Both have right origins - compare
        if new_has_ro && existing_has_ro {
            let new_ro = new_span.right_origin.unwrap();
            let existing_ro = existing.right_origin.unwrap();
            
            let new_ro_key = self.users.get_id(new_ro.0);
            let existing_ro_key = self.users.get_id(existing_ro.0);
            
            match (new_ro_key, existing_ro_key) {
                (Some(new_k), Some(ex_k)) => {
                    let new_ro_full = (new_k, new_ro.1);
                    let existing_ro_full = (ex_k, existing_ro.1);
                    match new_ro_full.cmp(&existing_ro_full) {
                        Ordering::Greater => return Ordering::Less,
                        Ordering::Less => return Ordering::Greater,
                        Ordering::Equal => {}
                    }
                }
                _ => {}
            }
        }

        // Rule 2: Tiebreaker - compare (KeyPub, seq)
        let new_key_pub = self.users.get_id(new_span.user_idx);
        let existing_key_pub = self.users.get_id(existing.user_idx);
        
        match (new_key_pub, existing_key_pub) {
            (Some(new_k), Some(ex_k)) => {
                let new_key = (new_k, new_span.seq);
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
    fn origin_precedes(
        &self,
        origin_a: &Option<(UserIdx, u32)>,
        origin_b: &Option<(UserIdx, u32)>,
    ) -> bool {
        match (origin_a, origin_b) {
            (None, _) => true,
            (_, None) => false,
            (Some((ua, sa)), Some((ub, sb))) => {
                let pos_a = self.find_span_by_id(*ua, *sa);
                let pos_b = self.find_span_by_id(*ub, *sb);
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
                        // Compare by global ID
                        let key_a = self.users.get_id(*ua);
                        let key_b = self.users.get_id(*ub);
                        match (key_a, key_b) {
                            (Some(ka), Some(kb)) => (ka, sa) < (kb, sb),
                            _ => sa < sb,
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

        while i < self.spans.len() {
            let span = self.spans.get(i).unwrap();

            if span.user_idx != user_idx {
                i += 1;
                continue;
            }

            let span_end = span.seq + span.len;

            if span.seq >= end_seq || span_end <= start_seq {
                i += 1;
                continue;
            }

            let overlap_start = start_seq.max(span.seq);
            let overlap_end = end_seq.min(span_end);

            if overlap_start == span.seq && overlap_end == span_end {
                // Entire span is in the deletion range
                let span = self.spans.get_mut(i).unwrap();
                span.deleted = true;
                self.spans.update_weight(i, 0);
                i += 1;
            } else if overlap_start == span.seq {
                // Deletion covers the prefix
                let split_offset = overlap_end - span.seq;
                self.split_span_at(i, split_offset);
                let span = self.spans.get_mut(i).unwrap();
                span.deleted = true;
                self.spans.update_weight(i, 0);
                i += 2;
            } else if overlap_end == span_end {
                // Deletion covers the suffix
                let split_offset = overlap_start - span.seq;
                self.split_span_at(i, split_offset);
                let right_span = self.spans.get_mut(i + 1).unwrap();
                right_span.deleted = true;
                self.spans.update_weight(i + 1, 0);
                i += 2;
            } else {
                // Deletion is in the middle
                let first_split = overlap_start - span.seq;
                self.split_span_at(i, first_split);
                let second_split = overlap_end - overlap_start;
                self.split_span_at(i + 1, second_split);
                let mid_span = self.spans.get_mut(i + 1).unwrap();
                mid_span.deleted = true;
                self.spans.update_weight(i + 1, 0);
                i += 3;
            }
        }
        self.cursor_cache.invalidate();
    }

    /// Map an origin from another DiamondRga to this one.
    fn map_origin(
        &mut self,
        origin: &Option<(UserIdx, u32)>,
        other: &DiamondRga,
    ) -> Option<(UserIdx, u32)> {
        let (other_user_idx, seq) = (*origin)?;
        let other_user = other.users.get_id(other_user_idx)?;
        let our_user_idx = self.users.get_or_insert(other_user);
        while self.user_content.len() <= our_user_idx.0 as usize {
            self.user_content.push(UserContent::default());
        }
        return Some((our_user_idx, seq));
    }
}

impl Rga for DiamondRga {
    type UserId = KeyPub;

    fn insert(&mut self, user: &Self::UserId, pos: u64, content: &[u8]) {
        if content.is_empty() {
            return;
        }

        self.clock.tick();
        let user_idx = self.ensure_user(user);
        let user_content = &mut self.user_content[user_idx.0 as usize];
        let seq = user_content.next_seq;
        let content_offset = user_content.content.len() as u32;

        // Store content in user's buffer
        user_content.content.extend_from_slice(content);

        // Determine left and right origins
        let doc_len = self.calculate_len();

        let left_origin = if pos == 0 {
            None
        } else {
            self.id_at_pos(pos - 1)
        };

        let right_origin = if pos >= doc_len {
            None
        } else {
            self.id_at_pos(pos)
        };

        let span = Span::new(
            user_idx,
            seq,
            content.len() as u32,
            content_offset,
            left_origin,
            right_origin,
        );

        self.insert_span(span);
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
            let result = self.find_span_at_pos(start);
            let (span_idx, offset) = match result {
                Some(x) => x,
                None => break,
            };

            let span = self.spans.get(span_idx).unwrap();
            let span_visible = span.visible_len();
            let available = span_visible - offset;

            if offset == 0 && remaining >= available {
                // Delete entire span
                let span = self.spans.get_mut(span_idx).unwrap();
                span.deleted = true;
                self.spans.update_weight(span_idx, 0);
                remaining -= available;
            } else if offset == 0 {
                // Delete prefix
                self.split_span_at(span_idx, remaining as u32);
                let span = self.spans.get_mut(span_idx).unwrap();
                span.deleted = true;
                self.spans.update_weight(span_idx, 0);
                remaining = 0;
            } else if remaining >= available {
                // Delete suffix
                self.split_span_at(span_idx, offset as u32);
                let right_span = self.spans.get_mut(span_idx + 1).unwrap();
                right_span.deleted = true;
                self.spans.update_weight(span_idx + 1, 0);
                remaining -= available;
            } else {
                // Delete middle
                self.split_span_at(span_idx, offset as u32);
                self.split_span_at(span_idx + 1, remaining as u32);
                let mid_span = self.spans.get_mut(span_idx + 1).unwrap();
                mid_span.deleted = true;
                self.spans.update_weight(span_idx + 1, 0);
                remaining = 0;
            }
        }
        self.cursor_cache.invalidate();
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

        // Merge spans
        for other_span in other.spans.iter() {
            // Map user index
            let other_user = match other.users.get_id(other_span.user_idx) {
                Some(u) => u,
                None => continue,
            };
            let our_user_idx = self.users.get_or_insert(other_user);
            while self.user_content.len() <= our_user_idx.0 as usize {
                self.user_content.push(UserContent::default());
            }

            // Check if we already have this span (or any character from it)
            let already_have = self.spans.iter().any(|span| {
                span.user_idx == our_user_idx && span.contains(our_user_idx, other_span.seq)
            });

            if already_have {
                // Check if other has it deleted
                if other_span.deleted {
                    self.apply_deletion_range(our_user_idx, other_span.seq, other_span.len);
                }
                continue;
            }

            // Map origins
            let left_origin = self.map_origin(&other_span.left_origin, other);
            let right_origin = self.map_origin(&other_span.right_origin, other);

            let mut span = Span::new(
                our_user_idx,
                other_span.seq,
                other_span.len,
                other_span.content_offset,
                left_origin,
                right_origin,
            );

            if other_span.deleted {
                span.deleted = true;
            }

            self.insert_span(span);
        }
    }

    fn to_string(&self) -> String {
        let mut result = Vec::new();
        for span in self.spans.iter() {
            if !span.deleted {
                let content = self.get_content(span.user_idx, span.content_offset, span.len);
                result.extend_from_slice(content);
            }
        }
        return String::from_utf8(result).unwrap_or_default();
    }

    fn len(&self) -> u64 {
        return self.calculate_len();
    }

    fn span_count(&self) -> usize {
        return self.spans.len();
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
        let rga = DiamondRga::new();
        assert_eq!(rga.len(), 0);
        assert_eq!(rga.to_string(), "");
    }

    #[test]
    fn insert_at_beginning() {
        let mut rga = DiamondRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello");
        assert_eq!(rga.to_string(), "hello");
        assert_eq!(rga.len(), 5);
    }

    #[test]
    fn insert_at_end() {
        let mut rga = DiamondRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello");
        rga.insert(&user, 5, b" world");
        assert_eq!(rga.to_string(), "hello world");
    }

    #[test]
    fn insert_in_middle() {
        let mut rga = DiamondRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hd");
        rga.insert(&user, 1, b"ello worl");
        assert_eq!(rga.to_string(), "hello world");
    }

    #[test]
    fn delete_range() {
        let mut rga = DiamondRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello world");
        rga.delete(5, 6);
        assert_eq!(rga.to_string(), "hello");
    }

    #[test]
    fn delete_middle() {
        let mut rga = DiamondRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello");
        rga.delete(1, 3); // Delete "ell"
        assert_eq!(rga.to_string(), "ho");
    }

    #[test]
    fn merge_simple() {
        let user1 = make_user();
        let user2 = make_user();

        let mut a = DiamondRga::new();
        let mut b = DiamondRga::new();

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
        let mut rga = DiamondRga::new();
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

        let mut a = DiamondRga::new();
        let mut b = DiamondRga::new();

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

        let mut a = DiamondRga::new();
        let mut b = DiamondRga::new();
        let mut c = DiamondRga::new();

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

        let mut base = DiamondRga::new();
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

        let mut a = DiamondRga::new();
        a.insert(&user, 0, b"hello");

        let mut b = a.clone();
        b.delete(1, 3);

        a.merge(&b);

        assert_eq!(a.to_string(), "ho");
    }

    #[test]
    fn span_count_increases_with_splits() {
        let user = make_user();
        let mut rga = DiamondRga::new();

        rga.insert(&user, 0, b"hello");
        let count_before = rga.span_count();

        rga.delete(2, 1); // This should split the span
        let count_after = rga.span_count();

        assert!(count_after > count_before);
    }
}
