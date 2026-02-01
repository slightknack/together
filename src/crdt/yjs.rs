// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

//! Yjs-style RGA using the YATA algorithm.
//!
//! YATA (Yet Another Transformation Approach) is the algorithm used by yjs,
//! one of the most widely deployed CRDTs. The key insight is dual origins:
//! each item stores both its left origin (what it was inserted after) and
//! right origin (what was to its right when it was inserted).
//!
//! This dual-origin approach prevents interleaving in concurrent edits.
//!
//! # Example
//!
//! ```
//! use together::crdt::yjs::YjsRga;
//! use together::crdt::rga_trait::Rga;
//! use together::key::KeyPair;
//!
//! let user = KeyPair::generate();
//! let mut doc = YjsRga::new();
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
// Item ID
// =============================================================================

/// A unique identifier for an item.
///
/// In YATA, each item is identified by (user_idx, seq). The seq is the
/// starting sequence number; for multi-character items, the item spans
/// seq..seq+len.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ItemId {
    user_idx: UserIdx,
    seq: u32,
}

impl ItemId {
    fn new(user_idx: UserIdx, seq: u32) -> ItemId {
        return ItemId { user_idx, seq };
    }

    fn none() -> ItemId {
        return ItemId {
            user_idx: UserIdx::NONE,
            seq: 0,
        };
    }

    fn is_none(&self) -> bool {
        return self.user_idx.is_none();
    }
}

impl PartialOrd for ItemId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        return Some(self.cmp(other));
    }
}

impl Ord for ItemId {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.user_idx.cmp(&other.user_idx) {
            Ordering::Equal => self.seq.cmp(&other.seq),
            other => other,
        }
    }
}

// =============================================================================
// Item
// =============================================================================

/// An item in the YATA linked list.
///
/// Each item represents a contiguous run of characters inserted by the same
/// user. Items form a doubly-linked list in document order.
///
/// The key YATA insight is dual origins:
/// - `left_origin`: The item that was to the left when this was inserted
/// - `right_origin`: The item that was to the right when this was inserted
///
/// These origins are immutable and capture the insertion context, enabling
/// consistent conflict resolution during merge.
#[derive(Clone, Debug)]
struct Item {
    /// User who created this item.
    user_idx: UserIdx,
    /// Starting sequence number.
    seq: u32,
    /// Number of characters in this item.
    len: u32,

    /// Left origin: what was to the left when this was inserted.
    /// ItemId::none() means inserted at the beginning.
    left_origin: ItemId,
    /// Right origin: what was to the right when this was inserted.
    /// ItemId::none() means inserted at the end.
    right_origin: ItemId,

    /// Content bytes (stored inline for simplicity).
    content: Vec<u8>,
    /// Whether this item has been deleted (tombstone).
    deleted: bool,
}

impl Item {
    fn new(
        user_idx: UserIdx,
        seq: u32,
        content: Vec<u8>,
        left_origin: ItemId,
        right_origin: ItemId,
    ) -> Item {
        let len = content.len() as u32;
        return Item {
            user_idx,
            seq,
            len,
            left_origin,
            right_origin,
            content,
            deleted: false,
        };
    }

    /// Check if this item contains the given (user_idx, seq).
    fn contains(&self, user_idx: UserIdx, seq: u32) -> bool {
        return self.user_idx == user_idx && seq >= self.seq && seq < self.seq + self.len;
    }

    /// Get the visible length (0 if deleted).
    fn visible_len(&self) -> u32 {
        if self.deleted {
            return 0;
        }
        return self.len;
    }

    /// Split this item at the given offset, returning the right part.
    ///
    /// After split:
    /// - self contains [0, offset)
    /// - returned item contains [offset, len)
    ///
    /// The right part's left_origin becomes the last char of the left part.
    /// Both parts keep the original right_origin (insertion-time property).
    fn split(&mut self, offset: u32) -> Item {
        debug_assert!(offset > 0 && offset < self.len);

        let right_content = self.content[offset as usize..].to_vec();
        let right = Item {
            user_idx: self.user_idx,
            seq: self.seq + offset,
            len: self.len - offset,
            // Right part's left origin is the last char of the left part
            left_origin: ItemId::new(self.user_idx, self.seq + offset - 1),
            // Keep original right origin
            right_origin: self.right_origin,
            content: right_content,
            deleted: self.deleted,
        };

        self.len = offset;
        self.content.truncate(offset as usize);

        return right;
    }
}

// =============================================================================
// Per-user state
// =============================================================================

/// Per-user state tracking the next sequence number.
#[derive(Clone, Debug, Default)]
struct UserState {
    /// Next sequence number to assign for this user.
    next_seq: u32,
}

// =============================================================================
// YjsRga
// =============================================================================

/// Yjs-style RGA using the YATA algorithm.
///
/// Uses a simple Vec of Items for storage. This is O(n) for random access
/// but correct, which is the priority for this implementation.
#[derive(Clone, Debug)]
pub struct YjsRga {
    /// Items in document order.
    items: Vec<Item>,
    /// User table mapping KeyPub to UserIdx.
    users: UserTable<KeyPub>,
    /// Per-user state (next_seq).
    user_states: Vec<UserState>,
    /// Lamport clock for ordering.
    clock: LamportClock,
}

impl Default for YjsRga {
    fn default() -> Self {
        return Self::new();
    }
}

impl YjsRga {
    /// Create a new empty YjsRga.
    pub fn new() -> YjsRga {
        return YjsRga {
            items: Vec::new(),
            users: UserTable::new(),
            user_states: Vec::new(),
            clock: LamportClock::new(),
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

    /// Find the item index containing the given ItemId.
    /// Returns (item_index, offset_within_item).
    fn find_item_by_id(&self, id: ItemId) -> Option<(usize, u32)> {
        if id.is_none() {
            return None;
        }
        for (i, item) in self.items.iter().enumerate() {
            if item.contains(id.user_idx, id.seq) {
                let offset = id.seq - item.seq;
                return Some((i, offset));
            }
        }
        return None;
    }

    /// Find the item at a visible position.
    /// Returns (item_index, offset_within_item).
    fn find_item_at_pos(&self, pos: u64) -> Option<(usize, u32)> {
        let mut current_pos: u64 = 0;
        for (i, item) in self.items.iter().enumerate() {
            let visible = item.visible_len() as u64;
            if visible == 0 {
                continue;
            }
            if current_pos + visible > pos {
                let offset = (pos - current_pos) as u32;
                return Some((i, offset));
            }
            current_pos += visible;
        }
        return None;
    }

    /// Get the ItemId at a visible position.
    fn id_at_pos(&self, pos: u64) -> Option<ItemId> {
        let (idx, offset) = self.find_item_at_pos(pos)?;
        let item = &self.items[idx];
        return Some(ItemId::new(item.user_idx, item.seq + offset));
    }

    /// Insert an item using YATA ordering rules.
    ///
    /// This is the core YATA algorithm:
    /// 1. Find the left origin's position
    /// 2. Scan right through potential conflicts
    /// 3. Apply YATA conflict resolution rules
    /// 4. Insert at the determined position
    fn insert_item(&mut self, item: Item) {
        // Track the sequence number
        self.advance_seq(item.user_idx, item.seq + item.len - 1);

        // Empty document: just insert
        if self.items.is_empty() {
            self.items.push(item);
            return;
        }

        // Find left origin position
        let start_idx = if item.left_origin.is_none() {
            0
        } else {
            match self.find_item_by_id(item.left_origin) {
                Some((idx, offset)) => {
                    // If origin is in the middle of an item, we need to split
                    let origin_item = &self.items[idx];
                    if offset < origin_item.len - 1 {
                        // Need to split: origin is not at the end of the item
                        let split_offset = offset + 1;
                        let right = self.items[idx].split(split_offset);
                        self.items.insert(idx + 1, right);
                        idx + 1
                    } else {
                        // Origin is at the end of the item
                        idx + 1
                    }
                }
                None => {
                    // Origin not found - insert at beginning
                    0
                }
            }
        };

        // Find right origin position (the boundary we cannot cross)
        let end_idx = if item.right_origin.is_none() {
            self.items.len()
        } else {
            match self.find_item_by_id(item.right_origin) {
                Some((idx, offset)) => {
                    // If right_origin is in the middle, split at that position
                    if offset > 0 {
                        let right = self.items[idx].split(offset);
                        self.items.insert(idx + 1, right);
                        idx + 1
                    } else {
                        idx
                    }
                }
                None => self.items.len(),
            }
        };

        // YATA conflict resolution: scan through items between start_idx and end_idx
        let mut insert_idx = start_idx;

        while insert_idx < end_idx {
            let existing = &self.items[insert_idx];

            // Check if this existing item is a sibling (same left origin)
            let same_left_origin = existing.left_origin == item.left_origin;

            if same_left_origin {
                // Case 1: Same left origin - compare right origins and IDs
                let order = self.yata_compare(&item, existing);
                match order {
                    Ordering::Less => {
                        // New item comes before existing
                        break;
                    }
                    Ordering::Greater => {
                        // New item comes after existing, continue scanning
                        insert_idx += 1;
                    }
                    Ordering::Equal => {
                        // Same item (shouldn't happen in normal operation)
                        return;
                    }
                }
            } else {
                // Case 2: Different left origin
                // Check if existing's origin is between our left_origin and us
                // If so, existing was inserted into a subtree that started before us
                if self.origin_precedes(&existing.left_origin, &item.left_origin) {
                    // existing's origin is before our origin - existing comes first
                    insert_idx += 1;
                } else {
                    // existing's origin is at or after our origin - we come first
                    break;
                }
            }
        }

        // Insert at the determined position
        self.items.insert(insert_idx, item);
    }

    /// YATA comparison for items with the same left origin.
    ///
    /// Returns:
    /// - Less: new item should come BEFORE existing
    /// - Greater: new item should come AFTER existing
    /// - Equal: same item
    fn yata_compare(&self, new_item: &Item, existing: &Item) -> Ordering {
        let new_has_ro = !new_item.right_origin.is_none();
        let existing_has_ro = !existing.right_origin.is_none();

        // Rule 1: Compare right origins
        // No right origin (null) = "inserted at end" = infinity
        // Finite < infinity, so item with right_origin comes first
        if new_has_ro != existing_has_ro {
            if new_has_ro && !existing_has_ro {
                // new has finite right origin, existing has infinity
                return Ordering::Less; // new comes first
            } else {
                // new has infinity, existing has finite
                return Ordering::Greater; // existing comes first
            }
        }

        // Both have right origins - compare using the actual KeyPub (globally consistent)
        if new_has_ro && existing_has_ro {
            let new_ro_key = self.users.get_id(new_item.right_origin.user_idx);
            let existing_ro_key = self.users.get_id(existing.right_origin.user_idx);
            match (new_ro_key, existing_ro_key) {
                (Some(new_k), Some(ex_k)) => {
                    let new_ro = (new_k, new_item.right_origin.seq);
                    let existing_ro = (ex_k, existing.right_origin.seq);
                    match new_ro.cmp(&existing_ro) {
                        Ordering::Greater => return Ordering::Less, // Higher RO = comes first
                        Ordering::Less => return Ordering::Greater,
                        Ordering::Equal => {} // Fall through to tiebreaker
                    }
                }
                _ => {} // Fall through if can't resolve
            }
        }

        // Rule 2: Tiebreaker - compare (KeyPub, seq)
        // Use the actual KeyPub for globally consistent ordering
        let new_key_pub = self.users.get_id(new_item.user_idx);
        let existing_key_pub = self.users.get_id(existing.user_idx);
        match (new_key_pub, existing_key_pub) {
            (Some(new_k), Some(ex_k)) => {
                let new_key = (new_k, new_item.seq);
                let existing_key = (ex_k, existing.seq);
                match new_key.cmp(&existing_key) {
                    Ordering::Greater => Ordering::Less, // Higher key = comes first
                    Ordering::Less => Ordering::Greater,
                    Ordering::Equal => Ordering::Equal,
                }
            }
            _ => Ordering::Equal, // Can't compare, treat as equal
        }
    }

    /// Check if origin_a precedes origin_b in document order.
    ///
    /// This is used for Case 2 in YATA: when items have different left origins,
    /// we need to determine their relative order.
    fn origin_precedes(&self, origin_a: &ItemId, origin_b: &ItemId) -> bool {
        if origin_a.is_none() {
            return true; // Beginning precedes everything
        }
        if origin_b.is_none() {
            return false; // Nothing precedes beginning
        }

        // Find positions of both origins
        let pos_a = self.find_item_by_id(*origin_a);
        let pos_b = self.find_item_by_id(*origin_b);

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
                // Both origins not found - compare by global ID (KeyPub, seq)
                let key_a = self.users.get_id(origin_a.user_idx);
                let key_b = self.users.get_id(origin_b.user_idx);
                match (key_a, key_b) {
                    (Some(ka), Some(kb)) => (ka, origin_a.seq) < (kb, origin_b.seq),
                    _ => origin_a.seq < origin_b.seq, // Fallback
                }
            }
        }
    }

    /// Calculate the total visible length.
    fn calculate_len(&self) -> u64 {
        let mut len: u64 = 0;
        for item in &self.items {
            len += item.visible_len() as u64;
        }
        return len;
    }
}

impl Rga for YjsRga {
    type UserId = KeyPub;

    fn insert(&mut self, user: &Self::UserId, pos: u64, content: &[u8]) {
        if content.is_empty() {
            return;
        }

        self.clock.tick();
        let user_idx = self.ensure_user(user);
        let seq = self.user_states[user_idx.0 as usize].next_seq;

        // Determine left and right origins based on position
        let doc_len = self.calculate_len();

        let left_origin = if pos == 0 {
            ItemId::none()
        } else {
            // Origin is the character at pos-1
            self.id_at_pos(pos - 1).unwrap_or(ItemId::none())
        };

        let right_origin = if pos >= doc_len {
            ItemId::none()
        } else {
            // Right origin is the character at pos (what will be pushed right)
            self.id_at_pos(pos).unwrap_or(ItemId::none())
        };

        let item = Item::new(
            user_idx,
            seq,
            content.to_vec(),
            left_origin,
            right_origin,
        );

        self.insert_item(item);
    }

    fn delete(&mut self, start: u64, len: u64) {
        if len == 0 {
            return;
        }

        self.clock.tick();
        let mut remaining = len;
        let current_pos = start;

        while remaining > 0 {
            let (item_idx, offset) = match self.find_item_at_pos(current_pos) {
                Some(x) => x,
                None => break,
            };

            let item = &self.items[item_idx];
            let item_visible = item.visible_len();
            let available = item_visible - offset;

            if offset == 0 && remaining >= available as u64 {
                // Delete entire item
                self.items[item_idx].deleted = true;
                remaining -= available as u64;
            } else if offset == 0 {
                // Delete prefix - split and delete left part
                let right = self.items[item_idx].split(remaining as u32);
                self.items[item_idx].deleted = true;
                self.items.insert(item_idx + 1, right);
                remaining = 0;
            } else if remaining >= available as u64 {
                // Delete suffix - split and delete right part
                let mut right = self.items[item_idx].split(offset);
                right.deleted = true;
                self.items.insert(item_idx + 1, right);
                remaining -= available as u64;
            } else {
                // Delete middle - split twice
                let mut mid = self.items[item_idx].split(offset);
                let right = mid.split(remaining as u32);
                mid.deleted = true;
                self.items.insert(item_idx + 1, mid);
                self.items.insert(item_idx + 2, right);
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

        // Collect items from other that we don't have
        for other_item in &other.items {
            // Map the other's user_idx to our user_idx
            let other_user = match other.users.get_id(other_item.user_idx) {
                Some(u) => u,
                None => continue,
            };
            let our_user_idx = self.users.get_or_insert(other_user);
            while self.user_states.len() <= our_user_idx.0 as usize {
                self.user_states.push(UserState::default());
            }

            // Check if we already have this item (or any character from it)
            // Items may be split, so we check if any item contains seq in its range
            let already_have = self.items.iter().any(|item| {
                item.user_idx == our_user_idx && item.contains(our_user_idx, other_item.seq)
            });

            if already_have {
                // Check if other has it deleted and we don't
                // Need to apply deletion to the exact range, potentially splitting items
                if other_item.deleted {
                    self.apply_deletion_range(our_user_idx, other_item.seq, other_item.len);
                }
                continue;
            }

            // Map origins to our user indices
            let left_origin = self.map_origin(&other_item.left_origin, other);
            let right_origin = self.map_origin(&other_item.right_origin, other);

            let item = Item::new(
                our_user_idx,
                other_item.seq,
                other_item.content.clone(),
                left_origin,
                right_origin,
            );

            // Apply deletion status
            let mut item = item;
            if other_item.deleted {
                item.deleted = true;
            }

            self.insert_item(item);
        }
    }

    fn to_string(&self) -> String {
        let mut result = Vec::new();
        for item in &self.items {
            if !item.deleted {
                result.extend_from_slice(&item.content);
            }
        }
        return String::from_utf8(result).unwrap_or_default();
    }

    fn len(&self) -> u64 {
        return self.calculate_len();
    }

    fn span_count(&self) -> usize {
        return self.items.len();
    }
}

impl YjsRga {
    /// Apply a deletion to a specific range of sequence numbers for a user.
    ///
    /// This handles the case where items may need to be split to apply
    /// a deletion that covers only part of an item.
    fn apply_deletion_range(&mut self, user_idx: UserIdx, start_seq: u32, len: u32) {
        let end_seq = start_seq + len;
        let mut i = 0;

        while i < self.items.len() {
            let item = &self.items[i];

            // Skip items from other users
            if item.user_idx != user_idx {
                i += 1;
                continue;
            }

            let item_end = item.seq + item.len;

            // Check for overlap: item.seq < end_seq && item_end > start_seq
            if item.seq >= end_seq || item_end <= start_seq {
                // No overlap
                i += 1;
                continue;
            }

            // There is overlap - determine if we need to split
            let overlap_start = start_seq.max(item.seq);
            let overlap_end = end_seq.min(item_end);

            if overlap_start == item.seq && overlap_end == item_end {
                // Entire item is in the deletion range
                self.items[i].deleted = true;
                i += 1;
            } else if overlap_start == item.seq {
                // Deletion covers the prefix of this item
                let split_offset = overlap_end - item.seq;
                let right = self.items[i].split(split_offset);
                self.items[i].deleted = true;
                self.items.insert(i + 1, right);
                i += 2; // Skip both the deleted prefix and the remaining suffix
            } else if overlap_end == item_end {
                // Deletion covers the suffix of this item
                let split_offset = overlap_start - item.seq;
                let mut right = self.items[i].split(split_offset);
                right.deleted = true;
                self.items.insert(i + 1, right);
                i += 2; // Skip both the prefix and the deleted suffix
            } else {
                // Deletion is in the middle - need two splits
                let first_split = overlap_start - item.seq;
                let mut mid = self.items[i].split(first_split);
                let second_split = overlap_end - overlap_start;
                let right = mid.split(second_split);
                mid.deleted = true;
                self.items.insert(i + 1, mid);
                self.items.insert(i + 2, right);
                i += 3; // Skip all three parts
            }
        }
    }

    /// Map an ItemId from another YjsRga to this one.
    fn map_origin(&mut self, origin: &ItemId, other: &YjsRga) -> ItemId {
        if origin.is_none() {
            return ItemId::none();
        }

        let other_user = match other.users.get_id(origin.user_idx) {
            Some(u) => u,
            None => return ItemId::none(),
        };

        let our_user_idx = self.users.get_or_insert(other_user);
        while self.user_states.len() <= our_user_idx.0 as usize {
            self.user_states.push(UserState::default());
        }

        return ItemId::new(our_user_idx, origin.seq);
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
        let rga = YjsRga::new();
        assert_eq!(rga.len(), 0);
        assert_eq!(rga.to_string(), "");
    }

    #[test]
    fn insert_at_beginning() {
        let mut rga = YjsRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello");
        assert_eq!(rga.to_string(), "hello");
        assert_eq!(rga.len(), 5);
    }

    #[test]
    fn insert_at_end() {
        let mut rga = YjsRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello");
        rga.insert(&user, 5, b" world");
        assert_eq!(rga.to_string(), "hello world");
    }

    #[test]
    fn insert_in_middle() {
        let mut rga = YjsRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hd");
        rga.insert(&user, 1, b"ello worl");
        assert_eq!(rga.to_string(), "hello world");
    }

    #[test]
    fn delete_range() {
        let mut rga = YjsRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello world");
        rga.delete(5, 6);
        assert_eq!(rga.to_string(), "hello");
    }

    #[test]
    fn delete_middle() {
        let mut rga = YjsRga::new();
        let user = make_user();
        rga.insert(&user, 0, b"hello");
        rga.delete(1, 3); // Delete "ell"
        assert_eq!(rga.to_string(), "ho");
    }

    #[test]
    fn merge_simple() {
        let user1 = make_user();
        let user2 = make_user();

        let mut a = YjsRga::new();
        let mut b = YjsRga::new();

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
        let mut rga = YjsRga::new();
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

        let mut a = YjsRga::new();
        let mut b = YjsRga::new();

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

        let mut a = YjsRga::new();
        let mut b = YjsRga::new();
        let mut c = YjsRga::new();

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
        let mut base = YjsRga::new();
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

        let mut a = YjsRga::new();
        a.insert(&user, 0, b"hello");

        let mut b = a.clone();
        b.delete(1, 3); // Delete "ell"

        a.merge(&b);

        assert_eq!(a.to_string(), "ho");
    }
}
