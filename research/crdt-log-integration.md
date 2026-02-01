+++
model = "claude-opus-4-5"
created = 2026-02-01
modified = 2026-02-01
driver = "Isaac Clayton"
+++

# CRDT Log Integration Design

This document describes how the OptimizedRga CRDT integrates with the signed append-only log infrastructure from DESIGN.md.

## Overview

The log integration provides three key capabilities:

1. **Export**: Convert RGA state to a sequence of operations
2. **Replay**: Rebuild RGA state from operations
3. **Determinism**: Same operations produce same result (with causal ordering)

## Operation Format

### Operation Types

```rust
pub enum Operation {
    Insert {
        user: KeyPub,           // 32 bytes
        seq: u32,               // Starting sequence number
        origin_left: Option<OperationId>,   // Left neighbor when inserted
        origin_right: Option<OperationId>,  // Right neighbor when inserted
        content: Vec<u8>,       // The inserted bytes
    },
    Delete {
        target_user: KeyPub,    // User whose content is deleted
        target_seq: u32,        // Starting sequence number
        len: u32,               // Number of characters
    },
}
```

### OperationId

```rust
pub struct OperationId {
    pub user: KeyPub,  // 32 bytes
    pub seq: u32,      // 4 bytes
}
```

An OperationId uniquely identifies a character in the RGA. Unlike positional indices (which shift), OperationIds are stable because they're based on (user, seq) pairs.

### Binary Encoding

Operations use a compact binary encoding:

```
Insert:
+----------+----------+----------+----------+----------+----------+
| type (1) | user(32) | seq (4)  | left_or  | right_or | content  |
|   0x01   |  pubkey  |  u32 LE  |  option  |  option  |  bytes   |
+----------+----------+----------+----------+----------+----------+

Delete:
+----------+----------+----------+----------+
| type (1) | user(32) | seq (4)  | len (4)  |
|   0x02   |  pubkey  |  u32 LE  |  u32 LE  |
+----------+----------+----------+----------+

Option encoding:
  0x00 = None
  0x01 + OperationId = Some(id)

OperationId encoding:
  user (32 bytes) + seq (4 bytes, little-endian)
```

## Integration with Signed Log

### Log Entry Structure

Each operation is wrapped in a LogEntry for storage:

```rust
pub struct LogEntry {
    pub operation: Operation,
    pub parent_hash: Option<Hash>,  // For chaining
    pub signature_placeholder: [u8; 64],
}
```

### Parent Hash Chaining

The parent_hash field creates a hash chain within each user's log:

```
Entry 0: parent_hash = None
Entry 1: parent_hash = hash(Entry 0)
Entry 2: parent_hash = hash(Entry 1)
...
```

This ensures:
- Operations cannot be reordered without detection
- Missing operations are detectable
- Forks are detectable (multiple entries with same parent)

### Integration with 16-Tree

From DESIGN.md, the signed append-only log uses a 16-ary merkle tree:

```
           H01234567
          /         \
     H0123           H4567
    /     \         /     \
  H01     H23     H45     H67
 /  \    /  \    /  \    /  \
H0  H1  H2  H3  H4  H5  H6  H7
 |   |   |   |   |   |   |   |
B0  B1  B2  B3  B4  B5  B6  B7
```

Each block (B0, B1, ...) contains a serialized LogEntry. The tree structure allows:
- Efficient verification of individual operations
- O(log n) membership proofs
- Efficient sync (exchange only missing subtrees)

### Workflow

1. **Local Edit**: User performs insert/delete on OptimizedRga
2. **Create Operation**: Convert edit to Operation
3. **Create LogEntry**: Wrap with parent hash
4. **Append to Log**: `log.append(entry.encode())`
5. **Sign**: `signed_log = log.sign()` creates SignedLog with merkle roots
6. **Sync**: Exchange SignedLog headers with peers
7. **Verify & Apply**: Peers verify signatures, apply operations

## Causality Preservation

### The Problem

Operations have causal dependencies:
- A Delete depends on the Insert it targets
- An Insert with origin_left depends on that origin existing

If operations arrive out of order, the result may be incorrect.

### Solution: Causal Delivery

The signed log provides causal delivery through:

1. **Per-user ordering**: Operations from a single user are totally ordered by sequence number

2. **Parent hash chaining**: Each entry references its predecessor, creating a chain

3. **Version vectors**: Track the highest sequence number seen from each user

```rust
pub struct VersionVector {
    versions: HashMap<KeyPub, u32>,
}

impl VersionVector {
    fn can_apply(&self, op: &Operation) -> bool {
        // Check if all dependencies are satisfied
        match op {
            Operation::Insert { origin_left, origin_right, .. } => {
                self.has_origin(origin_left) && self.has_origin(origin_right)
            }
            Operation::Delete { target_user, target_seq, .. } => {
                self.get(target_user) >= *target_seq
            }
        }
    }
}
```

### Export Ordering

The `export_operations()` method returns operations in causal order:
1. All Insert operations first (in document order)
2. All Delete operations after (targeting existing inserts)

This ensures replay produces correct results without additional sorting.

## Handling Partial Logs

### Version Vectors for Sync

When syncing between replicas:

1. **Compare vectors**: Determine which operations each side is missing
2. **Request missing**: Request operations newer than local vector
3. **Apply in order**: Apply received operations in causal order

```rust
// Alice has: {alice: 10, bob: 5}
// Bob has:   {alice: 7, bob: 8}
//
// Alice needs: bob's ops 6-8
// Bob needs:   alice's ops 8-10
```

### Buffering Out-of-Order Operations

If an operation arrives before its dependencies:

```rust
struct PendingBuffer {
    pending: Vec<Operation>,
}

impl PendingBuffer {
    fn try_apply(&mut self, rga: &mut OptimizedRga, vv: &VersionVector) {
        loop {
            let applicable: Vec<_> = self.pending
                .drain_filter(|op| vv.can_apply(op))
                .collect();
            
            if applicable.is_empty() {
                break;
            }
            
            for op in applicable {
                rga.apply_operation(op);
            }
        }
    }
}
```

## Determinism Guarantees

### Same Operations, Same Result

The Fugue algorithm ensures that concurrent operations produce the same result regardless of receive order:

1. **Dual origins**: Each insert records both left and right neighbors
2. **Conflict resolution**: When two inserts have the same left origin, use (right_origin, user_id, seq) as tiebreaker
3. **Total order**: This provides a deterministic total order for all operations

### Verification

The implementation includes tests for:

- **Round-trip**: `from_operations(export_operations())` produces same state
- **Determinism**: Rebuilding multiple times produces same result
- **Commutativity**: Different merge orders produce same result (for inserts)

## OpLog Trait

```rust
pub trait OpLog: Default {
    /// Export all operations needed to reconstruct this CRDT.
    fn export_operations(&self) -> Vec<Operation>;
    
    /// Rebuild a CRDT from a sequence of operations.
    fn from_operations(ops: impl Iterator<Item = Operation>) -> Self;
    
    /// Apply a single operation to this CRDT.
    /// Returns true if applied, false if already present (idempotent).
    fn apply_operation(&mut self, op: Operation) -> bool;
}
```

## Example Usage

```rust
use together::crdt::log_integration::{OpLog, Operation};
use together::crdt::rga_optimized::OptimizedRga;
use together::crdt::rga_trait::Rga;
use together::key::KeyPair;
use together::log::Log;

// Create user and document
let alice = KeyPair::generate();
let mut doc = OptimizedRga::new();

// Edit document
doc.insert(&alice.key_pub, 0, b"Hello, World!");
doc.delete(5, 2);  // Delete ", "

// Export and store in signed log
let ops = doc.export_operations();
let mut log = Log::new(alice.clone());
for op in &ops {
    log.append(&op.encode());
}
let signed = log.sign();

// Later: rebuild from log
let mut rebuilt = OptimizedRga::new();
for i in 0..log.len() {
    let bytes = log.block(i).unwrap();
    let op = Operation::decode(bytes).unwrap();
    rebuilt.apply_operation(op);
}

assert_eq!(doc.to_string(), rebuilt.to_string());
```

## Future Work

1. **Delta sync**: Export only operations since a given version vector
2. **Compression**: Run-length encode consecutive single-char inserts
3. **Snapshot + log**: Periodically snapshot state, replay only recent ops
4. **Multi-document**: Extend to support multiple CRDTs in one log
