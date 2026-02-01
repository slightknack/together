+++
model = "claude-opus-4-5"
created = 2026-01-30
modified = 2026-01-30
driver = "Isaac Clayton"
+++

# Initial Implementation

### key.rs

Implemented cryptographic primitives:

- `KeyPub`, `KeySec`, `KeyPair`, `KeyShared`, `Signature` types wrapping raw bytes
- `KeyPair::generate()` using ed25519
- `KeyPair::sign()` and `KeyPub::verify()` for signatures
- `KeyPair::conspire()` for Diffie-Hellman key exchange (ed25519 to X25519 conversion)
- `Hash` type with `hash()` function using blake3
- `Payload` type with `KeyShared::encrypt()` and `decrypt()` using XChaCha20-Poly1305

9 tests covering sign/verify, key exchange, hashing, and encryption.

### log.rs

Implemented signed append-only logs with a 16-tree merkle structure:

- `Log` struct holding a keypair and vector of blocks
- `SignedLog` struct containing author public key, length, roots, and signature
- `Proof` and `ProofLevel` for membership proofs

Key design decisions:

1. **16-tree structure**: Branching factor of 16 bounds the number of roots to at most 16 for any log up to 2^64 blocks. This avoids the need to concatenate-and-hash multiple roots.

2. **Domain separation**: Three type constants prevent cross-protocol attacks:
   - `TYPE_LEAF = 0x00` for block hashes
   - `TYPE_PARENT = 0x01` for internal node hashes  
   - `TYPE_ROOT = 0x02` for the signable message

3. **Hash format**:
   - Leaf: `hash(0x00 || length || data)`
   - Parent: `hash(0x01 || child_count || child_hashes...)`
   - Signable: `hash(0x02 || log_length || root_count || roots...)`

4. **Proof structure**: Each level contains the sibling hashes and the position of the proven hash. Verification checks that positions match the claimed index.

17 tests covering:
- Basic operations (append, retrieve, length)
- Signing and verification
- Tampering detection (wrong key, modified length, modified roots)
- Domain separation
- Tree structure (root counts for various block counts)
- Membership proofs (valid proofs, wrong data, wrong index)

### neopack (external)

Added bitfield support to the neopack serialization library:

- `Tag::Bitfield = 0x12`
- RLE compression for runs of identical bits
- LEB128 encoding for run lengths
- Format follows existing TLV pattern: `[Tag][Len][Body]` where Body contains bit count + RLE data

### Research

Created `research/00-dat.md` documenting the Dat/Hypercore protocol:
- Flat in-order tree representation
- Hash computation with type constants
- Bitfield RLE encoding for sparse replication
- Comparison with together's 16-tree approach

### crdt/mod.rs

Defined the core `Crdt` trait:

```rust
pub trait Crdt {
    fn merge(&mut self, other: &Self);
}
```

The merge operator must be commutative, associative, and idempotent.

### crdt/rga.rs

Implemented a Replicated Growable Array (RGA) for collaborative text editing.

**Research conducted:**
- [Diamond Types](https://github.com/josephg/diamond-types) - The world's fastest CRDT
- [CRDTs go brrr](https://josephg.com/blog/crdts-go-brrr/) - 5000x performance gains through spans, B-trees, and cache-friendly layouts
- [Zed's Rope & SumTree](https://zed.dev/blog/zed-decoded-rope-sumtree) - B+ trees with hierarchical summaries
- [Backing stores for CRDTs](https://slightknack.dev/blog/backing-crdt-store/) - Append-only columns per user

**Design decisions (Pareto-efficient for simplicity + performance):**

1. **Spans**: Consecutive insertions by the same user are stored as a single span. Typing "hello" creates one span, not five items. This reduces memory 14x in practice.

2. **Append-only columns**: Each user has a `Column` storing their content. Columns only append, making replication trivial - just send new bytes.

3. **RGA ordering**: When concurrent inserts happen at the same position, order by `(user, seq)` descending. This is deterministic and commutative.

4. **Flat list (for now)**: Currently O(n) for position lookups. The `BRANCHING` constant and `index` field are placeholders for a future B-tree implementation.

**Data structures:**

- `ItemId`: `(user: KeyPub, seq: u64)` - unique identifier for each character
- `Span`: Run of consecutive items from one user
- `Node`: Container for spans with summary metadata (visible_len, total_len)
- `Column`: Per-user content storage with next_seq counter
- `Rga`: The main structure holding root node and columns

**Operations:**

- `insert(user, pos, content)` - Insert at visible position
- `delete(start, len)` - Delete range by visible position
- `apply(user, OpBlock)` - Apply operation from log (idempotent)
- `merge(other)` - Merge another RGA (CRDT merge)

### crdt/op.rs

Defined operations for log integration:

```rust
pub enum Op {
    Insert { origin: Option<ItemId>, seq: u64, len: u64 },
    Delete { target: ItemId },
}

pub struct OpBlock {
    pub op: Op,
    pub content: Vec<u8>,
}
```

**Integration with log.rs:**

Each writer maintains a signed append-only log of `OpBlock`s. The flow:

1. User types "hello" at position 5
2. Find the `ItemId` at position 4 (the origin)
3. Create `OpBlock::insert(Some(origin), next_seq, b"hello")`
4. Append serialized OpBlock to the log
5. Sign the log

To sync:
1. Exchange `SignedLog` headers
2. Request missing blocks using bitfield diff
3. Verify blocks with `SignedLog::verify_proof`
4. Apply blocks with `Rga::apply`

The signed log guarantees:
- Operations are authentic (signed by writer)
- Operations are ordered within each writer's log
- Forks are detectable (if a writer rewrites history)

### Process

Updated PROCESS.md with several conventions discovered during the session:
- Import ordering (one item per line, std/external/internal groups)
- Frontmatter conventions (created/modified for machine-managed, date for human-managed)
- Scripts over instructions principle
- Following existing patterns when extending systems
- Using python scripts for calculations

### Benchmarks

Benchmarked against diamond-types using [josephg/editing-traces](https://github.com/josephg/editing-traces):

| Trace | Patches | Together | Diamond | Ratio |
|-------|---------|----------|---------|-------|
| sveltecomponent | 19,749 | 3.32s | 1.27ms | 2606x |

**Why so slow?**

Current implementation uses O(n) operations:
- `find_visible_pos`: Linear scan through spans
- `insert_span_raw`: Reindexes all spans after insertion point
- `find_span_by_id`: Linear search when not exact match

Diamond-types uses:
- JumpRope (skip list): O(log n) position lookup
- Efficient in-place mutations
- Cache-friendly node layout (~400 byte inline strings)

**Path to performance:**

1. Replace flat `Vec<Span>` with skip list or B-tree
2. Store cumulative character counts at each level for O(log n) position lookup
3. Use a proper index (BTreeMap keyed by (user, seq)) for O(log n) span lookup

See `research/01-diamond-types.md` for detailed architecture notes.

**Consistency verified:** All traces produce identical output to diamond-types.

### Test Summary

48 tests total:
- key.rs: 9 tests
- log.rs: 17 tests  
- crdt/rga.rs: 19 tests
- crdt/op.rs: 3 tests
