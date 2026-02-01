// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-02-01"
// driver = "Isaac Clayton"

//! CRDT primitives for collaborative data structures.
//!
//! This module contains the production RGA implementation.
//! For educational CRDT implementations and alternative algorithms,
//! see the `pedagogy` crate.

mod btree_list;
pub mod op;
pub mod rga;

/// A CRDT is a data type with a merge operator that is commutative,
/// associative, and idempotent.
pub trait Crdt {
    /// Merge another instance into this one.
    fn merge(&mut self, other: &Self);
}
