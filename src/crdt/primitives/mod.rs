// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

//! Shared primitives for CRDT implementations.
//!
//! This module provides reusable building blocks that multiple RGA
//! implementations can compose. Each primitive is designed to be:
//!
//! - Parameterized: configurable for different use cases
//! - Tested: comprehensive property-based tests
//! - Documented: clear complexity guarantees
//! - Benchmarked: performance characteristics understood
//!
//! # Primitives
//!
//! ## Clocks
//! - `LamportClock`: simple monotonic counter
//! - `VectorClock`: tracks causality across replicas
//! - `HybridLogicalClock`: combines wall time with logical time
//!
//! ## Trees
//! - Re-export of `btree_list::BTreeList` (already exists)
//! - `SplayTree`: self-adjusting for temporal locality
//!
//! ## Lists
//! - `SkipList`: probabilistic balanced structure
//! - `GapBuffer`: efficient for local edits
//! - `Rope`: for large documents
//!
//! ## Maps
//! - Re-export of `FxHashMap` (fast hashing)
//! - `IntervalMap`: for range queries
//!
//! ## IDs
//! - `UserId`: replica identifier (wraps KeyPub)
//! - `OpId`: operation identifier (user, seq)
//! - `ItemId`: character identifier (user, seq, offset)
//!
//! ## Spans
//! - `CompactSpan`: minimal memory footprint
//! - `RichSpan`: with metadata for debugging
//! - `RunLengthSpan`: for highly repetitive content
//!
//! ## Caches
//! - `CursorCache`: amortize sequential lookups
//! - `IdCache`: fast ID to position mapping
//! - `LruCache`: general purpose with eviction

pub mod clock;
pub mod cursor;
pub mod id;
pub mod range_tree;
pub mod span;
pub mod user_table;

// Re-exports for convenience
pub use clock::LamportClock;
pub use clock::VectorClock;
pub use cursor::CursorCache;
pub use cursor::SpanLocation;
pub use cursor::BTreeLocation;
pub use id::OpId;
pub use id::ItemId;
pub use id::CompactOpId;
pub use id::UserIdx;
pub use span::CompactSpan;
pub use user_table::UserTable;
