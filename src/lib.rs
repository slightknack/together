// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-01-31"
// driver = "Isaac Clayton"

//! Together - A collaborative text editing library using CRDTs.
//!
//! # Quick Start
//!
//! ```
//! use together::crdt::rga::RgaBuf;
//! use together::key::KeyPair;
//!
//! // Create a user identity
//! let user = KeyPair::generate();
//!
//! // Create a new document
//! let mut doc = RgaBuf::new();
//!
//! // Edit the document
//! doc.insert(&user.key_pub, 0, b"Hello, World!");
//! assert_eq!(doc.to_string(), "Hello, World!");
//! ```

pub mod crdt;
pub mod key;
pub mod log;
