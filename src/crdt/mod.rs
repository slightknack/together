// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-01-31"
// driver = "Isaac Clayton"

//! CRDT primitives for collaborative data structures.

mod btree_list;
pub mod op;
pub mod rga;

/// A CRDT is a data type with a merge operator that is commutative,
/// associative, and idempotent.
pub trait Crdt {
    /// Merge another instance into this one.
    fn merge(&mut self, other: &Self);
}
