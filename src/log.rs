// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-01-30"
// driver = "Isaac Clayton"

//! Signed append-only logs with a 16-tree merkle structure.
//!
//! A log is a sequence of blocks that can be verified using the author's
//! public key. The blocks are arranged into a 16-ary merkle tree, with
//! the roots signed along with the log length.

use crate::key::Hash;
use crate::key::KeyPair;
use crate::key::KeyPub;
use crate::key::Signature;

/// Type constant for leaf node hashes (block hashes).
pub const TYPE_LEAF: u8 = 0x00;

/// Type constant for parent node hashes (internal tree nodes).
pub const TYPE_PARENT: u8 = 0x01;

/// Type constant for root hash computation.
pub const TYPE_ROOT: u8 = 0x02;

/// The branching factor of the tree. 16 allows up to 2^64 blocks.
pub const BRANCHING: usize = 16;

/// Hash a block to produce a leaf hash with domain separation.
pub fn hash_leaf(data: &[u8]) -> Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&[TYPE_LEAF]);
    hasher.update(&(data.len() as u64).to_le_bytes());
    hasher.update(data);
    return Hash(*hasher.finalize().as_bytes());
}

/// Hash a set of child hashes to produce a parent hash with domain separation.
pub fn hash_parent(children: &[Hash]) -> Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&[TYPE_PARENT]);
    hasher.update(&(children.len() as u64).to_le_bytes());
    for child in children {
        hasher.update(&child.0);
    }
    return Hash(*hasher.finalize().as_bytes());
}

/// Compute the signable message from roots and length.
// TODO: Use neopack for serialization once it's published. Bug the driver about it!
fn signable(roots: &[Hash], length: u64) -> Vec<u8> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&[TYPE_ROOT]);
    hasher.update(&length.to_le_bytes());
    hasher.update(&(roots.len() as u64).to_le_bytes());
    for root in roots {
        hasher.update(&root.0);
    }
    return hasher.finalize().as_bytes().to_vec();
}

/// A signed append-only log.
pub struct Log {
    keypair: KeyPair,
    blocks: Vec<Vec<u8>>,
}

/// A signed snapshot of a log, verifiable without the secret key.
#[derive(Clone)]
pub struct SignedLog {
    pub author: KeyPub,
    pub length: u64,
    pub roots: Vec<Hash>,
    pub signature: Signature,
}

/// A proof that a block belongs to a signed log.
#[derive(Clone)]
pub struct Proof {
    /// Sibling hashes at each level, from leaf to root.
    /// Each entry contains the siblings and the index of our hash among them.
    pub levels: Vec<ProofLevel>,
}

/// One level of a membership proof.
#[derive(Clone)]
pub struct ProofLevel {
    /// The sibling hashes at this level (not including the hash we're proving).
    pub siblings: Vec<Hash>,
    /// Our position among the siblings (0..BRANCHING).
    pub position: usize,
}

impl Log {
    /// Create a new empty log with the given keypair.
    pub fn new(keypair: KeyPair) -> Log {
        return Log {
            keypair,
            blocks: Vec::new(),
        };
    }

    /// Return the number of blocks in the log.
    pub fn len(&self) -> u64 {
        return self.blocks.len() as u64;
    }

    /// Return true if the log is empty.
    pub fn is_empty(&self) -> bool {
        return self.blocks.is_empty();
    }

    /// Append a block to the log.
    pub fn append(&mut self, data: &[u8]) {
        self.blocks.push(data.to_vec());
    }

    /// Get a block by index.
    pub fn block(&self, index: u64) -> Option<&[u8]> {
        return self.blocks.get(index as usize).map(|v| v.as_slice());
    }

    /// Compute the current roots of the 16-tree.
    fn compute_roots(&self) -> Vec<Hash> {
        if self.blocks.is_empty() {
            return Vec::new();
        }

        // Start with leaf hashes
        let mut current: Vec<Hash> = self.blocks.iter().map(|b| hash_leaf(b)).collect();

        // Repeatedly collapse groups of BRANCHING into parent hashes.
        // We stop when no full groups remain to collapse.
        loop {
            // Count how many full groups we have
            let full_groups = current.len() / BRANCHING;
            if full_groups == 0 {
                break;
            }

            let mut next = Vec::new();
            let mut i = 0;
            while i < current.len() {
                let end = (i + BRANCHING).min(current.len());
                let group = &current[i..end];
                if group.len() == BRANCHING {
                    // Full group: collapse into parent
                    next.push(hash_parent(group));
                } else {
                    // Partial group: keep as separate roots
                    next.extend(group.iter().cloned());
                }
                i += BRANCHING;
            }
            current = next;
        }

        return current;
    }

    /// Sign the current state of the log.
    pub fn sign(&self) -> SignedLog {
        let roots = self.compute_roots();
        let message = signable(&roots, self.len());
        let signature = self.keypair.sign(&message);
        return SignedLog {
            author: self.keypair.key_pub.clone(),
            length: self.len(),
            roots,
            signature,
        };
    }

    /// Generate a proof that a block belongs to this log.
    pub fn proof(&self, index: u64) -> Option<Proof> {
        if index >= self.len() {
            return None;
        }

        let mut levels = Vec::new();
        let mut current_hashes: Vec<Hash> = self.blocks.iter().map(|b| hash_leaf(b)).collect();
        let mut current_index = index as usize;

        // Walk up the tree, collecting siblings at each level
        loop {
            let full_groups = current_hashes.len() / BRANCHING;
            if full_groups == 0 {
                break;
            }

            // Find which group this index belongs to
            let group_index = current_index / BRANCHING;
            let group_start = group_index * BRANCHING;
            let group_end = (group_start + BRANCHING).min(current_hashes.len());
            let position = current_index % BRANCHING;

            // Only process if this is a full group
            if group_end - group_start == BRANCHING {
                // Collect siblings (all hashes in group except our position)
                let mut siblings = Vec::new();
                for i in group_start..group_end {
                    if i != current_index {
                        siblings.push(current_hashes[i].clone());
                    }
                }
                levels.push(ProofLevel { siblings, position });
            }

            // Collapse to next level
            let mut next = Vec::new();
            let mut i = 0;
            while i < current_hashes.len() {
                let end = (i + BRANCHING).min(current_hashes.len());
                let group = &current_hashes[i..end];
                if group.len() == BRANCHING {
                    next.push(hash_parent(group));
                } else {
                    next.extend(group.iter().cloned());
                }
                i += BRANCHING;
            }
            current_hashes = next;
            current_index = group_index;
        }

        return Some(Proof { levels });
    }
}

impl SignedLog {
    /// Verify that this signed log is valid.
    pub fn verify(&self) -> bool {
        let message = signable(&self.roots, self.length);
        return self.author.verify(&message, &self.signature);
    }

    /// Verify that a block belongs to this signed log.
    pub fn verify_proof(&self, index: u64, data: &[u8], proof: &Proof) -> bool {
        if index >= self.length {
            return false;
        }

        // Start with the leaf hash
        let mut current = hash_leaf(data);
        let mut current_index = index as usize;

        // Walk up through the proof levels
        for level in &proof.levels {
            // The position in the proof must match the expected position from the index
            let expected_position = current_index % BRANCHING;
            if level.position != expected_position {
                return false;
            }

            // Reconstruct the full group of children
            let mut children = Vec::with_capacity(BRANCHING);
            let mut sibling_iter = level.siblings.iter();

            for i in 0..BRANCHING {
                if i == level.position {
                    children.push(current.clone());
                } else if let Some(sibling) = sibling_iter.next() {
                    children.push(sibling.clone());
                } else {
                    // Not enough siblings - invalid proof
                    return false;
                }
            }

            current = hash_parent(&children);
            current_index /= BRANCHING;
        }

        // Check if the computed hash matches one of the roots
        return self.roots.contains(&current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::KeyPair;

    #[test]
    fn empty_log_has_zero_length() {
        let pair = KeyPair::generate();
        let log = Log::new(pair);
        assert_eq!(log.len(), 0);
    }

    #[test]
    fn append_increases_length() {
        let pair = KeyPair::generate();
        let mut log = Log::new(pair);
        log.append(b"block 0");
        assert_eq!(log.len(), 1);
        log.append(b"block 1");
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn can_retrieve_appended_blocks() {
        let pair = KeyPair::generate();
        let mut log = Log::new(pair);
        log.append(b"hello");
        log.append(b"world");
        assert_eq!(log.block(0), Some(b"hello".as_slice()));
        assert_eq!(log.block(1), Some(b"world".as_slice()));
        assert_eq!(log.block(2), None);
    }

    #[test]
    fn sign_produces_valid_signature() {
        let pair = KeyPair::generate();
        let mut log = Log::new(pair);
        log.append(b"block 0");
        log.append(b"block 1");
        let signed = log.sign();
        assert!(signed.verify());
    }

    #[test]
    fn signature_covers_length() {
        let pair = KeyPair::generate();
        let mut log = Log::new(pair);
        log.append(b"block 0");
        let signed_1 = log.sign();
        log.append(b"block 1");
        let signed_2 = log.sign();
        // Different lengths mean different signatures
        assert_ne!(signed_1.signature, signed_2.signature);
    }

    #[test]
    fn verification_fails_with_wrong_key() {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();
        let mut log = Log::new(alice);
        log.append(b"data");
        let signed = log.sign();
        // Forge a SignedLog with bob's key
        let forged = SignedLog {
            author: bob.key_pub.clone(),
            length: signed.length,
            roots: signed.roots.clone(),
            signature: signed.signature.clone(),
        };
        assert!(!forged.verify());
    }

    #[test]
    fn verification_fails_with_tampered_length() {
        let pair = KeyPair::generate();
        let mut log = Log::new(pair);
        log.append(b"data");
        let mut signed = log.sign();
        signed.length += 1;
        assert!(!signed.verify());
    }

    #[test]
    fn verification_fails_with_tampered_roots() {
        let pair = KeyPair::generate();
        let mut log = Log::new(pair);
        log.append(b"data");
        let mut signed = log.sign();
        if !signed.roots.is_empty() {
            signed.roots[0].0[0] ^= 0xff;
        }
        assert!(!signed.verify());
    }

    #[test]
    fn leaf_hash_uses_domain_separation() {
        // The same data should produce different hashes when used as
        // a leaf vs when hashed directly without the type prefix.
        let data = b"block data";
        let leaf = hash_leaf(data);
        let plain = crate::key::hash(data);
        assert_ne!(leaf, plain);
    }

    #[test]
    fn parent_hash_uses_domain_separation() {
        let child1 = Hash([1u8; 32]);
        let child2 = Hash([2u8; 32]);
        let parent = hash_parent(&[child1.clone(), child2.clone()]);
        // Parent hash should include TYPE_PARENT prefix
        let mut direct = blake3::Hasher::new();
        direct.update(&child1.0);
        direct.update(&child2.0);
        let direct_result = Hash(*direct.finalize().as_bytes());
        assert_ne!(parent, direct_result);
    }

    #[test]
    fn single_block_has_one_root() {
        let pair = KeyPair::generate();
        let mut log = Log::new(pair);
        log.append(b"only block");
        let signed = log.sign();
        assert_eq!(signed.roots.len(), 1);
    }

    #[test]
    fn sixteen_blocks_have_one_root() {
        let pair = KeyPair::generate();
        let mut log = Log::new(pair);
        for i in 0..16 {
            log.append(format!("block {}", i).as_bytes());
        }
        let signed = log.sign();
        // 16 blocks should collapse into exactly one root
        assert_eq!(signed.roots.len(), 1);
    }

    #[test]
    fn seventeen_blocks_have_two_roots() {
        let pair = KeyPair::generate();
        let mut log = Log::new(pair);
        for i in 0..17 {
            log.append(format!("block {}", i).as_bytes());
        }
        let signed = log.sign();
        // 17 blocks: 16 collapse into one root, plus 1 extra root
        assert_eq!(signed.roots.len(), 2);
    }

    #[test]
    fn proof_verifies_block_membership() {
        let pair = KeyPair::generate();
        let mut log = Log::new(pair);
        for i in 0..20 {
            log.append(format!("block {}", i).as_bytes());
        }
        let signed = log.sign();
        // Get a proof for block 7
        let proof = log.proof(7).expect("block exists");
        // Verify the proof against the signed log
        assert!(signed.verify_proof(7, b"block 7", &proof));
    }

    #[test]
    fn proof_rejects_wrong_data() {
        let pair = KeyPair::generate();
        let mut log = Log::new(pair);
        for i in 0..20 {
            log.append(format!("block {}", i).as_bytes());
        }
        let signed = log.sign();
        let proof = log.proof(7).expect("block exists");
        // Try to verify with wrong data
        assert!(!signed.verify_proof(7, b"wrong data", &proof));
    }

    #[test]
    fn proof_rejects_wrong_index() {
        let pair = KeyPair::generate();
        let mut log = Log::new(pair);
        for i in 0..20 {
            log.append(format!("block {}", i).as_bytes());
        }
        let signed = log.sign();
        let proof = log.proof(7).expect("block exists");
        // Try to verify at wrong index
        assert!(!signed.verify_proof(8, b"block 7", &proof));
    }

    #[test]
    fn roots_bounded_by_branching_factor() {
        // No matter how many blocks, roots should never exceed BRANCHING
        let pair = KeyPair::generate();
        let mut log = Log::new(pair);
        for i in 0..1000 {
            log.append(format!("block {}", i).as_bytes());
        }
        let signed = log.sign();
        assert!(signed.roots.len() <= BRANCHING);
    }
}
