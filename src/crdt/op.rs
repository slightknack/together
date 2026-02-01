// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-01-31"
// driver = "Isaac Clayton"

//! Operations for RGA that can be stored in a signed append-only log.
//!
//! Each writer produces a sequence of operations that, when replayed,
//! reconstruct the CRDT state. The key insight is that operations are
//! *intention-preserving*: they describe what the user intended to do
//! in a way that can be merged with concurrent operations.
//!
//! For RGA, we store:
//! - Insert: "I inserted this content after ItemId X"
//! - Delete: "I deleted the item at ItemId X"
//!
//! The ItemId uniquely identifies a position in a way that survives
//! concurrent edits. Unlike positional indices (which shift), ItemIds
//! are stable because they're based on (user, seq) pairs.

use crate::key::KeyPub;

/// A unique identifier for an item in an RGA.
/// This is the same as rga::ItemId but defined here to avoid circular deps.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ItemId {
    pub user: KeyPub,
    pub seq: u64,
}

/// An operation that can be applied to an RGA.
#[derive(Clone, Debug)]
pub enum Op {
    /// Insert content after the given origin.
    /// If origin is None, insert at the beginning.
    /// The seq is the starting sequence number for this insert.
    /// Content is stored separately in the log block.
    Insert {
        /// The item this content is inserted after (None = beginning).
        origin: Option<ItemId>,
        /// The item that was immediately to the right when this was inserted (None = end).
        /// This is needed for correct merge commutativity (Yjs/YATA/Fugue approach).
        right_origin: Option<ItemId>,
        /// Starting sequence number for this user.
        seq: u64,
        /// Length of the inserted content.
        len: u64,
    },

    /// Delete an item.
    Delete {
        /// The item to delete.
        target: ItemId,
    },
}

/// A batch of operations from a single user, stored as one log block.
/// 
/// When a user types "hello", we don't store 5 separate Insert operations.
/// Instead, we store one Insert with len=5. The content bytes are stored
/// in the log block alongside the serialized Op.
///
/// Block format:
/// ```text
/// [Op (serialized)]
/// [Content bytes (for Insert ops)]
/// ```
///
/// This keeps operations small while allowing efficient storage of content.
#[derive(Clone, Debug)]
pub struct OpBlock {
    /// The operation.
    pub op: Op,
    /// Content bytes for Insert operations.
    pub content: Vec<u8>,
}

impl OpBlock {
    /// Create an insert operation block.
    pub fn insert(origin: Option<ItemId>, right_origin: Option<ItemId>, seq: u64, content: Vec<u8>) -> OpBlock {
        let len = content.len() as u64;
        return OpBlock {
            op: Op::Insert { origin, right_origin, seq, len },
            content,
        };
    }

    /// Create a delete operation block.
    pub fn delete(target: ItemId) -> OpBlock {
        return OpBlock {
            op: Op::Delete { target },
            content: Vec::new(),
        };
    }
}

/// Replaying operations from multiple writers to build an RGA.
///
/// The merge algorithm:
/// 1. Each writer has a signed append-only log of OpBlocks.
/// 2. To merge, we collect all operations from all writers.
/// 3. Operations are applied in causal order:
///    - An Insert can only be applied after its origin exists.
///    - A Delete can only be applied after its target exists.
/// 4. Concurrent inserts at the same position are ordered by (user, seq).
///
/// The signed log ensures:
/// - Operations are authentic (signed by the writer).
/// - Operations are ordered within each writer's log.
/// - Forks are detectable (if a writer tries to rewrite history).
///
/// Integration with `log.rs`:
/// - Each writer calls `Log::append(serialize(op_block))`.
/// - The log signs and stores the operation.
/// - To sync, exchange `SignedLog` headers and missing blocks.
/// - Blocks are verified using `SignedLog::verify_proof`.
pub struct OpLog {
    /// Operations from all writers, in the order we received them.
    /// For proper merging, we track causal dependencies.
    ops: Vec<(KeyPub, OpBlock)>,
}

impl OpLog {
    /// Create a new empty operation log.
    pub fn new() -> OpLog {
        return OpLog { ops: Vec::new() };
    }

    /// Add an operation from a writer.
    pub fn push(&mut self, user: KeyPub, block: OpBlock) {
        self.ops.push((user, block));
    }

    /// Get all operations.
    pub fn ops(&self) -> &[(KeyPub, OpBlock)] {
        return &self.ops;
    }
}

impl Default for OpLog {
    fn default() -> Self {
        return Self::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::KeyPair;

    #[test]
    fn insert_block() {
        let block = OpBlock::insert(None, None, 0, b"hello".to_vec());

        match &block.op {
            Op::Insert { origin, right_origin, seq, len } => {
                assert!(origin.is_none());
                assert!(right_origin.is_none());
                assert_eq!(*seq, 0);
                assert_eq!(*len, 5);
            }
            _ => panic!("expected Insert"),
        }
        assert_eq!(block.content, b"hello");
    }

    #[test]
    fn delete_block() {
        let user = KeyPair::generate().key_pub;
        let target = ItemId {
            user: user.clone(),
            seq: 42,
        };
        let block = OpBlock::delete(target.clone());

        match &block.op {
            Op::Delete { target: t } => {
                assert_eq!(t, &target);
            }
            _ => panic!("expected Delete"),
        }
        assert!(block.content.is_empty());
    }

    #[test]
    fn op_log_collects_operations() {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();

        let mut log = OpLog::new();
        log.push(alice.key_pub.clone(), OpBlock::insert(None, None, 0, b"hello".to_vec()));
        log.push(bob.key_pub.clone(), OpBlock::insert(None, None, 0, b"world".to_vec()));

        assert_eq!(log.ops().len(), 2);
    }
}
