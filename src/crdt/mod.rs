// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-01-30"
// driver = "Isaac Clayton"

//! CRDT primitives for authenticated collaborative data structures.

pub mod op;
pub mod rga;
pub mod skip_list;
pub mod weighted_list;

/// A CRDT is a data type with a merge operator that is commutative,
/// associative, and idempotent.
pub trait Crdt {
    /// Merge another instance into this one.
    /// Must be commutative: merge(a, b) == merge(b, a)
    /// Must be associative: merge(a, merge(b, c)) == merge(merge(a, b), c)
    /// Must be idempotent: merge(a, merge(a, b)) == merge(a, b)
    fn merge(&mut self, other: &Self);
}
