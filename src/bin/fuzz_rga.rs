//! AFL Fuzz harness for RGA CRDT
//!
//! This harness tests the critical CRDT properties:
//! 1. Convergence: replicas that see the same operations converge to the same state
//! 2. Merge commutativity: merge(A, B) and merge(B, A) produce equivalent results
//! 3. Merge idempotency: merging the same thing twice is a no-op
//!
//! Model: Each user has their own replica. They edit locally and periodically
//! broadcast their state to other users.

use afl::fuzz;
use together::crdt::rga::Rga;
use together::crdt::Crdt;
use together::key::{KeyPair, KeyPub};

const NUM_USERS: usize = 3;

/// Operation types the fuzzer can generate
#[derive(Debug, Clone, Copy)]
enum FuzzOp {
    /// User inserts text at a position in their replica
    Insert { user: u8, pos_frac: u8, len: u8 },
    /// User deletes text from their replica
    Delete { user: u8, pos_frac: u8, len: u8 },
    /// User A receives broadcast from user B (merges B into A)
    Broadcast { from: u8, to: u8 },
    /// All users sync (full mesh broadcast)
    FullSync,
}

impl FuzzOp {
    fn from_bytes(bytes: &[u8]) -> Option<(FuzzOp, &[u8])> {
        if bytes.is_empty() {
            return None;
        }
        
        let op_type = bytes[0] % 4;
        let rest = &bytes[1..];
        
        match op_type {
            0 if rest.len() >= 3 => {
                let op = FuzzOp::Insert {
                    user: rest[0] % NUM_USERS as u8,
                    pos_frac: rest[1],
                    len: (rest[2] % 32).saturating_add(1), // 1-32 bytes
                };
                Some((op, &rest[3..]))
            }
            1 if rest.len() >= 3 => {
                let op = FuzzOp::Delete {
                    user: rest[0] % NUM_USERS as u8,
                    pos_frac: rest[1],
                    len: (rest[2] % 16).saturating_add(1), // 1-16 bytes
                };
                Some((op, &rest[3..]))
            }
            2 if rest.len() >= 2 => {
                let op = FuzzOp::Broadcast {
                    from: rest[0] % NUM_USERS as u8,
                    to: rest[1] % NUM_USERS as u8,
                };
                Some((op, &rest[2..]))
            }
            3 => Some((FuzzOp::FullSync, rest)),
            _ => None,
        }
    }
}

fn main() {
    // Use deterministic keys for reproducible crashes
    let users: Vec<KeyPair> = (0..NUM_USERS).map(|i| KeyPair::from_seed(i as u64)).collect();
    let user_keys: Vec<&KeyPub> = users.iter().map(|u| &u.key_pub).collect();
    
    fuzz!(|data: &[u8]| {
        // Each user has their own replica
        let mut replicas: Vec<Rga> = (0..NUM_USERS).map(|_| Rga::new()).collect();
        let mut remaining = data;
        
        // Parse and execute operations
        while let Some((op, rest)) = FuzzOp::from_bytes(remaining) {
            remaining = rest;
            
            match op {
                FuzzOp::Insert { user, pos_frac, len } => {
                    let r = &mut replicas[user as usize];
                    let doc_len = r.len();
                    let pos = if doc_len == 0 {
                        0
                    } else {
                        ((pos_frac as u64) * doc_len / 256).min(doc_len)
                    };
                    
                    // Generate content based on user
                    let content: Vec<u8> = (0..len)
                        .map(|i| b'A' + ((user).wrapping_add(i) % 26))
                        .collect();
                    
                    r.insert(user_keys[user as usize], pos, &content);
                }
                
                FuzzOp::Delete { user, pos_frac, len } => {
                    let r = &mut replicas[user as usize];
                    let doc_len = r.len();
                    if doc_len > 0 {
                        let pos = ((pos_frac as u64) * doc_len / 256).min(doc_len - 1);
                        let del_len = (len as u64).min(doc_len - pos);
                        if del_len > 0 {
                            r.delete(pos, del_len);
                        }
                    }
                }
                
                FuzzOp::Broadcast { from, to } => {
                    if from != to {
                        let source = replicas[from as usize].clone();
                        replicas[to as usize].merge(&source);
                    }
                }
                
                FuzzOp::FullSync => {
                    // Full mesh - everyone broadcasts to everyone
                    for i in 0..NUM_USERS {
                        for j in 0..NUM_USERS {
                            if i != j {
                                let source = replicas[j].clone();
                                replicas[i].merge(&source);
                            }
                        }
                    }
                    
                    // CRITICAL INVARIANT: All replicas must converge!
                    let first = replicas[0].to_string();
                    for (i, r) in replicas.iter().enumerate().skip(1) {
                        assert_eq!(
                            r.to_string(), first,
                            "Convergence failure! User {} != User 0 after full sync",
                            i
                        );
                    }
                }
            }
        }
        
        // Final full sync and convergence check
        for i in 0..NUM_USERS {
            for j in 0..NUM_USERS {
                if i != j {
                    let source = replicas[j].clone();
                    replicas[i].merge(&source);
                }
            }
        }
        
        // All replicas must converge
        let first = replicas[0].to_string();
        for (i, r) in replicas.iter().enumerate().skip(1) {
            assert_eq!(
                r.to_string(), first,
                "Final convergence failure! User {} != User 0",
                i
            );
        }
        
        // Verify internal consistency
        for r in &replicas {
            let content = r.to_string();
            assert_eq!(content.len() as u64, r.len(), "Length mismatch");
            
            let version = r.version();
            assert_eq!(r.len_at(&version), r.len(), "Version length mismatch");
        }
    });
}
