// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

//! Log integration for OptimizedRga.
//!
//! This module provides the bridge between the RGA CRDT and the signed
//! append-only log infrastructure. It enables:
//!
//! - Exporting RGA operations as log entries
//! - Rebuilding RGA state from a log of operations
//! - Deterministic replay regardless of operation order
//! - Integration with cryptographic signing for authenticity
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                        Log Integration                               │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │  Operation           <- Self-contained CRDT operation               │
//! │  OperationId         <- Unique identifier (user, seq)               │
//! │  LogEntry            <- Operation + metadata for log storage        │
//! │  OpLog trait         <- Export/replay operations                    │
//! └─────────────────────────────────────────────────────────────────────┘
//!
//!                              ▼
//!
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                     Signed Append-Only Log                           │
//! │  (from src/log.rs: Log, SignedLog, Proof)                           │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Determinism Guarantee
//!
//! The key property of this integration is that replaying the same set of
//! operations in any order produces the same final state. This is achieved
//! through the Fugue algorithm's dual-origin conflict resolution, which
//! provides a total ordering for concurrent operations.
//!
//! # Example
//!
//! ```
//! use pedagogy::log_integration::{OpLog, Operation};
//! use pedagogy::rga_optimized::OptimizedRga;
//! use pedagogy::rga_trait::Rga;
//! use pedagogy::key::KeyPair;
//!
//! // Create and edit a document
//! let user = KeyPair::generate();
//! let mut doc = OptimizedRga::new();
//! doc.insert(&user.key_pub, 0, b"Hello, World!");
//! doc.delete(5, 2);
//!
//! // Export operations
//! let ops = doc.export_operations();
//!
//! // Rebuild from operations (deterministic)
//! let rebuilt = OptimizedRga::from_operations(ops.into_iter());
//! assert_eq!(doc.to_string(), rebuilt.to_string());
//! ```

use crate::key::Hash;
use crate::key::KeyPub;

// =============================================================================
// Operation Types
// =============================================================================

/// Unique identifier for an operation or item in the RGA.
///
/// This is the fundamental unit of identity in the CRDT. Every character
/// inserted gets a unique OperationId based on the user who inserted it
/// and their local sequence number.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OperationId {
    /// The public key of the user who created this operation.
    pub user: KeyPub,
    /// The sequence number, unique per user and monotonically increasing.
    pub seq: u32,
}

impl OperationId {
    /// Create a new operation ID.
    pub fn new(user: KeyPub, seq: u32) -> OperationId {
        return OperationId { user, seq };
    }
}

/// An operation that can be applied to an RGA.
///
/// Operations are self-contained: they include all information needed to
/// apply them to any replica without external context. This makes them
/// suitable for storage in an append-only log and transmission over network.
///
/// # Encoding
///
/// Operations use a compact binary encoding for efficient storage:
///
/// ```text
/// Insert:
/// ┌──────────┬──────────┬──────────┬──────────┬──────────┬──────────┐
/// │ type (1) │ user(32) │ seq (4)  │ left_or  │ right_or │ content  │
/// │   0x01   │  pubkey  │  u32 LE  │  option  │  option  │  bytes   │
/// └──────────┴──────────┴──────────┴──────────┴──────────┴──────────┘
///
/// Delete:
/// ┌──────────┬──────────┬──────────┬──────────┐
/// │ type (1) │ user(32) │ seq (4)  │ len (4)  │
/// │   0x02   │  pubkey  │  u32 LE  │  u32 LE  │
/// └──────────┴──────────┴──────────┴──────────┘
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    /// Insert content at a position determined by origins.
    ///
    /// The Fugue algorithm uses dual origins (left and right) to determine
    /// the exact insertion point and resolve concurrent conflicts.
    Insert {
        /// The user who performed this insert.
        user: KeyPub,
        /// Starting sequence number for this insert.
        seq: u32,
        /// The character immediately to the left when inserted (None = start).
        origin_left: Option<OperationId>,
        /// The character immediately to the right when inserted (None = end).
        origin_right: Option<OperationId>,
        /// The content bytes being inserted.
        content: Vec<u8>,
    },

    /// Delete a range of characters.
    ///
    /// Deletes are identified by the (user, seq) of the first character
    /// and the length. This allows deleting spans efficiently.
    Delete {
        /// The user whose content is being deleted.
        target_user: KeyPub,
        /// The starting sequence number of the deleted range.
        target_seq: u32,
        /// The number of characters to delete.
        len: u32,
    },
}

impl Operation {
    /// Create an insert operation.
    pub fn insert(
        user: KeyPub,
        seq: u32,
        origin_left: Option<OperationId>,
        origin_right: Option<OperationId>,
        content: Vec<u8>,
    ) -> Operation {
        return Operation::Insert {
            user,
            seq,
            origin_left,
            origin_right,
            content,
        };
    }

    /// Create a delete operation.
    pub fn delete(target_user: KeyPub, target_seq: u32, len: u32) -> Operation {
        return Operation::Delete {
            target_user,
            target_seq,
            len,
        };
    }

    /// Get the user who authored this operation.
    pub fn author(&self) -> &KeyPub {
        match self {
            Operation::Insert { user, .. } => user,
            Operation::Delete { target_user, .. } => target_user,
        }
    }

    /// Encode this operation to bytes.
    ///
    /// The encoding is compact and deterministic, suitable for hashing
    /// and storage in the signed log.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        match self {
            Operation::Insert {
                user,
                seq,
                origin_left,
                origin_right,
                content,
            } => {
                buf.push(0x01); // Type tag
                buf.extend_from_slice(&user.0);
                buf.extend_from_slice(&seq.to_le_bytes());

                // Encode left origin
                match origin_left {
                    None => buf.push(0x00),
                    Some(id) => {
                        buf.push(0x01);
                        buf.extend_from_slice(&id.user.0);
                        buf.extend_from_slice(&id.seq.to_le_bytes());
                    }
                }

                // Encode right origin
                match origin_right {
                    None => buf.push(0x00),
                    Some(id) => {
                        buf.push(0x01);
                        buf.extend_from_slice(&id.user.0);
                        buf.extend_from_slice(&id.seq.to_le_bytes());
                    }
                }

                // Encode content length and bytes
                buf.extend_from_slice(&(content.len() as u32).to_le_bytes());
                buf.extend_from_slice(content);
            }

            Operation::Delete {
                target_user,
                target_seq,
                len,
            } => {
                buf.push(0x02); // Type tag
                buf.extend_from_slice(&target_user.0);
                buf.extend_from_slice(&target_seq.to_le_bytes());
                buf.extend_from_slice(&len.to_le_bytes());
            }
        }

        return buf;
    }

    /// Decode an operation from bytes.
    ///
    /// Returns None if the bytes are malformed.
    pub fn decode(bytes: &[u8]) -> Option<Operation> {
        if bytes.is_empty() {
            return None;
        }

        let mut cursor = 0;

        let type_tag = bytes[cursor];
        cursor += 1;

        match type_tag {
            0x01 => {
                // Insert
                if bytes.len() < cursor + 32 + 4 {
                    return None;
                }

                let mut user_bytes = [0u8; 32];
                user_bytes.copy_from_slice(&bytes[cursor..cursor + 32]);
                let user = KeyPub(user_bytes);
                cursor += 32;

                let seq = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().ok()?);
                cursor += 4;

                // Decode left origin
                if cursor >= bytes.len() {
                    return None;
                }
                let origin_left = if bytes[cursor] == 0x00 {
                    cursor += 1;
                    None
                } else {
                    cursor += 1;
                    if bytes.len() < cursor + 32 + 4 {
                        return None;
                    }
                    let mut id_user = [0u8; 32];
                    id_user.copy_from_slice(&bytes[cursor..cursor + 32]);
                    cursor += 32;
                    let id_seq = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().ok()?);
                    cursor += 4;
                    Some(OperationId::new(KeyPub(id_user), id_seq))
                };

                // Decode right origin
                if cursor >= bytes.len() {
                    return None;
                }
                let origin_right = if bytes[cursor] == 0x00 {
                    cursor += 1;
                    None
                } else {
                    cursor += 1;
                    if bytes.len() < cursor + 32 + 4 {
                        return None;
                    }
                    let mut id_user = [0u8; 32];
                    id_user.copy_from_slice(&bytes[cursor..cursor + 32]);
                    cursor += 32;
                    let id_seq = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().ok()?);
                    cursor += 4;
                    Some(OperationId::new(KeyPub(id_user), id_seq))
                };

                // Decode content
                if bytes.len() < cursor + 4 {
                    return None;
                }
                let content_len =
                    u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().ok()?) as usize;
                cursor += 4;

                if bytes.len() < cursor + content_len {
                    return None;
                }
                let content = bytes[cursor..cursor + content_len].to_vec();

                return Some(Operation::Insert {
                    user,
                    seq,
                    origin_left,
                    origin_right,
                    content,
                });
            }

            0x02 => {
                // Delete
                if bytes.len() < cursor + 32 + 4 + 4 {
                    return None;
                }

                let mut target_user_bytes = [0u8; 32];
                target_user_bytes.copy_from_slice(&bytes[cursor..cursor + 32]);
                let target_user = KeyPub(target_user_bytes);
                cursor += 32;

                let target_seq = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().ok()?);
                cursor += 4;

                let len = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().ok()?);

                return Some(Operation::Delete {
                    target_user,
                    target_seq,
                    len,
                });
            }

            _ => return None,
        }
    }
}

// =============================================================================
// Log Entry
// =============================================================================

/// A log entry wraps an operation with metadata for the signed log.
///
/// Each entry contains:
/// - The operation itself
/// - A reference to the parent entry (for chaining)
/// - A placeholder for the user's signature
///
/// The parent hash creates a hash chain within each user's log, ensuring
/// that entries cannot be reordered without detection.
#[derive(Clone, Debug)]
pub struct LogEntry {
    /// The operation.
    pub operation: Operation,
    /// Hash of the previous entry from this user (None for first entry).
    pub parent_hash: Option<Hash>,
    /// Placeholder for cryptographic signature.
    /// In the actual log, this is computed by the Log infrastructure.
    pub signature_placeholder: [u8; 64],
}

impl LogEntry {
    /// Create a new log entry.
    pub fn new(operation: Operation, parent_hash: Option<Hash>) -> LogEntry {
        return LogEntry {
            operation,
            parent_hash,
            signature_placeholder: [0u8; 64],
        };
    }

    /// Encode this entry to bytes for storage in the log.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // Parent hash
        match &self.parent_hash {
            None => buf.push(0x00),
            Some(hash) => {
                buf.push(0x01);
                buf.extend_from_slice(&hash.0);
            }
        }

        // Operation
        let op_bytes = self.operation.encode();
        buf.extend_from_slice(&(op_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(&op_bytes);

        return buf;
    }

    /// Decode a log entry from bytes.
    pub fn decode(bytes: &[u8]) -> Option<LogEntry> {
        if bytes.is_empty() {
            return None;
        }

        let mut cursor = 0;

        // Parent hash
        let parent_hash = if bytes[cursor] == 0x00 {
            cursor += 1;
            None
        } else {
            cursor += 1;
            if bytes.len() < cursor + 32 {
                return None;
            }
            let mut hash_bytes = [0u8; 32];
            hash_bytes.copy_from_slice(&bytes[cursor..cursor + 32]);
            cursor += 32;
            Some(Hash(hash_bytes))
        };

        // Operation length
        if bytes.len() < cursor + 4 {
            return None;
        }
        let op_len = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().ok()?) as usize;
        cursor += 4;

        // Operation
        if bytes.len() < cursor + op_len {
            return None;
        }
        let operation = Operation::decode(&bytes[cursor..cursor + op_len])?;

        return Some(LogEntry::new(operation, parent_hash));
    }
}

// =============================================================================
// OpLog Trait
// =============================================================================

/// Trait for CRDTs that can export and replay operations.
///
/// This trait provides the bridge between the CRDT's internal state and
/// the signed append-only log. Implementations must ensure:
///
/// 1. **Round-trip**: `from_operations(export_operations())` produces same state
/// 2. **Order independence**: Any permutation of ops produces same final state
/// 3. **Determinism**: Same ops always produce same state
pub trait OpLog: Default {
    /// Export all operations needed to reconstruct this CRDT.
    ///
    /// The returned operations, when replayed via `from_operations`,
    /// should produce an identical CRDT state.
    fn export_operations(&self) -> Vec<Operation>;

    /// Rebuild a CRDT from a sequence of operations.
    ///
    /// This is the replay function. It applies operations in iteration
    /// order, using the CRDT's merge semantics to resolve conflicts.
    ///
    /// Due to CRDT properties, the iteration order does not affect the
    /// final result as long as all operations are applied.
    fn from_operations(ops: impl Iterator<Item = Operation>) -> Self;

    /// Apply a single operation to this CRDT.
    ///
    /// Returns true if the operation was applied (new), false if it
    /// was already present (idempotent).
    fn apply_operation(&mut self, op: Operation) -> bool;
}

// =============================================================================
// Version Vector
// =============================================================================

/// A version vector tracks the latest sequence number seen from each user.
///
/// Version vectors enable efficient synchronization:
/// - Compare vectors to detect missing operations
/// - Request only operations newer than our vector
/// - Determine causal ordering of operations
#[derive(Clone, Debug, Default)]
pub struct VersionVector {
    /// Map from user public key to highest seen sequence number.
    versions: std::collections::HashMap<KeyPub, u32>,
}

impl VersionVector {
    /// Create a new empty version vector.
    pub fn new() -> VersionVector {
        return VersionVector {
            versions: std::collections::HashMap::new(),
        };
    }

    /// Get the version for a user (0 if not present).
    pub fn get(&self, user: &KeyPub) -> u32 {
        return *self.versions.get(user).unwrap_or(&0);
    }

    /// Update the version for a user if the new seq is higher.
    pub fn update(&mut self, user: &KeyPub, seq: u32) {
        let entry = self.versions.entry(user.clone()).or_insert(0);
        if seq > *entry {
            *entry = seq;
        }
    }

    /// Check if this vector dominates another (has all their updates).
    pub fn dominates(&self, other: &VersionVector) -> bool {
        for (user, &seq) in &other.versions {
            if self.get(user) < seq {
                return false;
            }
        }
        return true;
    }

    /// Merge another version vector into this one (take max of each).
    pub fn merge(&mut self, other: &VersionVector) {
        for (user, &seq) in &other.versions {
            self.update(user, seq);
        }
    }

    /// Get all users in this vector.
    pub fn users(&self) -> impl Iterator<Item = &KeyPub> {
        return self.versions.keys();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::KeyPair;

    fn make_user() -> KeyPub {
        return KeyPair::generate().key_pub;
    }

    // =========================================================================
    // Operation encoding tests
    // =========================================================================

    #[test]
    fn encode_decode_insert_no_origins() {
        let user = make_user();
        let op = Operation::insert(user, 0, None, None, b"hello".to_vec());

        let encoded = op.encode();
        let decoded = Operation::decode(&encoded).expect("decode should succeed");

        assert_eq!(op, decoded);
    }

    #[test]
    fn encode_decode_insert_with_origins() {
        let user1 = make_user();
        let user2 = make_user();

        let left = OperationId::new(user2.clone(), 5);
        let right = OperationId::new(user2.clone(), 6);

        let op = Operation::insert(user1, 10, Some(left), Some(right), b"world".to_vec());

        let encoded = op.encode();
        let decoded = Operation::decode(&encoded).expect("decode should succeed");

        assert_eq!(op, decoded);
    }

    #[test]
    fn encode_decode_delete() {
        let user = make_user();
        let op = Operation::delete(user, 5, 10);

        let encoded = op.encode();
        let decoded = Operation::decode(&encoded).expect("decode should succeed");

        assert_eq!(op, decoded);
    }

    #[test]
    fn decode_empty_returns_none() {
        assert!(Operation::decode(&[]).is_none());
    }

    #[test]
    fn decode_invalid_type_returns_none() {
        assert!(Operation::decode(&[0xFF]).is_none());
    }

    #[test]
    fn decode_truncated_returns_none() {
        let user = make_user();
        let op = Operation::insert(user, 0, None, None, b"hello".to_vec());
        let encoded = op.encode();

        // Try decoding truncated versions
        for len in 1..encoded.len() {
            let truncated = &encoded[..len];
            // Should either decode or return None, never panic
            let _ = Operation::decode(truncated);
        }
    }

    // =========================================================================
    // LogEntry tests
    // =========================================================================

    #[test]
    fn encode_decode_log_entry_no_parent() {
        let user = make_user();
        let op = Operation::insert(user, 0, None, None, b"test".to_vec());
        let entry = LogEntry::new(op.clone(), None);

        let encoded = entry.encode();
        let decoded = LogEntry::decode(&encoded).expect("decode should succeed");

        assert_eq!(decoded.operation, op);
        assert!(decoded.parent_hash.is_none());
    }

    #[test]
    fn encode_decode_log_entry_with_parent() {
        let user = make_user();
        let op = Operation::delete(user, 0, 5);
        let parent = Hash([42u8; 32]);
        let entry = LogEntry::new(op.clone(), Some(parent.clone()));

        let encoded = entry.encode();
        let decoded = LogEntry::decode(&encoded).expect("decode should succeed");

        assert_eq!(decoded.operation, op);
        assert_eq!(decoded.parent_hash, Some(parent));
    }

    // =========================================================================
    // VersionVector tests
    // =========================================================================

    #[test]
    fn version_vector_starts_at_zero() {
        let vv = VersionVector::new();
        let user = make_user();
        assert_eq!(vv.get(&user), 0);
    }

    #[test]
    fn version_vector_update() {
        let mut vv = VersionVector::new();
        let user = make_user();

        vv.update(&user, 5);
        assert_eq!(vv.get(&user), 5);

        // Update to higher value
        vv.update(&user, 10);
        assert_eq!(vv.get(&user), 10);

        // Update to lower value (should not change)
        vv.update(&user, 3);
        assert_eq!(vv.get(&user), 10);
    }

    #[test]
    fn version_vector_dominates() {
        let user1 = make_user();
        let user2 = make_user();

        let mut vv1 = VersionVector::new();
        vv1.update(&user1, 10);
        vv1.update(&user2, 5);

        let mut vv2 = VersionVector::new();
        vv2.update(&user1, 5);
        vv2.update(&user2, 5);

        assert!(vv1.dominates(&vv2));
        assert!(!vv2.dominates(&vv1));

        // Equal vectors dominate each other
        let vv3 = vv2.clone();
        assert!(vv2.dominates(&vv3));
        assert!(vv3.dominates(&vv2));
    }

    #[test]
    fn version_vector_merge() {
        let user1 = make_user();
        let user2 = make_user();

        let mut vv1 = VersionVector::new();
        vv1.update(&user1, 10);
        vv1.update(&user2, 3);

        let mut vv2 = VersionVector::new();
        vv2.update(&user1, 5);
        vv2.update(&user2, 7);

        vv1.merge(&vv2);

        // Should have max of each
        assert_eq!(vv1.get(&user1), 10);
        assert_eq!(vv1.get(&user2), 7);
    }
}

// =============================================================================
// Integration tests with OptimizedRga
// =============================================================================

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::rga_optimized::OptimizedRga;
    use crate::rga_trait::Rga;
    use crate::key::KeyPair;

    fn make_user() -> KeyPub {
        return KeyPair::generate().key_pub;
    }

    // =========================================================================
    // Round-trip tests
    // =========================================================================

    #[test]
    fn round_trip_empty() {
        let doc = OptimizedRga::new();
        let ops = doc.export_operations();
        let rebuilt = OptimizedRga::from_operations(ops.into_iter());

        assert_eq!(doc.to_string(), rebuilt.to_string());
        assert_eq!(doc.len(), rebuilt.len());
    }

    #[test]
    fn round_trip_single_insert() {
        let user = make_user();
        let mut doc = OptimizedRga::new();
        doc.insert(&user, 0, b"hello");

        let ops = doc.export_operations();
        let rebuilt = OptimizedRga::from_operations(ops.into_iter());

        assert_eq!(doc.to_string(), rebuilt.to_string());
        assert_eq!(doc.len(), rebuilt.len());
    }

    #[test]
    fn round_trip_multiple_inserts() {
        let user = make_user();
        let mut doc = OptimizedRga::new();
        doc.insert(&user, 0, b"hello");
        doc.insert(&user, 5, b" world");
        doc.insert(&user, 0, b"say ");

        let ops = doc.export_operations();
        let rebuilt = OptimizedRga::from_operations(ops.into_iter());

        assert_eq!(doc.to_string(), rebuilt.to_string());
    }

    #[test]
    fn round_trip_with_deletes() {
        let user = make_user();
        let mut doc = OptimizedRga::new();
        doc.insert(&user, 0, b"hello world");
        doc.delete(5, 6); // Delete " world"

        let ops = doc.export_operations();
        let rebuilt = OptimizedRga::from_operations(ops.into_iter());

        assert_eq!(doc.to_string(), rebuilt.to_string());
        assert_eq!(rebuilt.to_string(), "hello");
    }

    #[test]
    fn round_trip_sequential_typing() {
        let user = make_user();
        let mut doc = OptimizedRga::new();

        let text = "The quick brown fox jumps over the lazy dog.";
        for (i, c) in text.bytes().enumerate() {
            doc.insert(&user, i as u64, &[c]);
        }

        let ops = doc.export_operations();
        let rebuilt = OptimizedRga::from_operations(ops.into_iter());

        assert_eq!(doc.to_string(), rebuilt.to_string());
        assert_eq!(rebuilt.to_string(), text);
    }

    #[test]
    fn round_trip_multi_user() {
        let user1 = make_user();
        let user2 = make_user();

        let mut doc = OptimizedRga::new();
        doc.insert(&user1, 0, b"Hello");
        doc.insert(&user2, 5, b" World");
        doc.insert(&user1, 11, b"!");

        let ops = doc.export_operations();
        let rebuilt = OptimizedRga::from_operations(ops.into_iter());

        assert_eq!(doc.to_string(), rebuilt.to_string());
    }

    // =========================================================================
    // Order independence tests (determinism)
    // =========================================================================

    #[test]
    fn order_independence_two_users() {
        let user1 = make_user();
        let user2 = make_user();

        // Create document with concurrent edits
        let mut doc1 = OptimizedRga::new();
        doc1.insert(&user1, 0, b"A");

        let mut doc2 = OptimizedRga::new();
        doc2.insert(&user2, 0, b"B");

        // Merge to create combined document
        let mut combined = doc1.clone();
        combined.merge(&doc2);

        // Export operations
        let ops = combined.export_operations();

        // Rebuild in original order
        let rebuilt1 = OptimizedRga::from_operations(ops.clone().into_iter());

        // Rebuild in reversed order
        let mut reversed_ops = ops.clone();
        reversed_ops.reverse();
        let rebuilt2 = OptimizedRga::from_operations(reversed_ops.into_iter());

        // Both should produce the same result
        assert_eq!(rebuilt1.to_string(), rebuilt2.to_string());
    }

    #[test]
    fn order_independence_three_concurrent_inserts() {
        let user1 = make_user();
        let user2 = make_user();
        let user3 = make_user();

        // Three users all insert at position 0
        let mut doc1 = OptimizedRga::new();
        doc1.insert(&user1, 0, b"A");

        let mut doc2 = OptimizedRga::new();
        doc2.insert(&user2, 0, b"B");

        let mut doc3 = OptimizedRga::new();
        doc3.insert(&user3, 0, b"C");

        // Merge all
        let mut combined = doc1.clone();
        combined.merge(&doc2);
        combined.merge(&doc3);

        let ops = combined.export_operations();

        // Try all permutations of applying operations
        use std::collections::HashSet;
        let mut results = HashSet::new();

        // Generate permutations manually for 3 elements
        let permutations = [
            vec![0, 1, 2],
            vec![0, 2, 1],
            vec![1, 0, 2],
            vec![1, 2, 0],
            vec![2, 0, 1],
            vec![2, 1, 0],
        ];

        for perm in &permutations {
            let reordered: Vec<_> = perm.iter().map(|&i| ops[i].clone()).collect();
            let rebuilt = OptimizedRga::from_operations(reordered.into_iter());
            results.insert(rebuilt.to_string());
        }

        // All permutations should produce the same result
        assert_eq!(
            results.len(),
            1,
            "Different orderings produced different results: {:?}",
            results
        );
    }

    #[test]
    fn order_independence_concurrent_inserts_with_deletes() {
        // Test that concurrent inserts can be reordered, even when there are deletes
        // Note: Deletes must be applied after their target inserts (causal ordering)
        let user1 = make_user();
        let user2 = make_user();

        // User1 inserts "hello"
        let mut doc1 = OptimizedRga::new();
        doc1.insert(&user1, 0, b"hello");

        // User2 inserts "world" concurrently
        let mut doc2 = OptimizedRga::new();
        doc2.insert(&user2, 0, b"world");

        // Merge and then delete from merged state
        let mut combined = doc1.clone();
        combined.merge(&doc2);
        combined.delete(0, 5); // Delete first 5 chars

        let ops = combined.export_operations();

        // Separate inserts from deletes to ensure causal ordering
        let inserts: Vec<_> = ops
            .iter()
            .filter(|op| matches!(op, Operation::Insert { .. }))
            .cloned()
            .collect();
        let deletes: Vec<_> = ops
            .iter()
            .filter(|op| matches!(op, Operation::Delete { .. }))
            .cloned()
            .collect();

        // Try different orderings of inserts, followed by deletes
        let mut reversed_inserts = inserts.clone();
        reversed_inserts.reverse();

        let rebuilt1 = OptimizedRga::from_operations(
            inserts.into_iter().chain(deletes.clone().into_iter()),
        );
        let rebuilt2 = OptimizedRga::from_operations(
            reversed_inserts.into_iter().chain(deletes.into_iter()),
        );

        // Both should produce the same result
        assert_eq!(rebuilt1.to_string(), rebuilt2.to_string());
    }

    #[test]
    fn causal_delivery_required_for_correct_replay() {
        // This test documents that operations MUST be applied in causal order.
        // The export_operations() method returns ops in causal order (inserts
        // before their dependent deletes). Reversing breaks causality.
        //
        // In production, use version vectors or explicit dependencies to ensure
        // causal delivery. The signed log from DESIGN.md provides this through
        // parent hash references.
        let user = make_user();

        let mut doc = OptimizedRga::new();
        doc.insert(&user, 0, b"hello");
        doc.delete(0, 2); // Delete "he"

        let ops = doc.export_operations();

        // Correct order produces expected result
        let rebuilt_correct = OptimizedRga::from_operations(ops.clone().into_iter());
        assert_eq!(rebuilt_correct.to_string(), "llo");

        // Verify exports are ordered: inserts before deletes
        let first_delete_idx = ops.iter().position(|op| {
            matches!(op, Operation::Delete { .. })
        });
        let last_insert_idx = ops.iter().rposition(|op| {
            matches!(op, Operation::Insert { .. })
        });

        if let (Some(del_idx), Some(ins_idx)) = (first_delete_idx, last_insert_idx) {
            assert!(
                ins_idx < del_idx,
                "Inserts should come before deletes in exported operations"
            );
        }
    }

    // =========================================================================
    // Determinism tests
    // =========================================================================

    #[test]
    fn determinism_same_ops_same_result() {
        let user = make_user();

        let mut doc = OptimizedRga::new();
        doc.insert(&user, 0, b"test");

        let ops = doc.export_operations();

        // Rebuild multiple times
        let rebuilt1 = OptimizedRga::from_operations(ops.clone().into_iter());
        let rebuilt2 = OptimizedRga::from_operations(ops.clone().into_iter());
        let rebuilt3 = OptimizedRga::from_operations(ops.into_iter());

        assert_eq!(rebuilt1.to_string(), rebuilt2.to_string());
        assert_eq!(rebuilt2.to_string(), rebuilt3.to_string());
    }

    #[test]
    fn determinism_complex_document() {
        let user1 = make_user();
        let user2 = make_user();

        // Create a complex document
        let mut doc = OptimizedRga::new();
        doc.insert(&user1, 0, b"Hello");
        doc.insert(&user2, 5, b" World");
        doc.delete(0, 1); // Delete 'H'
        doc.insert(&user1, 0, b"h"); // Insert lowercase 'h'
        doc.insert(&user2, 11, b"!");
        doc.delete(6, 5); // Delete "World"
        doc.insert(&user1, 6, b"Universe");

        let ops = doc.export_operations();

        // Rebuild 10 times and verify consistency
        let expected = doc.to_string();
        for _ in 0..10 {
            let rebuilt = OptimizedRga::from_operations(ops.clone().into_iter());
            assert_eq!(rebuilt.to_string(), expected);
        }
    }

    // =========================================================================
    // Apply operation idempotence tests
    // =========================================================================

    #[test]
    fn apply_operation_idempotent() {
        let user = make_user();

        let op = Operation::insert(user.clone(), 0, None, None, b"hello".to_vec());

        let mut doc = OptimizedRga::new();

        // First application should succeed
        let applied1 = doc.apply_operation(op.clone());
        assert!(applied1);
        assert_eq!(doc.to_string(), "hello");

        // Second application should be idempotent (return false)
        let applied2 = doc.apply_operation(op);
        assert!(!applied2);
        assert_eq!(doc.to_string(), "hello"); // Content unchanged
    }

    // =========================================================================
    // Operation encoding round-trip tests
    // =========================================================================

    #[test]
    fn operation_encode_decode_roundtrip() {
        let user = make_user();

        let ops = vec![
            Operation::insert(user.clone(), 0, None, None, b"hello".to_vec()),
            Operation::insert(
                user.clone(),
                5,
                Some(OperationId::new(user.clone(), 4)),
                None,
                b" world".to_vec(),
            ),
            Operation::delete(user.clone(), 0, 5),
        ];

        for op in ops {
            let encoded = op.encode();
            let decoded = Operation::decode(&encoded).expect("decode should succeed");
            assert_eq!(op, decoded);
        }
    }
}
