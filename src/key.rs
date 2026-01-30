// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-01-30"
// driver = "Isaac Clayton"

use blake3::Hasher;
use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::XNonce;
use chacha20poly1305::aead::Aead;
use chacha20poly1305::aead::KeyInit;
use ed25519_dalek::Signer;
use ed25519_dalek::SigningKey;
use ed25519_dalek::Verifier;
use ed25519_dalek::VerifyingKey;
use rand_core::OsRng;
use rand_core::RngCore;
use x25519_dalek::PublicKey as X25519Public;
use x25519_dalek::StaticSecret;

/// A public key, 32 bytes on the ed25519 curve.
#[derive(Clone, PartialEq, Eq)]
pub struct KeyPub(pub [u8; 32]);

/// A secret key, 32 bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct KeySec(pub [u8; 32]);

/// A keypair bundles a public and secret key together.
#[derive(Clone, PartialEq, Eq)]
pub struct KeyPair {
    pub key_pub: KeyPub,
    pub key_sec: KeySec,
}

/// A shared secret derived via Diffie-Hellman, 32 bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct KeyShared(pub [u8; 32]);

/// A signature, 64 bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct Signature(pub [u8; 64]);

/// A blake3 hash, 32 bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct Hash(pub [u8; 32]);

/// An encrypted payload: nonce plus ciphertext.
#[derive(Clone, PartialEq, Eq)]
pub struct Payload {
    pub nonce: [u8; 24],
    pub ciphertext: Vec<u8>,
}

/// Error returned when decryption fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecryptError {
    /// The ciphertext was tampered with or the key is wrong.
    AuthenticationFailed,
}

/// Hash a message using blake3.
pub fn hash(message: &[u8]) -> Hash {
    let mut hasher = Hasher::new();
    hasher.update(message);
    let result = hasher.finalize();
    return Hash(*result.as_bytes());
}

impl KeyPair {
    /// Generate a random keypair.
    pub fn generate() -> KeyPair {
        let signing = SigningKey::generate(&mut OsRng);
        let verifying = signing.verifying_key();
        return KeyPair {
            key_pub: KeyPub(verifying.to_bytes()),
            key_sec: KeySec(signing.to_bytes()),
        };
    }

    /// Sign a message.
    pub fn sign(&self, message: &[u8]) -> Signature {
        let signing = SigningKey::from_bytes(&self.key_sec.0);
        return Signature(signing.sign(message).to_bytes());
    }

    /// Derive a shared secret with another party's public key.
    pub fn conspire(&self, other: &KeyPub) -> KeyShared {
        let signing = SigningKey::from_bytes(&self.key_sec.0);
        let scalar = signing.to_scalar_bytes();
        let secret = StaticSecret::from(scalar);

        let verifying = VerifyingKey::from_bytes(&other.0).expect("invalid public key");
        let montgomery = verifying.to_montgomery();
        let public = X25519Public::from(*montgomery.as_bytes());

        let shared = secret.diffie_hellman(&public);
        return KeyShared(*shared.as_bytes());
    }
}

impl KeyPub {
    /// Verify a signature against this public key.
    pub fn verify(&self, message: &[u8], signature: &Signature) -> bool {
        let verifying = match VerifyingKey::from_bytes(&self.0) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let sig = ed25519_dalek::Signature::from_bytes(&signature.0);
        return verifying.verify(message, &sig).is_ok();
    }
}

impl KeyShared {
    /// Encrypt a message using XChaCha20-Poly1305.
    pub fn encrypt(&self, message: &[u8]) -> Payload {
        let cipher = XChaCha20Poly1305::new_from_slice(&self.0)
            .expect("key is 32 bytes");
        let mut nonce_bytes = [0u8; 24];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = XNonce::from_slice(&nonce_bytes);
        let ciphertext = cipher.encrypt(nonce, message)
            .expect("encryption failed");
        return Payload {
            nonce: nonce_bytes,
            ciphertext,
        };
    }

    /// Decrypt a message using XChaCha20-Poly1305.
    pub fn decrypt(&self, payload: &Payload) -> Result<Vec<u8>, DecryptError> {
        let cipher = XChaCha20Poly1305::new_from_slice(&self.0)
            .expect("key is 32 bytes");
        let nonce = XNonce::from_slice(&payload.nonce);
        return cipher
            .decrypt(nonce, payload.ciphertext.as_ref())
            .map_err(|_| DecryptError::AuthenticationFailed);
    }
}

fn hex(bytes: &[u8]) -> String {
    return bytes.iter().map(|b| format!("{:02x}", b)).collect();
}

impl std::fmt::Debug for KeyPub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return write!(f, "KeyPub({})", hex(&self.0));
    }
}

impl std::fmt::Debug for KeySec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return write!(f, "KeySec({})", hex(&self.0));
    }
}

impl std::fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return write!(f, "KeyPair {{ pub: {}, sec: {} }}", hex(&self.key_pub.0), hex(&self.key_sec.0));
    }
}

impl std::fmt::Debug for KeyShared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return write!(f, "KeyShared({})", hex(&self.0));
    }
}

impl std::fmt::Debug for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return write!(f, "Signature({})", hex(&self.0));
    }
}

impl std::fmt::Debug for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return write!(f, "Hash({})", hex(&self.0));
    }
}

impl std::fmt::Debug for Payload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return write!(f, "Payload {{ nonce: {}, ciphertext: {} bytes }}", hex(&self.nonce), self.ciphertext.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify() {
        let pair = KeyPair::generate();
        let message = b"hello world";
        let signature = pair.sign(message);
        assert!(pair.key_pub.verify(message, &signature));
    }

    #[test]
    fn verify_rejects_wrong_message() {
        let pair = KeyPair::generate();
        let signature = pair.sign(b"hello world");
        assert!(!pair.key_pub.verify(b"wrong message", &signature));
    }

    #[test]
    fn verify_rejects_wrong_key() {
        let pair_a = KeyPair::generate();
        let pair_b = KeyPair::generate();
        let signature = pair_a.sign(b"hello world");
        assert!(!pair_b.key_pub.verify(b"hello world", &signature));
    }

    #[test]
    fn conspire_produces_same_shared_secret() {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();
        let shared_a = alice.conspire(&bob.key_pub);
        let shared_b = bob.conspire(&alice.key_pub);
        assert_eq!(shared_a, shared_b);
    }

    #[test]
    fn hash_is_deterministic() {
        let a = hash(b"hello world");
        let b = hash(b"hello world");
        assert_eq!(a, b);
    }

    #[test]
    fn hash_differs_for_different_input() {
        let a = hash(b"hello world");
        let b = hash(b"hello world!");
        assert_ne!(a, b);
    }

    #[test]
    fn encrypt_and_decrypt() {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();
        let shared = alice.conspire(&bob.key_pub);
        let message = b"secret message";
        let payload = shared.encrypt(message);
        let decrypted = shared.decrypt(&payload).unwrap();
        assert_eq!(decrypted, message);
    }

    #[test]
    fn decrypt_fails_with_wrong_key() {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();
        let eve = KeyPair::generate();
        let shared_alice_bob = alice.conspire(&bob.key_pub);
        let shared_alice_eve = alice.conspire(&eve.key_pub);
        let payload = shared_alice_bob.encrypt(b"secret");
        let result = shared_alice_eve.decrypt(&payload);
        assert!(result.is_err());
    }

    #[test]
    fn decrypt_fails_with_tampered_ciphertext() {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();
        let shared = alice.conspire(&bob.key_pub);
        let mut payload = shared.encrypt(b"secret");
        payload.ciphertext[0] ^= 0xff;
        let result = shared.decrypt(&payload);
        assert!(result.is_err());
    }
}
