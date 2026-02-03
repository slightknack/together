# Together API Reference

A collaborative text editing library using CRDTs (Conflict-free Replicated Data Types).

## Quick Start

```rust
use together::crdt::rga::RgaBuf;
use together::key::KeyPair;

// Create a user identity
let user = KeyPair::generate();

// Create a new document
let mut doc = RgaBuf::new();

// Edit the document
doc.insert(&user.key_pub, 0, b"Hello, World!");
assert_eq!(doc.to_string(), "Hello, World!");

// Delete some text
doc.delete(5, 8); // Delete ", World"
assert_eq!(doc.to_string(), "Hello!");
```

## Cryptographic Keys (`together::key`)

### `KeyPair::generate()`

Generate a new random keypair for signing and encryption.

```rust
let user = KeyPair::generate();
// user.key_pub - the public key (32 bytes, ed25519)
// user.key_sec - the secret key (32 bytes)
```

### `keypair.sign(message)`

Sign a message with the secret key.

```rust
let signature = keypair.sign(b"hello world");
```

### `key_pub.verify(message, signature)`

Verify a signature against a public key.

```rust
let valid = key_pub.verify(b"hello world", &signature);
assert!(valid);
```

### `keypair.conspire(other_pub)`

Derive a shared secret via Diffie-Hellman key exchange.

```rust
let alice = KeyPair::generate();
let bob = KeyPair::generate();

let shared_a = alice.conspire(&bob.key_pub);
let shared_b = bob.conspire(&alice.key_pub);
assert_eq!(shared_a, shared_b); // Same shared secret
```

### `shared.encrypt(message)`

Encrypt a message using XChaCha20-Poly1305.

```rust
let shared = alice.conspire(&bob.key_pub);
let payload = shared.encrypt(b"secret message");
// payload.nonce - 24-byte nonce
// payload.ciphertext - encrypted bytes with auth tag
```

### `shared.decrypt(payload)`

Decrypt a message. Returns `Err(DecryptError::AuthenticationFailed)` if tampered.

```rust
let plaintext = shared.decrypt(&payload)?;
```

### `hash(message)`

Hash a message using BLAKE3.

```rust
let h = together::key::hash(b"hello world");
// h.0 - 32-byte hash
```

## RGA Document (`together::crdt::rga`)

RGA (Replicated Growable Array) is the core data structure for collaborative text editing.

### `Rga::new()`

Create a new empty RGA document.

```rust
let mut doc = Rga::new();
assert!(doc.is_empty());
```

### `rga.insert(user, pos, content)`

Insert content at the given visible position.

```rust
let user = KeyPair::generate();
let mut doc = Rga::new();

doc.insert(&user.key_pub, 0, b"hello");
doc.insert(&user.key_pub, 5, b" world");
assert_eq!(doc.to_string(), "hello world");

// Insert in the middle
doc.insert(&user.key_pub, 5, b",");
assert_eq!(doc.to_string(), "hello, world");
```

### `rga.delete(start, len)`

Delete a range of visible characters using tombstone deletion.

```rust
doc.insert(&user.key_pub, 0, b"hello world");
doc.delete(5, 6); // Delete " world"
assert_eq!(doc.to_string(), "hello");
```

Deleted characters are marked as tombstones (not removed), preserving order for merges.

### `rga.len()`

Get the visible length (excluding deleted items).

```rust
let len = doc.len();
```

### `rga.is_empty()`

Check if the document has no visible content.

```rust
if doc.is_empty() {
    println!("Document is empty");
}
```

### `rga.to_string()`

Get the full document content as a UTF-8 string.

```rust
let content = doc.to_string();
```

### `rga.slice(start, end)`

Read characters in range `[start, end)` without allocating the full document.

```rust
doc.insert(&user.key_pub, 0, b"hello world");
assert_eq!(doc.slice(0, 5), Some("hello".to_string()));
assert_eq!(doc.slice(6, 11), Some("world".to_string()));
assert_eq!(doc.slice(0, 100), None); // Out of bounds
```

### `rga.span_count()`

Get the number of internal spans (useful for profiling).

```rust
let spans = doc.span_count();
```

## Anchors

Anchors track positions that move with edits. Useful for cursors, selections, and annotations.

### `rga.anchor_at(pos, bias)`

Create an anchor at the given visible position.

```rust
use together::crdt::rga::AnchorBias;

let anchor = doc.anchor_at(5, AnchorBias::Before)?;
// AnchorBias::Before - anchor stays before the character
// AnchorBias::After - anchor stays after the character
```

### `rga.resolve_anchor(anchor)`

Resolve an anchor to its current visible position. Returns `None` if the anchored character was deleted.

```rust
let pos = doc.resolve_anchor(&anchor)?;
```

### `rga.anchor_range(start, end)`

Create an anchor range for `[start, end)`. The range expands when text is inserted at its boundaries.

```rust
let range = doc.anchor_range(0, 5)?;
```

### `rga.slice_anchored(range)`

Get the current content of an anchor range.

```rust
let content = doc.slice_anchored(&range)?;
```

## Versioning

Save and restore document snapshots for undo/history.

### `rga.version()`

Get a snapshot of the current document state. Uses Arc for cheap cloning.

```rust
let v1 = doc.version();
doc.insert(&user.key_pub, 0, b"more text");
let v2 = doc.version();
```

### `rga.to_string_at(version)`

Get the full document at a specific version.

```rust
let old_content = doc.to_string_at(&v1);
```

### `rga.slice_at(start, end, version)`

Read a slice from a specific version.

```rust
let slice = doc.slice_at(0, 5, &v1)?;
```

### `rga.len_at(version)`

Get the document length at a specific version (O(1)).

```rust
let old_len = doc.len_at(&v1);
```

## Buffered RGA (`RgaBuf`)

A wrapper around `Rga` that buffers adjacent operations for better performance during sequential typing.

### `RgaBuf::new()`

Create a new buffered RGA.

```rust
let mut doc = RgaBuf::new();
```

### `buf.insert(user, pos, content)`

Insert content. Adjacent inserts by the same user are buffered.

```rust
// These are buffered into a single RGA operation
buf.insert(&user.key_pub, 0, b"h");
buf.insert(&user.key_pub, 1, b"e");
buf.insert(&user.key_pub, 2, b"l");
buf.insert(&user.key_pub, 3, b"l");
buf.insert(&user.key_pub, 4, b"o");
```

### `buf.delete(start, len)`

Delete a range. Backspace at end of pending insert trims the buffer directly.

```rust
buf.insert(&user.key_pub, 0, b"helllo"); // Typo
buf.delete(3, 1); // Backspace - trims buffer, no RGA operation
```

### `buf.flush()`

Force pending operations to be applied to the underlying RGA.

```rust
buf.flush();
```

### `buf.len()`, `buf.to_string()`, etc.

Read operations automatically flush pending operations first.

```rust
let content = buf.to_string(); // Flushes, then returns content
```

### `buf.inner()` / `buf.inner_mut()`

Access the underlying `Rga`. Warning: does not flush pending operations.

```rust
let rga = buf.inner();
```

## Merging & Operations (`together::crdt::op`)

For synchronizing documents across replicas.

### `rga.apply(user, block)`

Apply an operation from a remote user. Returns `true` if applied, `false` if already present (idempotent).

```rust
use together::crdt::op::OpBlock;

let block = OpBlock::insert(None, 0, b"hello".to_vec());
let applied = doc.apply(&user.key_pub, &block);
```

### `rga.merge(other)`

Merge another RGA into this one. Implements the `Crdt` trait.

```rust
use together::crdt::Crdt;

let mut doc_a = Rga::new();
let mut doc_b = Rga::new();

doc_a.insert(&alice.key_pub, 0, b"hello");
doc_b.insert(&bob.key_pub, 0, b"world");

doc_a.merge(&doc_b);
// doc_a now contains both "hello" and "world"
```

### `OpBlock::insert(origin, seq, content)`

Create an insert operation block.

```rust
use together::crdt::op::{OpBlock, ItemId};

// Insert at beginning (no origin)
let block = OpBlock::insert(None, 0, b"hello".to_vec());

// Insert after a specific character
let origin = ItemId { user: alice.key_pub, seq: 4 };
let block = OpBlock::insert(Some(origin), 5, b" world".to_vec());
```

### `OpBlock::delete(target)`

Create a delete operation block.

```rust
let target = ItemId { user: alice.key_pub, seq: 2 };
let block = OpBlock::delete(target);
```

## Signed Logs (`together::log`)

Append-only logs with merkle tree verification.

### `Log::new(keypair)`

Create a new empty log owned by the keypair.

```rust
use together::log::Log;

let keypair = KeyPair::generate();
let mut log = Log::new(keypair);
```

### `log.append(data)`

Append a block to the log.

```rust
log.append(b"block 0");
log.append(b"block 1");
```

### `log.len()` / `log.is_empty()`

Get the number of blocks in the log.

```rust
let n = log.len();
```

### `log.block(index)`

Get a block by index.

```rust
let data = log.block(0)?; // Returns &[u8]
```

### `log.sign()`

Sign the current log state, producing a `SignedLog`.

```rust
let signed = log.sign();
// signed.author - the signer's public key
// signed.length - number of blocks
// signed.roots - merkle tree roots
// signed.signature - ed25519 signature
```

### `log.proof(index)`

Generate a merkle proof that a block belongs to the log.

```rust
let proof = log.proof(7)?;
```

### `signed_log.verify()`

Verify the signature on a signed log.

```rust
if signed.verify() {
    println!("Log is authentic");
}
```

### `signed_log.verify_proof(index, data, proof)`

Verify that a block belongs to a signed log.

```rust
let valid = signed.verify_proof(7, b"block 7", &proof);
```

## Common Patterns

### Collaborative Editing Session

```rust
use together::crdt::rga::RgaBuf;
use together::crdt::Crdt;
use together::key::KeyPair;

// Each user has their own identity
let alice = KeyPair::generate();
let bob = KeyPair::generate();

// Each user has their own document replica
let mut alice_doc = RgaBuf::new();
let mut bob_doc = RgaBuf::new();

// Alice types "hello"
alice_doc.insert(&alice.key_pub, 0, b"hello");

// Bob types "world" (concurrently, before syncing)
bob_doc.insert(&bob.key_pub, 0, b"world");

// Sync: merge bob's changes into alice's doc
alice_doc.flush();
bob_doc.flush();
alice_doc.inner_mut().merge(bob_doc.inner());

// Both documents converge to the same content
// (order depends on user key comparison)
```

### Tracking Cursor Position

```rust
use together::crdt::rga::{Rga, AnchorBias};

let user = KeyPair::generate();
let mut doc = Rga::new();

doc.insert(&user.key_pub, 0, b"hello world");

// Create anchor at position 5 (between "hello" and " world")
let cursor = doc.anchor_at(5, AnchorBias::After)?;

// Insert text before the cursor
doc.insert(&user.key_pub, 0, b"say: ");

// Cursor position automatically updates
let new_pos = doc.resolve_anchor(&cursor)?;
assert_eq!(new_pos, 10); // Shifted by 5 characters
```

### Undo with Versioning

```rust
let user = KeyPair::generate();
let mut doc = Rga::new();

// Make some edits, saving versions
doc.insert(&user.key_pub, 0, b"hello");
let v1 = doc.version();

doc.insert(&user.key_pub, 5, b" world");
let v2 = doc.version();

// Access historical content
assert_eq!(doc.to_string_at(&v1), "hello");
assert_eq!(doc.to_string_at(&v2), "hello world");
```

### Append-Only Log with Verification

```rust
use together::log::Log;

let author = KeyPair::generate();
let mut log = Log::new(author);

// Append operations
for i in 0..100 {
    log.append(format!("op {}", i).as_bytes());
}

// Sign and share
let signed = log.sign();

// Receiver verifies authenticity
assert!(signed.verify());

// Verify a specific block with proof
let proof = log.proof(42).unwrap();
assert!(signed.verify_proof(42, b"op 42", &proof));
```

## Performance Notes

- **Span coalescing**: Sequential inserts by the same user are coalesced into a single span, keeping memory usage low during continuous typing.

- **Cursor caching**: The RGA caches the last lookup position. Sequential typing at positions P, P+1, P+2... achieves O(1) amortized lookups instead of O(log n).

- **Buffered operations**: `RgaBuf` batches adjacent operations, reducing per-keystroke overhead for common editing patterns like typing and backspace.

- **B-tree storage**: Spans are stored in a B-tree weighted by visible length, enabling O(log n) position lookups even in large documents.

- **Origin index**: During merges, finding concurrent insertions at the same position is O(k) where k is the number of concurrent edits (typically small), rather than O(n).
