//! Reproduce AFL crashes without AFL instrumentation
//!
//! Usage: cargo run --bin repro_crash -- <crash_file>

use std::fs;
use together::crdt::rga::Rga;
use together::crdt::Crdt;
use together::key::{KeyPair, KeyPub};

const NUM_USERS: usize = 3;

#[derive(Debug, Clone, Copy)]
enum FuzzOp {
    Insert { user: u8, pos_frac: u8, len: u8 },
    Delete { user: u8, pos_frac: u8, len: u8 },
    Broadcast { from: u8, to: u8 },
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
                    len: (rest[2] % 32).saturating_add(1),
                };
                Some((op, &rest[3..]))
            }
            1 if rest.len() >= 3 => {
                let op = FuzzOp::Delete {
                    user: rest[0] % NUM_USERS as u8,
                    pos_frac: rest[1],
                    len: (rest[2] % 16).saturating_add(1),
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
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <crash_file>", args[0]);
        std::process::exit(1);
    }
    let data = fs::read(&args[1]).expect("Failed to read file");
    
    eprintln!("Input: {} bytes", data.len());
    eprintln!("Hex: {}", data.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" "));
    
    // Use deterministic keys to match the fuzzer
    let users: Vec<KeyPair> = (0..NUM_USERS).map(|i| KeyPair::from_seed(i as u64)).collect();
    let user_keys: Vec<&KeyPub> = users.iter().map(|u| &u.key_pub).collect();
    
    let mut replicas: Vec<Rga> = (0..NUM_USERS).map(|_| Rga::new()).collect();
    let mut remaining = data.as_slice();
    let mut op_num = 0;
    
    while let Some((op, rest)) = FuzzOp::from_bytes(remaining) {
        remaining = rest;
        op_num += 1;
        
        match op {
            FuzzOp::Insert { user, pos_frac, len } => {
                let r = &mut replicas[user as usize];
                let doc_len = r.len();
                let pos = if doc_len == 0 {
                    0
                } else {
                    ((pos_frac as u64) * doc_len / 256).min(doc_len)
                };
                
                let content: Vec<u8> = (0..len)
                    .map(|i| b'A' + ((user).wrapping_add(i) % 26))
                    .collect();
                
                eprintln!("Op {}: User {} inserts at pos={} len={}", op_num, user, pos, len);
                eprintln!("  Before: len={} {:?}", r.len(), r.to_string());
                
                r.insert(user_keys[user as usize], pos, &content);
                
                eprintln!("  After: len={} {:?}", r.len(), r.to_string());
            }
            
            FuzzOp::Delete { user, pos_frac, len } => {
                let r = &mut replicas[user as usize];
                let doc_len = r.len();
                if doc_len > 0 {
                    let pos = ((pos_frac as u64) * doc_len / 256).min(doc_len - 1);
                    let del_len = (len as u64).min(doc_len - pos);
                    
                    eprintln!("Op {}: User {} deletes at pos={} len={}", op_num, user, pos, del_len);
                    eprintln!("  Before: len={} {:?}", r.len(), r.to_string());
                    
                    if del_len > 0 {
                        r.delete(pos, del_len);
                    }
                    
                    eprintln!("  After: len={} {:?}", r.len(), r.to_string());
                } else {
                    eprintln!("Op {}: User {} delete (skipped, empty)", op_num, user);
                }
            }
            
            FuzzOp::Broadcast { from, to } => {
                eprintln!("Op {}: Broadcast User {} -> User {}", op_num, from, to);
                if from != to {
                    eprintln!("  Before U{}: {:?}", to, replicas[to as usize].to_string());
                    eprintln!("  Source U{}: {:?}", from, replicas[from as usize].to_string());
                    
                    let source = replicas[from as usize].clone();
                    replicas[to as usize].merge(&source);
                    
                    eprintln!("  After U{}: {:?}", to, replicas[to as usize].to_string());
                } else {
                    eprintln!("  (self-broadcast, skipped)");
                }
            }
            
            FuzzOp::FullSync => {
                eprintln!("Op {}: FullSync", op_num);
                eprintln!("  Before:");
                for (i, r) in replicas.iter().enumerate() {
                    eprintln!("    U{}: {:?}", i, r.to_string());
                }
                
                for i in 0..NUM_USERS {
                    for j in 0..NUM_USERS {
                        if i != j {
                            let source = replicas[j].clone();
                            replicas[i].merge(&source);
                        }
                    }
                }
                
                eprintln!("  After:");
                for (i, r) in replicas.iter().enumerate() {
                    eprintln!("    U{}: {:?}", i, r.to_string());
                }
                
                // Check convergence
                let first = replicas[0].to_string();
                for (i, r) in replicas.iter().enumerate().skip(1) {
                    assert_eq!(
                        r.to_string(), first,
                        "Convergence failure! U{} != U0",
                        i
                    );
                }
                eprintln!("  Convergence check: PASSED");
            }
        }
    }
    
    eprintln!("\n=== Final full sync ===");
    for i in 0..NUM_USERS {
        for j in 0..NUM_USERS {
            if i != j {
                let source = replicas[j].clone();
                replicas[i].merge(&source);
            }
        }
    }
    
    eprintln!("Final state:");
    for (i, r) in replicas.iter().enumerate() {
        eprintln!("  U{}: len={} {:?}", i, r.len(), r.to_string());
    }
    
    // Final convergence check
    let first = replicas[0].to_string();
    for (i, r) in replicas.iter().enumerate().skip(1) {
        assert_eq!(r.to_string(), first, "Final convergence failure! U{} != U0", i);
    }
    
    eprintln!("\nAll checks passed!");
}
