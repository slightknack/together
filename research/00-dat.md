---
model = "claude-opus-4-5"
created = "2026-01-30"
modified = "2026-01-30"
driver = "Isaac Clayton"
---

# Dat and Hypercore

Hypercore is the append-only log at the heart of the Dat protocol. It provides a single-writer log where each entry can be verified using only the public key identifier.

Sources:
- https://www.datprotocol.com/deps/0002-hypercore/
- https://github.com/dat-ecosystem-archive/book/blob/master/src/ch01-02-merkle-tree.md

## Flat In-Order Tree

Hypercore represents its merkle tree as a flat list using in-order indexing. Even indices are leaf nodes (data blocks), odd indices are parent nodes.

```
0──┐
   1──┐
2──┘  │
      3
4──┐  │
   5──┘
6──┘
```

Node 1 is the hash of nodes 0 and 2. Node 3 is the hash of nodes 1 and 5. Tree depth is calculated by counting trailing binary 1s in the index.

## Hash Computation

Hypercore uses blake2b-256 for hashing and ed25519 for signatures. Three type constants prevent preimage attacks:

- `0x00` for leaf hashes
- `0x01` for parent hashes
- `0x02` for root hashes

Leaf hash: `hash(0x00 || length || data)`

Parent hash: `hash(0x01 || left_size + right_size || left_hash || right_hash)`

Root hash: `hash(0x02 || root_hash || index || size || ...)`

When the number of leaves is not a power of two, multiple roots exist. These are concatenated and hashed together before signing.

## Signing and Verification

The root hash is signed with the feed creator's private key. The public key serves as the feed's identifier.

To verify a block:
1. Receive the block, its ancestor hashes, and a signed root hash
2. Verify the root signature against the public key
3. Hash the block with the ancestors to reproduce the root
4. Compare computed root with signed root

Verification requires at most log2(n) ancestor hashes. This enables efficient partial replication: you can verify any block without downloading the entire log.

## Linear History

Hypercore enforces strict append-only semantics. If two signatures exist for the same index with different content, the feed is marked corrupt. This prevents forking but means the private key cannot be safely shared across devices.

## Storage

Each tree node is 40 bytes on disk: 32 bytes for the hash, 8 bytes for the size of the subtree (uint64 big endian). Nodes are stored sequentially at offset `32 + (index * 40)` after a 32-byte header.

Six files per hypercore:
- `data`: the actual content
- `tree`: merkle tree hashes
- `signatures`: signed root hashes
- `bitfield`: which blocks exist locally
- `public key`: for verification
- `secret key`: for signing (creator only)

## Comparison with Together

Together's design uses a 16-tree instead of a binary tree. The tradeoff:

- Binary tree (hypercore): deeper, but each verification step needs only one sibling hash
- 16-tree (together): shallower, bounds the number of roots to at most 16, avoids the concatenate-and-hash step for multiple roots

Both use single-writer logs with ed25519 signatures. Hypercore uses blake2b-256; together uses blake3.

The key insight from hypercore is that verification requires only log2(n) hashes. This makes partial replication efficient: peers can request specific blocks and verify them without downloading everything.

## Bitfield Run-Length Encoding

Hypercore uses run-length encoding to communicate which blocks a peer has or needs. This enables compact "have" and "want" messages for sparse replication.

Sources:
- https://github.com/mafintosh/bitfield-rle
- https://github.com/datrs/bitfield-rle

The encoding format uses varints to prefix each sequence:

Compressed sequence (for runs of identical bits):
```
varint(byte_length << 2 | bit << 1 | 1)
```

Uncompressed sequence (when compression would not help):
```
varint(byte_length << 1 | 0) + raw_bitfield_bytes
```

The last bit of the varint header distinguishes the two: odd means compressed, even means uncompressed. The encoder only uses compression when it reduces size, so the output is never larger than the input (except for a 1-6 byte header).

This is effective for sparse bitfields with long runs of the same bit. A peer that has blocks 0-1000 and 5000-6000 can encode this compactly as two runs of 1s separated by a run of 0s, rather than sending 6000 individual bits.

The wire protocol uses this in "Have" messages to announce block availability. A peer receiving a Have message can compute what it needs by XORing its local bitfield with the remote bitfield.
