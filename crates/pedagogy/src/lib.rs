// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

//! Educational CRDT implementations and primitives.
//!
//! This crate provides pedagogical implementations of various CRDT algorithms
//! for collaborative text editing. It is designed for:
//!
//! - Learning how different CRDT approaches work
//! - Comparing algorithm characteristics (memory, performance, semantics)
//! - Running conformance tests to verify CRDT properties
//!
//! # Implementations
//!
//! | Implementation | Algorithm | Key Feature |
//! |----------------|-----------|-------------|
//! | `YjsRga` | YATA | Dual origins, widely deployed |
//! | `DiamondRga` | YATA + B-tree | O(log n) operations |
//! | `ColaRga` | Anchor + Lamport | Simpler than YATA |
//! | `JsonJoyRga` | Dual-tree + Splay | Temporal locality optimization |
//! | `LoroRga` | Fugue | Best anti-interleaving |
//! | `OptimizedRga` | Fugue + B-tree | Production-ready |
//!
//! # Primitives
//!
//! The `primitives` module provides reusable building blocks:
//!
//! - `LamportClock`, `VectorClock`: Logical clocks for ordering
//! - `UserTable`: Compact user ID mapping
//! - `CursorCache`: Amortize sequential lookups
//! - `CompactSpan`: Memory-efficient span representation
//!
//! # CRDT Properties
//!
//! All implementations must satisfy:
//!
//! - **Commutativity**: merge(A, B) == merge(B, A)
//! - **Associativity**: merge(A, merge(B, C)) == merge(merge(A, B), C)
//! - **Idempotency**: merge(A, A) == A
//!
//! These properties are verified by the conformance test suite.
//!
//! # Example
//!
//! ```
//! use pedagogy::rga_trait::Rga;
//! use pedagogy::yjs::YjsRga;
//! use pedagogy::key::KeyPair;
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

pub mod btree_list;
pub mod cola;
pub mod diamond;
pub mod json_joy;
pub mod key;
pub mod log_integration;
pub mod loro;
pub mod primitives;
pub mod rga_optimized;
pub mod rga_trait;
pub mod yjs;

/// A CRDT is a data type with a merge operator that is commutative,
/// associative, and idempotent.
pub trait Crdt {
    /// Merge another instance into this one.
    fn merge(&mut self, other: &Self);
}
