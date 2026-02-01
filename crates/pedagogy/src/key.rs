// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

//! Simple key types for CRDT identity.
//!
//! These types provide the identity primitives needed for CRDTs without
//! cryptographic dependencies. For cryptographic operations (signing,
//! encryption), use the full key module from the `together` crate.

use std::fmt;
use std::hash::Hash as StdHash;

/// A public key identifier, 32 bytes.
///
/// This is used to identify users/replicas in CRDTs. It implements the
/// traits needed for use as a user identifier: Clone, Eq, Hash, Ord.
///
/// When used with the `together` crate, this is the same as the cryptographic
/// public key. In the standalone `pedagogy` crate, it's just a unique identifier.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct KeyPub(pub [u8; 32]);

impl StdHash for KeyPub {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl KeyPub {
    /// Create a KeyPub from raw bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> KeyPub {
        return KeyPub(bytes);
    }

    /// Get the raw bytes of this key.
    pub fn as_bytes(&self) -> &[u8; 32] {
        return &self.0;
    }
}

fn hex(bytes: &[u8]) -> String {
    return bytes.iter().map(|b| format!("{:02x}", b)).collect();
}

impl fmt::Debug for KeyPub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(f, "KeyPub({})", hex(&self.0));
    }
}

/// A blake3 hash, 32 bytes.
///
/// Used in log_integration for operation chaining.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Hash(pub [u8; 32]);

impl StdHash for Hash {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(f, "Hash({})", hex(&self.0));
    }
}

/// A simple keypair for testing.
///
/// This generates random keys for testing purposes. For real cryptographic
/// operations, use the full KeyPair from the `together` crate.
#[derive(Clone, PartialEq, Eq)]
pub struct KeyPair {
    pub key_pub: KeyPub,
}

impl KeyPair {
    /// Generate a random keypair for testing.
    ///
    /// Uses a simple counter-based approach for deterministic testing.
    /// Not suitable for real cryptographic use.
    pub fn generate() -> KeyPair {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);

        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut bytes = [0u8; 32];
        bytes[0..8].copy_from_slice(&counter.to_le_bytes());

        // Add some entropy-like variation using a simple hash-like mixing
        // This is NOT cryptographically secure, just for testing uniqueness
        for i in 8..32 {
            bytes[i] = bytes[i - 8].wrapping_add(bytes[i - 1]).wrapping_mul(31);
        }

        return KeyPair {
            key_pub: KeyPub(bytes),
        };
    }
}

impl fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(f, "KeyPair {{ pub: {:?} }}", self.key_pub);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keypair_generates_unique_keys() {
        let a = KeyPair::generate();
        let b = KeyPair::generate();
        assert_ne!(a.key_pub, b.key_pub);
    }

    #[test]
    fn keypub_ordering() {
        let a = KeyPub::from_bytes([0u8; 32]);
        let mut b_bytes = [0u8; 32];
        b_bytes[0] = 1;
        let b = KeyPub::from_bytes(b_bytes);
        assert!(a < b);
    }

    #[test]
    fn keypub_hash() {
        use std::collections::HashSet;
        let a = KeyPair::generate().key_pub;
        let b = KeyPair::generate().key_pub;
        let mut set = HashSet::new();
        set.insert(a);
        set.insert(b);
        assert_eq!(set.len(), 2);
    }
}
