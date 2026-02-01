// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-02-01"
// driver = "Isaac Clayton"

//! CRDT primitives for collaborative data structures.

mod btree_list;
pub mod cola;
pub mod diamond;
pub mod json_joy;
pub mod log_integration;
pub mod loro;
pub mod op;
pub mod primitives;
pub mod rga;
pub mod rga_optimized;
pub mod rga_trait;
pub mod yjs;

/// A CRDT is a data type with a merge operator that is commutative,
/// associative, and idempotent.
pub trait Crdt {
    /// Merge another instance into this one.
    fn merge(&mut self, other: &Self);
}
