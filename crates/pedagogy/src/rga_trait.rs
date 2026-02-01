// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

//! The Rga trait defines the interface for a Replicated Growable Array.
//!
//! All RGA implementations must provide this interface, enabling:
//! - Conformance testing with shared test suites
//! - Benchmarking across different implementations
//! - Easy swapping of implementations
//!
//! The trait is designed to be minimal while capturing the essential
//! operations for collaborative text editing.

use std::hash::Hash;

/// A Replicated Growable Array (RGA) is a sequence CRDT for collaborative
/// text editing.
///
/// Implementors must provide:
/// - Insert operations with CRDT ordering
/// - Delete operations (tombstones or real deletion)
/// - Merge with another replica
/// - Conversion to/from visible content
///
/// The merge operation must satisfy CRDT laws:
/// - Commutative: merge(A, B) == merge(B, A)
/// - Associative: merge(A, merge(B, C)) == merge(merge(A, B), C)
/// - Idempotent: merge(A, A) == A
pub trait Rga: Clone + Default {
    /// The user/agent identifier type.
    /// 
    /// This is typically a public key or unique ID for each replica.
    type UserId: Clone + Eq + Hash;

    /// Insert content at a visible position.
    ///
    /// The position is in terms of visible characters (not including
    /// deleted/tombstoned items). Position 0 inserts at the beginning.
    ///
    /// The user ID is used for:
    /// - Assigning sequence numbers
    /// - Breaking ties in concurrent edits
    fn insert(&mut self, user: &Self::UserId, pos: u64, content: &[u8]);

    /// Delete a range of visible characters.
    ///
    /// Deletes `len` characters starting at position `start`.
    /// The range is in terms of visible characters.
    fn delete(&mut self, start: u64, len: u64);

    /// Merge another replica into this one.
    ///
    /// This operation must be:
    /// - Commutative: merge(A, B) produces same result as merge(B, A)
    /// - Associative: merge(A, merge(B, C)) == merge(merge(A, B), C)
    /// - Idempotent: merge(A, A) == A
    ///
    /// After merging, both replicas should have the same visible content.
    fn merge(&mut self, other: &Self);

    /// Get the visible content as a string.
    ///
    /// Returns only non-deleted characters, concatenated in document order.
    fn to_string(&self) -> String;

    /// Get the visible length.
    ///
    /// Returns the number of visible (non-deleted) characters.
    fn len(&self) -> u64;

    /// Check if the RGA is empty.
    fn is_empty(&self) -> bool {
        return self.len() == 0;
    }

    /// Get a slice of the visible content.
    ///
    /// Returns characters in range [start, end) as a String.
    /// Returns None if range is out of bounds.
    fn slice(&self, start: u64, end: u64) -> Option<String> {
        if start > end || end > self.len() {
            return None;
        }
        let content = self.to_string();
        return Some(content[start as usize..end as usize].to_string());
    }

    /// Get the number of internal spans (for profiling).
    ///
    /// Returns the number of spans/nodes in the internal structure.
    /// This is implementation-specific and used for measuring fragmentation.
    fn span_count(&self) -> usize {
        return 0; // Default implementation for those that don't track spans
    }
}

/// Extension trait for Rga implementations that support operation-based sync.
///
/// Some implementations may support extracting and applying individual
/// operations for more efficient sync protocols.
pub trait RgaOps: Rga {
    /// The operation type.
    type Op;

    /// Apply an operation from a specific user.
    ///
    /// Returns true if the operation was applied, false if it was already
    /// present (idempotent application).
    fn apply(&mut self, user: &Self::UserId, op: &Self::Op) -> bool;

    /// Extract all operations needed to reconstruct this document.
    ///
    /// The returned operations, when applied in order to an empty RGA,
    /// should produce the same document.
    fn to_ops(&self) -> Vec<(Self::UserId, Self::Op)>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper to verify CRDT properties for an Rga implementation.
    /// This is a function rather than a trait so implementations can call it.
    pub fn verify_crdt_properties<R: Rga>(
        make_empty: impl Fn() -> R,
        user1: R::UserId,
        user2: R::UserId,
    ) {
        // Test commutativity: merge(A, B) == merge(B, A)
        {
            let mut a = make_empty();
            let mut b = make_empty();
            
            a.insert(&user1, 0, b"hello");
            b.insert(&user2, 0, b"world");
            
            let mut ab = a.clone();
            ab.merge(&b);
            
            let mut ba = b.clone();
            ba.merge(&a);
            
            assert_eq!(ab.to_string(), ba.to_string(), "merge should be commutative");
        }

        // Test associativity: merge(A, merge(B, C)) == merge(merge(A, B), C)
        {
            let mut a = make_empty();
            let mut b = make_empty();
            let mut c = make_empty();
            
            a.insert(&user1, 0, b"A");
            b.insert(&user2, 0, b"B");
            c.insert(&user1, 0, b"C");
            
            let mut bc = b.clone();
            bc.merge(&c);
            let mut a_bc = a.clone();
            a_bc.merge(&bc);
            
            let mut ab = a.clone();
            ab.merge(&b);
            let mut ab_c = ab;
            ab_c.merge(&c);
            
            assert_eq!(a_bc.to_string(), ab_c.to_string(), "merge should be associative");
        }

        // Test idempotence: merge(A, A) == A
        {
            let mut a = make_empty();
            a.insert(&user1, 0, b"hello");
            
            let before = a.to_string();
            a.merge(&a.clone());
            let after = a.to_string();
            
            assert_eq!(before, after, "merge should be idempotent");
        }
    }
}
