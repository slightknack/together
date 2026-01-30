---
model = "claude-opus-4-5"
created = "2026-01-30"
modified = "2026-01-30"
driver = "Isaac Clayton"
---

# Diamond Types Architecture

Diamond-types is described as "the world's fastest CRDT." This document analyzes its architecture to inform together's design.

Sources:
- https://github.com/josephg/diamond-types
- https://josephg.com/blog/crdts-go-brrr/
- Local source inspection: `~/.cargo/registry/src/*/diamond-types-*/`

## Core Architecture: Separation of Concerns

Diamond-types separates three concerns:

### 1. OpLog (Operation Log)

Stores the complete history of all operations in a compact format:

```rust
pub struct OpLog {
    client_with_localtime: RleVec<KVPair<CRDTSpan>>,  // Order -> CRDT location
    client_data: Vec<ClientData>,                      // Agent data
    operation_ctx: OperationCtx,                       // Inserted content
    operations: RleVec<KVPair<OperationInternal>>,    // The actual ops
    history: History,                                  // Time DAG
    version: LocalVersion,                             // Current version
}
```

Key insight: Operations are stored in **Structure of Arrays (SoA)** format, allowing each field to be run-length encoded independently.

### 2. Branch (Document State)

A snapshot of the document at a specific version:

```rust
pub struct Branch {
    version: LocalVersion,
    content: JumpRope,  // The actual text
}
```

Key insight: **Content storage is separate from CRDT metadata.** The Branch just holds a JumpRope (efficient text buffer) and a version. All CRDT logic lives in OpLog.

### 3. ListCRDT (Convenience Wrapper)

```rust
pub struct ListCRDT {
    pub branch: Branch,
    pub oplog: OpLog,
}
```

Simple wrapper for the common case of one branch + one oplog.

## JumpRope: The Content Buffer

JumpRope is a skip list of gap buffers. Key properties:

- O(log n) insert/delete at any position
- O(log n) position-to-index conversion
- Cache-friendly: nodes are ~400 bytes with inline strings
- Probabilistic balancing (skip list) - simpler than B-tree rebalancing

```rust
pub struct JumpRope {
    rng: RopeRng,
    num_bytes: usize,
    head: Node,
}

struct Node {
    str: GapBuffer<NODE_STR_SIZE>,  // ~392 bytes inline
    height: u8,
    nexts: [SkipEntry; MAX_HEIGHT+1],
}

struct SkipEntry {
    node: *mut Node,
    skip_chars: usize,  // Characters to skip to reach next
}
```

The `skip_chars` field is the key: each level of the skip list stores how many characters are skipped, enabling O(log n) position lookups.

## Run-Length Encoding (RleVec)

Everything is run-length encoded. For example, typing "hello" creates one span, not five operations:

```rust
// Instead of: [Insert('h'), Insert('e'), Insert('l'), Insert('l'), Insert('o')]
// Store: Insert { pos: 0, len: 5, content: "hello" }
```

The `RleVec<T>` type handles this transparently for any type that implements `MergableSpan`.

## Time DAG vs Linear Log

Diamond-types tracks causality with a Time DAG:

- Each operation has parents (the version when it was created)
- Merges are implicit - no special merge nodes
- Operations are stored in a linear log but reference their causal parents

This differs from together's current approach where each user has a separate linear log.

## API Design Lessons

### Good patterns to adopt:

1. **Separation of log and state**: Keep operation history separate from current document state. This enables:
   - Multiple branches from one history
   - Time travel (checkout any version)
   - Efficient sync (just send missing ops)

2. **Skip list over B-tree**: For text editing, skip lists are simpler and fast enough. No complex rebalancing code.

3. **Inline strings in nodes**: Gap buffers of ~400 bytes mean most edits don't allocate.

4. **RLE everywhere**: Run-length encode operations, not just content.

5. **Position caching**: Track last edit position to speed up sequential edits.

### Differences from together's design:

| Aspect | Diamond-types | Together |
|--------|--------------|----------|
| History | Single Time DAG | Per-user append-only logs |
| Content | JumpRope (skip list) | Vec in flat spans |
| Auth | None built-in | Signed logs |
| Branching | First-class | Not yet |

## Implications for Together

### Keep:
- Per-user signed append-only logs (authentication is a core feature)
- Span-based storage (already doing this)

### Add:
- Skip list or B-tree for O(log n) position lookups
- Index from ItemId -> span location
- Proper partial span deletion

### Consider:
- Separating `Rga` into `RgaLog` (ops) + `RgaBranch` (state)
- Using JumpRope as the content buffer instead of rolling our own
- RLE for operation storage in logs

## Performance Numbers

From "CRDTs go brrr":
- Automerge (tree): 291 seconds
- Reference (flat list): 31 seconds
- Yjs (linked list + spans): 0.97 seconds  
- Diamond/Rust (skip list + native): 0.056 seconds

The 5000x speedup comes from:
1. Spans (14x memory reduction)
2. Skip list (O(log n) vs O(n))
3. Cache-friendly layout
4. Native code vs JS
