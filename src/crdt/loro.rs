// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

//! Loro-style RGA using the Fugue algorithm.
//!
//! This implementation is inspired by the Loro library, which uses the Fugue
//! algorithm for text sequences. Fugue minimizes interleaving in concurrent
//! edits through a dual-origin approach.
//!
//! # Fugue Algorithm
//!
//! The key insight of Fugue is that each character stores two origins:
//! - `origin_left`: The ID of the character immediately to the left when inserted
//! - `origin_right`: The ID of the character immediately to the right when inserted
//!
//! When concurrent inserts happen at the same position, Fugue first resolves
//! conflicts using `origin_left`. If there is still ambiguity (same left origin),
//! it uses `origin_right` to break ties. This prevents interleaving of concurrent
//! text passages.
//!
//! # Architecture
//!
//! - Uses B-tree based storage for O(log n) operations
//! - Run-length encoding / span coalescing for memory efficiency
//! - Separates content storage from CRDT metadata
//!
//! # Example
//!
//! ```
//! use together::crdt::loro::LoroRga;
//! use together::crdt::rga_trait::Rga;
//! use together::key::KeyPair;
//!
//! let user = KeyPair::generate();
//! let mut doc = LoroRga::new();
//!
//! doc.insert(&user.key_pub, 0, b"Hello");
//! doc.insert(&user.key_pub, 5, b" World");
//! assert_eq!(doc.to_string(), "Hello World");
//!
//! doc.delete(5, 6);
//! assert_eq!(doc.to_string(), "Hello");
//! ```

use crate::key::KeyPub;
use super::btree_list::BTreeList;
use super::primitives::{UserTable, LamportClock, UserIdx};
use super::rga_trait::Rga;

// =============================================================================
// Span
// =============================================================================

/// A span of consecutive characters from one user.
///
/// Loro uses span coalescing to reduce memory usage. Consecutive characters
/// from the same user with compatible origins are merged into single spans.
///
/// Each span stores:
/// - User and sequence range
/// - Left and right origins (for Fugue conflict resolution)
/// - Content offset (into user's content buffer)
/// - Deletion state (using delete counter, not tombstone)
#[derive(Clone, Debug)]
struct Span {
    /// User who created this span.
    user_idx: UserIdx,
    /// Starting sequence number.
    seq: u32,
    /// Number of characters in this span.
    len: u32,
    /// Left origin: what was to the left when this was inserted.
    /// None means inserted at the beginning.
    origin_left: Option<(UserIdx, u32)>,
    /// Right origin: what was to the right when this was inserted.
    /// None means inserted at the end.
    origin_right: Option<(UserIdx, u32)>,
    /// Whether this span is deleted.
    /// Loro uses delete counters, but for simplicity we use a boolean.
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
        origin_left: Option<(UserIdx, u32)>,
        origin_right: Option<(UserIdx, u32)>,
    ) -> Span {
        return Span {
            user_idx,
            seq,
            len,
            origin_left,
            origin_right,
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
            origin_left: Some((self.user_idx, self.seq + offset - 1)),
            // Right origin stays the same
            origin_right: self.origin_right,
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
/// Loro separates content from CRDT metadata. Each user has their own
/// content buffer, and spans reference offsets into this buffer.
#[derive(Clone, Debug, Default)]
struct UserContent {
    /// The content bytes inserted by this user.
    content: Vec<u8>,
    /// Next sequence number to assign.
    next_seq: u32,
}

// =============================================================================
// LoroRga
// =============================================================================

/// Loro-style RGA using the Fugue algorithm.
///
/// Uses a B-tree for O(log n) position lookups, run-length encoded spans,
/// and separated content storage.
#[derive(Clone, Debug)]
pub struct LoroRga {
    /// Spans in document order, stored in a B-tree weighted by visible length.
    spans: BTreeList<Span>,
    /// User table mapping KeyPub to UserIdx.
    users: UserTable<KeyPub>,
    /// Per-user content storage.
    user_content: Vec<UserContent>,
    /// Lamport clock for ordering.
    clock: LamportClock,
}

impl Default for LoroRga {
    fn default() -> Self {
        return Self::new();
    }
}

impl LoroRga {
    /// Create a new empty LoroRga.
    pub fn new() -> LoroRga {
        return LoroRga {
            spans: BTreeList::new(),
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

    /// Insert a span using Fugue ordering rules.
    ///
    /// The Fugue algorithm:
    /// 1. Find the left origin's position
    /// 2. Scan right through potential conflicts
    /// 3. Apply Fugue conflict resolution rules:
    ///    a. First compare by origin_left
    ///    b. If same origin_left, compare by origin_right
    ///    c. If same origins, use (peer_id, seq) as tiebreaker
    /// 4. Insert at the determined position
    fn insert_span(&mut self, span: Span) {
        // Track the sequence number
        self.advance_seq(span.user_idx, span.seq + span.len - 1);

        // Empty document: just insert
        if self.spans.is_empty() {
            let weight = span.visible_len();
            self.spans.insert(0, span, weight);
            return;
        }

        // Find left origin position
        let start_idx = match &span.origin_left {
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
        let end_idx = match &span.origin_right {
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

        // Fugue conflict resolution: scan through items between start_idx and end_idx
        //
        // The Fugue algorithm prevents interleaving by considering where existing items
        // were inserted relative to our insertion point. The key insight:
        //
        // We scan forward from our left origin. For each existing item:
        // - If same origin_left: compare origin_right positions, then ID
        // - If different origin_left: check if existing's origin_left is "between"
        //   our origin_left and origin_right (meaning it was inserted into our gap)
        //
        // An item whose origin_left points to something BETWEEN our origin_left and
        // origin_right was inserted "into" the gap we're also inserting into, and
        // forms a subtree. We need to skip over entire subtrees.
        let mut insert_idx = start_idx;
        let mut scanning = true;

        while insert_idx < end_idx && scanning {
            let existing = self.spans.get(insert_idx).unwrap();
            let existing_left_origin = existing.origin_left;
            let existing_right_origin = existing.origin_right;

            if span.origin_left == existing_left_origin {
                // Same left origin - this is a sibling insertion
                // Use origin_right to determine order
                
                if span.origin_right == existing_right_origin {
                    // Same left AND right origins - use ID as final tiebreaker
                    // Higher ID comes first (arbitrary but consistent)
                    let new_key = self.users.get_id(span.user_idx);
                    let existing_key = self.users.get_id(existing.user_idx);
                    match (new_key, existing_key) {
                        (Some(new_k), Some(ex_k)) => {
                            if (ex_k, existing.seq) > (new_k, span.seq) {
                                // existing has higher ID, insert before it
                                scanning = false;
                            } else {
                                // we have higher ID, continue scanning
                                insert_idx += 1;
                            }
                        }
                        _ => insert_idx += 1,
                    }
                } else {
                    // Different right origins - compare their positions
                    // The item with the LEFTWARD right origin should be placed FIRST
                    // (it was inserted into a "tighter" gap)
                    //
                    // None = no right origin = inserted at document end = rightmost position
                    let existing_ro_precedes = match (&existing_right_origin, &span.origin_right) {
                        (None, None) => false, // Both at end, equal
                        (None, Some(_)) => false, // Existing at end, ours is left of that
                        (Some(_), None) => true, // Existing has finite position, ours at end
                        (Some(ex_ro), Some(new_ro)) => {
                            self.origin_precedes(&Some(*ex_ro), &Some(*new_ro))
                        }
                    };
                    
                    if existing_ro_precedes {
                        // Existing's right origin is more leftward = tighter gap
                        // Existing comes first, continue scanning
                        insert_idx += 1;
                    } else {
                        // Our right origin is more leftward or equal
                        // Check if equal - if so, use ID tiebreaker
                        let origins_equal = existing_right_origin == span.origin_right;
                        if origins_equal {
                            let new_key = self.users.get_id(span.user_idx);
                            let existing_key = self.users.get_id(existing.user_idx);
                            match (new_key, existing_key) {
                                (Some(new_k), Some(ex_k)) => {
                                    if (ex_k, existing.seq) > (new_k, span.seq) {
                                        scanning = false;
                                    } else {
                                        insert_idx += 1;
                                    }
                                }
                                _ => insert_idx += 1,
                            }
                        } else {
                            // Our right origin is strictly more leftward
                            // We come first
                            scanning = false;
                        }
                    }
                }
            } else {
                // Different left origin
                //
                // Key Fugue insight: if existing's origin_left is "between" our
                // origin_left and origin_right, then existing was inserted into
                // a child subtree and we should skip over it (continue scanning).
                //
                // "Between" means: existing's origin_left is at a position that is
                // after our origin_left but before (or at) our origin_right.
                //
                // If existing's origin_left is at or before our origin_left,
                // then existing is a sibling or ancestor - stop scanning.
                
                let existing_origin_after_ours = !self.origin_precedes(&existing_left_origin, &span.origin_left)
                    && existing_left_origin != span.origin_left;
                
                if existing_origin_after_ours {
                    // Existing's origin_left is AFTER our origin_left
                    // This means existing was inserted into a position that came after
                    // where we're inserting - it belongs to a child subtree
                    // 
                    // But we also need to check: is it before our origin_right?
                    // If existing's origin_left is at or after our origin_right,
                    // then it's NOT in our subtree - we should stop.
                    let in_our_subtree = match &span.origin_right {
                        None => true, // No right boundary, everything after is in subtree
                        Some(ro) => {
                            // Check if existing's origin_left is before our origin_right
                            self.origin_precedes(&existing_left_origin, &Some(*ro))
                        }
                    };
                    
                    if in_our_subtree {
                        // Existing is in a child subtree - skip over it
                        insert_idx += 1;
                    } else {
                        // Existing is past our right boundary - stop
                        scanning = false;
                    }
                } else {
                    // Existing's origin_left is at or before our origin_left
                    // This is a sibling or ancestor - stop scanning
                    scanning = false;
                }
            }
        }

        // Insert at the determined position
        let weight = span.visible_len();
        self.spans.insert(insert_idx, span, weight);
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
    }

    /// Check if origin_a precedes origin_b in document order.
    ///
    /// This is used when items have different left origins to determine
    /// their relative order.
    fn origin_precedes(
        &self,
        origin_a: &Option<(UserIdx, u32)>,
        origin_b: &Option<(UserIdx, u32)>,
    ) -> bool {
        match (origin_a, origin_b) {
            (None, _) => true, // Beginning precedes everything
            (_, None) => false, // Nothing precedes beginning
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
                    (None, Some(_)) => true, // Missing origin treated as beginning
                    (Some(_), None) => false,
                    (None, None) => {
                        // Both origins not found - compare by global ID
                        let key_a = self.users.get_id(*ua);
                        let key_b = self.users.get_id(*ub);
                        match (key_a, key_b) {
                            (Some(ka), Some(kb)) => (ka, sa) < (kb, sb),
                            _ => sa < sb, // Fallback
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
    }

    /// Map an origin from another LoroRga to this one.
    fn map_origin(
        &mut self,
        origin: &Option<(UserIdx, u32)>,
        other: &LoroRga,
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

impl Rga for LoroRga {
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

        // Determine left and right origins based on position
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
            let left_origin = self.map_origin(&other_span.origin_left, other);
            let right_origin = self.map_origin(&other_span.origin_right, other);

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
        let rga = LoroRga::new();
        assert_eq!(rga.len(), 0);
        assert_eq!(rga.to_string(), "");
    }

    #[test]
    fn insert_at_beginning() {
        let mut rga = LoroRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello");
        assert_eq!(rga.to_string(), "hello");
        assert_eq!(rga.len(), 5);
    }

    #[test]
    fn insert_at_end() {
        let mut rga = LoroRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello");
        rga.insert(&user, 5, b" world");
        assert_eq!(rga.to_string(), "hello world");
    }

    #[test]
    fn insert_in_middle() {
        let mut rga = LoroRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hd");
        rga.insert(&user, 1, b"ello worl");
        assert_eq!(rga.to_string(), "hello world");
    }

    #[test]
    fn delete_range() {
        let mut rga = LoroRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello world");
        rga.delete(5, 6);
        assert_eq!(rga.to_string(), "hello");
    }

    #[test]
    fn delete_middle() {
        let mut rga = LoroRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello");
        rga.delete(1, 3); // Delete "ell"
        assert_eq!(rga.to_string(), "ho");
    }

    #[test]
    fn merge_simple() {
        let user1 = make_user();
        let user2 = make_user();

        let mut a = LoroRga::new();
        let mut b = LoroRga::new();

        a.insert(&user1, 0, b"A");
        b.insert(&user2, 0, b"B");

        let mut ab = a.clone();
        ab.merge(&b);

        let mut ba = b.clone();
        ba.merge(&a);

        // Both should have the same result (commutativity)
        assert_eq!(ab.to_string(), ba.to_string());
        assert_eq!(ab.len(), 2);
    }

    #[test]
    fn merge_idempotent() {
        let user = make_user();
        let mut rga = LoroRga::new();
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

        let mut a = LoroRga::new();
        let mut b = LoroRga::new();

        // Both insert at position 0
        a.insert(&user1, 0, b"A");
        b.insert(&user2, 0, b"B");

        let mut ab = a.clone();
        ab.merge(&b);

        let mut ba = b.clone();
        ba.merge(&a);

        // Should be commutative
        assert_eq!(ab.to_string(), ba.to_string());
    }

    #[test]
    fn merge_associative() {
        let user1 = make_user();
        let user2 = make_user();
        let user3 = make_user();

        let mut a = LoroRga::new();
        let mut b = LoroRga::new();
        let mut c = LoroRga::new();

        a.insert(&user1, 0, b"A");
        b.insert(&user2, 0, b"B");
        c.insert(&user3, 0, b"C");

        // (a merge (b merge c))
        let mut bc = b.clone();
        bc.merge(&c);
        let mut a_bc = a.clone();
        a_bc.merge(&bc);

        // ((a merge b) merge c)
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

        // Start with shared base "ac"
        let mut base = LoroRga::new();
        base.insert(&user1, 0, b"ac");

        let mut a = base.clone();
        let mut b = base.clone();

        // Both insert between 'a' and 'c' (position 1)
        a.insert(&user1, 1, b"b");
        b.insert(&user2, 1, b"x");

        let mut ab = a.clone();
        ab.merge(&b);

        let mut ba = b.clone();
        ba.merge(&a);

        // Should be commutative
        assert_eq!(ab.to_string(), ba.to_string());

        // Both 'b' and 'x' should be between 'a' and 'c'
        let result = ab.to_string();
        assert!(result.starts_with("a"));
        assert!(result.ends_with("c"));
        assert!(result.contains("b"));
        assert!(result.contains("x"));
    }

    #[test]
    fn delete_propagates_through_merge() {
        let user = make_user();

        let mut a = LoroRga::new();
        a.insert(&user, 0, b"hello");

        let mut b = a.clone();
        b.delete(1, 3); // Delete "ell"

        a.merge(&b);

        assert_eq!(a.to_string(), "ho");
    }

    #[test]
    fn span_count_increases_with_splits() {
        let user = make_user();
        let mut rga = LoroRga::new();

        rga.insert(&user, 0, b"hello");
        let count_before = rga.span_count();

        rga.delete(2, 1); // This should split the span
        let count_after = rga.span_count();

        assert!(count_after > count_before);
    }

    #[test]
    fn fugue_prevents_interleaving_with_shared_base() {
        // Test that Fugue algorithm prevents character interleaving
        // when there is a shared base document.
        //
        // The Fugue guarantee: When two users concurrently insert text
        // at the same position in a SHARED document, their text passages
        // will NOT interleave character-by-character.
        //
        // Note: When two users type into completely separate empty documents
        // (no shared base), interleaving can still occur because each user's
        // characters have origins relative to their own local insertions.
        
        let user1 = make_user();
        let user2 = make_user();

        // Start with a shared base document
        let mut base = LoroRga::new();
        base.insert(&user1, 0, b"[]");  // Shared base: "[]"
        
        let mut a = base.clone();
        let mut b = base.clone();

        // Both users type between '[' and ']' (position 1)
        // User1 types "Hello" character by character
        for (i, c) in "Hello".bytes().enumerate() {
            a.insert(&user1, 1 + i as u64, &[c]);
        }

        // User2 types "World" character by character at the same position
        for (i, c) in "World".bytes().enumerate() {
            b.insert(&user2, 1 + i as u64, &[c]);
        }

        let mut merged = a.clone();
        merged.merge(&b);

        let result = merged.to_string();
        
        // The result should have Hello and World as contiguous blocks
        // (either "[HelloWorld]" or "[WorldHello]")
        // NOT something interleaved like "[HWeolrllod]"
        assert!(
            result == "[HelloWorld]" || result == "[WorldHello]",
            "Expected non-interleaved result, got: {}",
            result
        );
    }
}
