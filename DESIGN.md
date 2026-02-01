+++
author = "Isaac Clayton"
date = 2026-01-29
managed = "human"
+++

# together

Together is an embeddable and authenticated conflict-free replicated datatype (CRDT) library. Think like a git repository without merge conflicts and rich data types. It is written in Rust and can be run natively or on the web, through WebAssembly.

## background on CRDTs

A CRDT is a datatype with a single operator, merge. Merge is an operation that takes a pair of values and produces a new value, merging the two. This operator has some special properties. Specifically, only requirement on merge is that, for all types, it is:

- Commutative, meaning `merge(A, B)` is the same as `merge(B, A)`; the order of operations does not matter

- Associative, meaning `merge(A, merge(B, C))` is the same as `merge(merge(A, B), C)`; the grouping of operations does not matter

- Idempotent, meaning `merge(A, merge(A, B))` is the same as `merge(A, B)`; applying the same operation multiple times does not change the result.

A simple example of a CRDT is a max counter over the integers. The merge operator simply takes whichever argument is greater. We can define it in Rust like so:

```rust
struct MaxCounter {
  value: usize,
}

fn merge(a: MaxCounter, b: MaxCounter) -> MaxCounter {
  MaxCounter { value: a.value.max(b.value) }
}
```

Imagine we have three integer values such that `A > B > C`. Then, it is trivial to show that max counter is:

- Commutative, as `max(A, B) = A = max(B, A)`
- Associative, as `max(A, max(B, C)) = A = max(max(A, B), X)`
- Idempotent, as `max(A, max(A, B)) = A = max(A, B)`

Therefore max over the integers forms a CRDT. 

There are lots of other cool CRDTs, like replicated growth arrays, for more complicated structures of data. The core idea of CRDTS is that if the merge operator forms a lattice, we can take any set of changes and merge them in any order and eventually arrive at the same result. This has very nice properties for collaboration, of course.

## background on public key cryptography

Alice, nice to meet you. My name is Bob. I trust you a lot, but I don't trust Charlie. I'm worried he's listening to us as we speak. But don't fear, we can use good old public-key cryptography to communicate safely. Here are the primitives.

First, we have to agree to use the same cryptosystem. Any cryptosystem provides essentially the same set of primitives, which we can assemble to form higher-level communication channels. Our cryptosystem will use the ed25519 curve, Diffie-Hellman key exchange, XChaCha20-Poly1305 for authenticated encryption with associated data (AEAD). 

First, the ed25519 curve. We can use this curve to generate secret keys:

- `generate(Rng) -> KeyPair` generates a random public key and secret key, which are bundled together into a key pair. Public keys can be communicated far and wide, but secret keys must be kept strictly secret.

- `public(KeyPair) -> KeyPublic` returns the public half of the key pair. This public half can be sent far and wide.

- `sign(KeyPair, Message) -> Signature` attest that the owner of a key produced a given message.

- `verify(KeyPublic, Message, Signature) -> bool` verify whether an untrusted public message was indeed attested by the signing key.

Second, Diffie-Hellman key exchange. This provides a function we can use to come to agreement on a shared secret random value.

- `conspire(KeyPair, KeyPublic) -> KeyShared` takes my private key and your public key to compute a shared secret. What's interesting here is that, if you use your private key and my public key, this function arrives at the same shared secret for both of us. This way, we can both generate secrets, exchange only public keys, and arrive at the same shared secret.

Next, the XChaCha20-Poly1305 AEAD. If we have both arrived at the same shared secret, we can use this AEAD algorithm to encrypt cleartext messages and decrypt ciphertext payloads. The primitives are:

- `encrypt(KeyShared, Message, Nonce) -> Payload`, which encrypts a message that can be decrypted by anyone with the shared secret.

- `decrypt(KeyShared, Payload) -> Message`, which decrypts a message anyone has encoded with our shared secret.

This in essence, forms a chain. The process looks something like:

> `generate` >> `public` >> (exchange public keys) >> `conspire` >> `encrypt` >> (exchange payloads) >> `decrypt`.

Ah, the wonders of modern mathematics.

## background on signed append-only logs

Imagine that I am a firehose that produces a stream of data, or messages, of a given maximum size. I want to show that:

- Yes, these messages come from me, I am the fountain of all truth with respect to the tokens that may drip from my lips

- No, I have not tampered with or reordered any messages, and if I have tried to do that, you may call me out and banish me.

There's a relatively simple way to do this. Let's start by introducing a new primitive. This is a hash function. We will use `blake3`.

- `blake3(Message) -> Hash`. A hash function, like `blake3` takes a message, and produces a hash, which you can think of as a fixed-size tag. If you have a message and its hash, you can verify the integrity of the message by hashing it again and comparing that new hash with the ground-truth hash you have on hand. If they don't match, that implies that either the message or the hash has been corrupted.

We can use this primitive, along with some public key cryptography, to build a signed append-only log. Here's how it works. 

- Let's say we have a sequence of blocks, `B0, B1, B2, ...`

- Each block has an associated `blake3` hash, `H0, H1, H2, ...`

We can build a tree of hashes, like so:

```
           H01234567
          /         \
     H0123           H4567
    /     \         /     \
  H01     H23     H45     H67
 /  \    /  \    /  \    /  \
H0  H1  H2  H3  H4  H5  H6  H7
 |   |   |   |   |   |   |   |
BO  B1  B2  B3  B4  B5  B6  B7
```

On the bottom we have the blocks. One step up from the blocks, we have the hashes of each block. Each layer above that zips the layers below in pairs.

Let's say I want to attest that I authored `B0` - `B7`. One approach we could take would be to sign the concatenated contents of `B0` - `B7`. This would work, but that could be a lot of data to sign.

A more efficient approach might be to sign the concatenated contents of `H0` - `H7`. If any underlying block changes, the associated hash will also change, so a signature over hashes is a valid way to check if any block has been tampered with.

Following this logic to its conclusion, the most efficient way to verify any tree of hashes is by signing the tree's root hash. 

What if we can't construct a perfect tree? e.g. consider this pessimistic case:

```
     H0123
    /     \
  H01     H23     H45
 /  \    /  \    /  \
H0  H1  H2  H3  H4  H5  H6
 |   |   |   |   |   |   |
BO  B1  B2  B3  B4  B5  B6
```

Here we have three roots: `H0123`, `H45`, and `H6`.

There are a few simple solutions: 

- We could concatenate all the roots together and sign them. The root message would scale with log2 the number of blocks

- We could build another tree on top of the roots. Something like the following:

```
------------------+---------- 1 root
                  |
               H0123456
              /         \
-------------+-----------+--- 2 roots
             |           |
          H012345        |
        /         \      |
-------+-----------+-----+--- 3 roots
       |           |     |
     H0123         |     |
    /     \        |     |
  H01     H23     H45    |
 /  \    /  \    /  \    |
H0  H1  H2  H3  H4  H5  H6
 |   |   |   |   |   |   |
BO  B1  B2  B3  B4  B5  B6
```

This is cool, but a little complicated.

The best approach, in terms of implementation speed and complexity is to allow for a dynamic—but bounded—number of hashes to be rolled up at each level. 

So instead of a strict binary tree, something more like a k-tree. To capture k, we pick k to be the log_k of whatever the maximum length of the tree should be. In other words, we want the branching factor to be greater than the maximum tree depth. That way, we will never have more roots than the branching factor of a single node.

For example, if our maximum length were 100 blocks, with a tree of k-arity 4, the maximum depth / number of roots we expect is 4, meaning we will never have leftover roots; we can always collect all roots into a single node above. I don't know of a closed-form solution to find k, but here's a table for some common lengths to build intuition:

| k | k^k = max n for k-tree | <= 2^x |
|---|------------------------|--------|
| 2 | 4 | 2^2 |
| 3 | 27 | 2^5 |
| 4 | 256 | 2^8 |
| 5 | 3,125 | 2^12 |
| 6 | 46,656 | 2^16 |
| 7 | 823,543 | 2^20 |
| 8 | 16,777,216 | 2^24 |
| 9 | 387,420,489 | 2^29 |
| 10 | 10,000,000,000 | 2^34 |
| 11 | 285,311,670,611 | 2^39 |
| 12 | 8,916,100,448,256 | 2^44 |
| 13 | 302,875,106,592,253 | 2^49 |
| 14 | 11,112,006,825,558,016 | 2^54 |
| 15 | 437,893,890,380,859,375 | 2^59 |
| 16 | 18,446,744,073,709,551,616 | 2^64 |

If we're operating on a standard 64-bit architecture, and we expect the maximum length to be 2^64, we can use a branching factor of 16 to fully capture all roots of any tree of hashes. This happens to be a very nice power-of-two coincidence: `log16(2^64) = 64/log2(16) = 64/4 = 16`.

This gives us a very simple algorithm for building a signed append-only log, given we have a key pair:

1. Hash each block `B0` - `BN`, producing hashes `H1` - `HN`.

2. Construct a 16-tree, resulting in `D` root hashes `D` <= 16 labeled `R1` - `RD`.

3. Collect `R1` - `RD` into a root node `R`. Sign R and the length of the log with the key pair.

4. Upon receiving a new `R`, accept it if:
  - The signature is valid.
  - The length of the log attested by `R` is greater than the previous length.
  - All blocks present in the last log are accounted for in the new log.

A couple notes:

- If the author attempts to commit foul play by reordering the log, removing items from the log, or publishing multiple logs with different histories, we become aware of this fact, and at an applicaiton level, can choose how to proceed.

- Checking that all blocks in the last log are accounted for is easy: we can just expand the tree from the new root R until we hit a hash we have in the previous tree. If they match, all is good; if they are different, we know that at least one block in the subtree has been tampered with.

- Figuring out which blocks to transmit is easy: we can recurse down the tree and build a set of blocks that are missing, and very efficiently communicate we only need the new blocks or some subset of blocks. There are lots of approaches here, see e.g. the old DAT protocol.

Signed append-only logs are a very nice primitive for saying: we have an writer who verifiably produces an ordered sequence of events. For example, if these events are edits to a document, we can totally order an individual's edits within that document. If we include information about the last seen edit (e.g. logical timestamps), we can partially order edits between multiple writers. If we assign an ordering to writers, we can even totally order all edits to a document.

A signed append-only log is a very strong primitive for turning a merge operator that does not respect the CRDT laws into one that does, in an untamperable way at that. We'll get into exact constructions later.

## together and authenticated CRDTs

With all that background out of the way, we'll get into together and CRDTs. Together provides:

- Cryptographic primitives for building authenticated systems.
- Data primitives for authenticated data replication.
- Rich CRDT primitives for building arbitrary authenticated collaborative objects.

These primitives are fully usable on both the web and natively, and are designed in a future-compatible manner (so we don't fix any one protocol or cryptosystem).

## Examples

Create a new identity:

```
let alice = KeyPair::generate();
```

Create a new text document:

---

# Scratchpad

Let's build out the log-based sync layer next. I want it to work for more than just RGAs. 

Can you do research into hypercore, and merkle trees? read e.g. DESIGN. We have a 16-tree implemented in log. Does it implement signing like we need?

A document is a set of logs. each key is a:

- Reader
- Writer
- Admin

Admin controls the set of readers/writers, there is always exactly one Admin. Messages published to a log can be operations to apply to a crdt or meta. Any reader etc can fork the document; admin can transfer ownership by sending a terminal message that identifies a specific fork as the legitimate new Admin.

Each log message contains a vector clock of the other branches. I'll say that if another log hasn't changed then there's no need to include it in the vector clock, it's implicitly the same.

There should be two functions: one that performs an topological iteration over all events across all branches, and another that returns the last consistent branch (e.g. if you were to take the transitive closure over the logical clock of the current branch, what is the most recent clock in each branch reached, and which is the oldest? all events before whichever clock that is are guaranteed to be ordered

All events should be signed, verified, etc.

I wonder if we took the wrong approach with the crdt? if we can topologically sort events, we can just use some arbitrary good merge algo and the result/replica will be eventually consistent?

---

I want you to write a new process doc here's my challenge to you:

create a new branch. write an `Rga` trait. then, for each of:

- diamond-types
- loro
- cola
- json-joy
- yjs

do research and write up, in excruciating detail but plain prose:

- what the data structure is
- how the merge algorithm works
- what optimizations

then, create a new file in the crdt folder with the name of that library, and implement, from scratch, that approach. (A trait impl of `Rga`)

I want you to build up a library or vocabulary of common primitives as you work: clocks, b-trees, splay trees, skip lists, caches, maps, and so on, that all the different implementations depend on. With lots of property-based tests too!

That way, each implementation should be a rather minimal composition of CRDT building blocks. Write the code in a way that is compatible with our eventual log ideal.

Then, once all implementations work and pass all tests, benchmark them and:

- implement optimizations
- start a new file that takes the best of everything

when you're done, you can read Scratchpad in Design, BUT NOT BEFORE THEN.

Write a process document in procedures, including instructions for the agent responsible for spinning up subagents and maintaining a tree of progress and a summary of learnings.

---

Is our rga still persistent? Can we jump to any point in time?

---

goals:

- document is a bit like a git repo
  - schema is arbitrary composition of container-like crdts
  - multiple writers, set controlled by admin
  - possibility of forking and transferring ownership
  - can rewind to any previous version from the perspective of any writer
  - branching is cheap
  - something hard:
    - document has writers have branches have events, partially ordered, topological sort
- many CRDT types, think neorpc schema or json
  - lww, counter, etc. for atomic values
  - rga over bytes for strings
  - rga over items for lists
  - sets, bit sets, and maps
  - enums etc
- the crdts are operational; we provide some zipper + some edit, or something to that effect. They are also authenticated; published to the log of the writer, signed, announced
- we need to do some sort of p2p messaging. basically we track a set of cores. we have peers. we track what they know. we have a function prepare message that bundles up all the operations into a compressed and encrypted message of at most a maximum size to send to the peer. likewise we can receive messages from peers and demux them onto our documents
- we need to do some sort of blob store. separate the crdts from the contents of the store. send over the compressed ops, and bulk pull over the backing data. could even be coupled into the same message ofc.
- we need to be able to persist to disk. I think we could do something simple like a sqlite database per document. we would do the blob store elsewhere. there would be a trait perhaps very generic for writing operations; the network and the disk could just plug into this trait. The table has like an admin singleton and a table per core. admin also has a permissions table; admin's log is scanned for membership set events; membership set events are determined by the timestamps of the logs of admin. so if someone is removed but not aware of it if they try editing their future edits will be dropped, even if they arrive before the admin announced their removal. 
- we would have to implement some transport. I think http server, quic, and websockets would be a good basis.
- final step would be wasm / js integration. end goal would be to be able to ship this binary to client and stream back edits.
