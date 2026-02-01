// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

//! Identifier types for CRDT operations and items.
//!
//! # Identifier Hierarchy
//!
//! - `OpId`: Identifies an operation (user, sequence number)
//! - `ItemId`: Identifies a specific character within an operation (user, seq, offset)
//!
//! # Design Decisions
//!
//! IDs are designed to be:
//! - Globally unique: (user, seq) pairs are unique across all replicas
//! - Totally ordered: can be compared deterministically
//! - Compact: minimal memory footprint
//! - Hashable: can be used as map keys

use std::cmp::Ordering;
use std::hash::Hash;

/// An operation identifier.
///
/// Uniquely identifies an operation from a specific user.
/// The (user, seq) pair is globally unique assuming users
/// are unique and sequence numbers are monotonically increasing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OpId<U> {
    /// The user who created this operation.
    pub user: U,
    /// The sequence number (monotonically increasing per user).
    pub seq: u64,
}

impl<U> OpId<U> {
    /// Create a new operation ID.
    pub fn new(user: U, seq: u64) -> OpId<U> {
        return OpId { user, seq };
    }
}

impl<U: Ord> PartialOrd for OpId<U> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        return Some(self.cmp(other));
    }
}

impl<U: Ord> Ord for OpId<U> {
    fn cmp(&self, other: &Self) -> Ordering {
        // Compare by user first, then by seq
        match self.user.cmp(&other.user) {
            Ordering::Equal => self.seq.cmp(&other.seq),
            other => other,
        }
    }
}

/// An item identifier.
///
/// Identifies a specific character within an operation.
/// Used when operations can insert multiple characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ItemId<U> {
    /// The user who created this item.
    pub user: U,
    /// The sequence number of the operation.
    pub seq: u64,
    /// Offset within the operation (0 for single-char ops).
    pub offset: u32,
}

impl<U> ItemId<U> {
    /// Create a new item ID.
    pub fn new(user: U, seq: u64, offset: u32) -> ItemId<U> {
        return ItemId { user, seq, offset };
    }

    /// Create an item ID for a single-character operation.
    pub fn single(user: U, seq: u64) -> ItemId<U> {
        return ItemId { user, seq, offset: 0 };
    }
}

impl<U: Clone> ItemId<U> {
    /// Get the operation ID for this item.
    pub fn op_id(&self) -> OpId<U> {
        return OpId::new(self.user.clone(), self.seq);
    }
}

impl<U: Ord> PartialOrd for ItemId<U> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        return Some(self.cmp(other));
    }
}

impl<U: Ord> Ord for ItemId<U> {
    fn cmp(&self, other: &Self) -> Ordering {
        // Compare by user, then seq, then offset
        match self.user.cmp(&other.user) {
            Ordering::Equal => match self.seq.cmp(&other.seq) {
                Ordering::Equal => self.offset.cmp(&other.offset),
                other => other,
            },
            other => other,
        }
    }
}

/// A compact user index.
///
/// Instead of storing full user IDs (which may be 32-byte public keys),
/// we can use a 16-bit index into a user table. This reduces memory
/// usage significantly for spans.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UserIdx(pub u16);

impl UserIdx {
    /// Sentinel value indicating no user (e.g., for null origins).
    pub const NONE: UserIdx = UserIdx(u16::MAX);

    /// Create a new user index.
    pub fn new(idx: u16) -> UserIdx {
        return UserIdx(idx);
    }

    /// Check if this is the sentinel value.
    pub fn is_none(&self) -> bool {
        return self.0 == u16::MAX;
    }
}

/// A compact operation ID using user indices.
///
/// Uses a 16-bit user index instead of full user ID.
/// Suitable for internal storage in spans.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CompactOpId {
    /// User index (into a user table).
    pub user_idx: UserIdx,
    /// Sequence number.
    pub seq: u32,
}

impl CompactOpId {
    /// Create a new compact operation ID.
    pub fn new(user_idx: UserIdx, seq: u32) -> CompactOpId {
        return CompactOpId { user_idx, seq };
    }

    /// Create a "none" ID for null origins.
    pub fn none() -> CompactOpId {
        return CompactOpId {
            user_idx: UserIdx::NONE,
            seq: 0,
        };
    }

    /// Check if this is the "none" sentinel.
    pub fn is_none(&self) -> bool {
        return self.user_idx.is_none();
    }
}

impl PartialOrd for CompactOpId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        return Some(self.cmp(other));
    }
}

impl Ord for CompactOpId {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.user_idx.cmp(&other.user_idx) {
            Ordering::Equal => self.seq.cmp(&other.seq),
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn op_id_ordering() {
        let a = OpId::new("alice", 1);
        let b = OpId::new("alice", 2);
        let c = OpId::new("bob", 1);

        assert!(a < b);
        assert!(a < c); // "alice" < "bob"
        assert!(b < c);
    }

    #[test]
    fn item_id_ordering() {
        let a = ItemId::new("alice", 1, 0);
        let b = ItemId::new("alice", 1, 1);
        let c = ItemId::new("alice", 2, 0);

        assert!(a < b);
        assert!(b < c);
    }

    #[test]
    fn item_id_to_op_id() {
        let item = ItemId::new("alice", 42, 5);
        let op = item.op_id();

        assert_eq!(op.user, "alice");
        assert_eq!(op.seq, 42);
    }

    #[test]
    fn user_idx_none() {
        let none = UserIdx::NONE;
        assert!(none.is_none());

        let some = UserIdx::new(5);
        assert!(!some.is_none());
    }

    #[test]
    fn compact_op_id_none() {
        let none = CompactOpId::none();
        assert!(none.is_none());

        let some = CompactOpId::new(UserIdx::new(0), 1);
        assert!(!some.is_none());
    }

    #[test]
    fn compact_op_id_ordering() {
        let a = CompactOpId::new(UserIdx::new(0), 1);
        let b = CompactOpId::new(UserIdx::new(0), 2);
        let c = CompactOpId::new(UserIdx::new(1), 1);

        assert!(a < b);
        assert!(b < c);
    }
}
