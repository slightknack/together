+++
model = "claude-opus-4-5"
created = 2026-02-01
modified = 2026-02-01
driver = "Isaac Clayton"
+++

# Json-Joy Deep Dive

## Overview

- Repository: https://github.com/streamich/json-joy
- Language: TypeScript
- Author: Vadim Dalecky (streamich)
- Primary innovations: Dual-tree indexing with splay trees, block-wise RGA with run-length encoding, Peritext rich text CRDT, sonic-forest high-performance tree library

Json-joy is a comprehensive TypeScript library implementing JSON CRDTs with a focus on performance. The library claims to be the fastest list CRDT implementation in JavaScript, outperforming yjs by 3-10x depending on the workload. It implements a block-wise RGA algorithm using dual splay trees for O(log n) lookups by both position and ID.

The project is funded by NLnet and implements several specifications:
- JSON CRDT: Full JSON document as a CRDT
- JSON CRDT Patch: Operation format for changes
- Peritext: Rich text CRDT for collaborative editing

## Data Structure

### Core Types

Json-joy's architecture separates concerns into several layers:

**Model** (`src/json-crdt/model/Model.ts`): The root container for a JSON CRDT document.

```typescript
export class Model<N extends JsonNode = JsonNode<any>> implements Printable {
  public root: RootNode<N>;           // LWW register for root value
  public clock: clock.IClockVector;   // Logical/vector clock
  public index = new AvlMap<clock.ITimestampStruct, JsonNode>(clock.compare);
  public ext: Extensions;             // Peritext and other extensions
}
```

**JsonNode Types** (`src/json-crdt/nodes/`):
- `RootNode`: LWW register for the document root
- `StrNode`: RGA of UTF-16 code units (text)
- `BinNode`: RGA of bytes (binary data)
- `ArrNode`: RGA of arbitrary JSON values
- `ObjNode`: LWW map for object properties
- `VecNode`: Fixed-size tuple
- `ValNode`: LWW register for scalar values
- `ConNode`: Constant/immutable values

**Chunk** (`src/json-crdt/nodes/rga/AbstractRga.ts`): The fundamental unit of an RGA.

```typescript
export interface Chunk<T> {
  id: ITimestampStruct;      // Unique ID: {sid: number, time: number}
  span: number;              // Length of this chunk (run-length)
  del: boolean;              // Tombstone flag
  data: T | undefined;       // Content (undefined if deleted)
  len: number;               // Visible length of subtree
  
  // Primary tree (position order)
  p: Chunk<T> | undefined;   // Parent
  l: Chunk<T> | undefined;   // Left child
  r: Chunk<T> | undefined;   // Right child
  
  // Secondary tree (ID order)
  p2: Chunk<T> | undefined;  // Parent in ID tree
  l2: Chunk<T> | undefined;  // Left child in ID tree
  r2: Chunk<T> | undefined;  // Right child in ID tree
  
  s: Chunk<T> | undefined;   // Split link to next fragment
}
```

### Dual Tree Indexing

The key innovation in json-joy is maintaining two separate tree indices over the same chunks:

**Tree 1 (Position Order)**: A splay tree ordered by document position. Each node tracks `len`, the total visible length of its subtree. This enables O(log n) position-to-chunk lookups.

**Tree 2 (ID Order)**: A splay tree ordered by (sid, time). This enables O(log n) ID-to-chunk lookups for remote operations that reference specific IDs.

Both trees use the same `Chunk` nodes but different pointer sets (`p/l/r` vs `p2/l2/r2`). This is similar to diamond-types' dual ContentTree/IndexTree design but more tightly integrated.

```
Position Tree (root)              ID Tree (ids)
     [C]                              [B]
    /   \                            /   \
  [A]   [D]                        [A]   [C]
    \                                      \
    [B]                                    [D]

Same chunks, different orderings
```

### Timestamp Structure

Json-joy uses Lamport timestamps with session IDs:

```typescript
export interface ITimestampStruct {
  sid: number;   // Session ID (unique per user)
  time: number;  // Lamport clock value
}
```

The `compare` function orders timestamps by time first, then by session ID:

```typescript
export const compare = (ts1: ITimestampStruct, ts2: ITimestampStruct): -1 | 0 | 1 => {
  const t1 = ts1.time;
  const t2 = ts2.time;
  if (t1 > t2) return 1;
  if (t1 < t2) return -1;
  const s1 = ts1.sid;
  const s2 = ts2.sid;
  if (s1 > s2) return 1;
  if (s1 < s2) return -1;
  return 0;
};
```

This ordering is used for conflict resolution. Unlike yjs and diamond-types which use dual origins (left and right), json-joy uses only a single "after" reference plus timestamp comparison.

### Splay Tree Implementation

Json-joy uses the `sonic-forest` library for tree operations. Splay trees are self-adjusting binary search trees that move recently accessed nodes to the root. This exploits temporal locality: recently edited positions are likely to be edited again.

The splay operation performs rotations to bring a node to the root:

```typescript
public splay(chunk: Chunk<T>): void {
  const p = chunk.p;
  if (!p) return;
  const pp = p.p;
  const l2 = p.l === chunk;
  if (!pp) {
    // Zig: single rotation
    if (l2) rSplay(chunk, p);
    else lSplay(chunk, p);
    this.root = chunk;
    updateLenOne(p);
    updateLenOneLive(chunk);
    return;
  }
  // Zig-zig or zig-zag: double rotation
  const l1 = pp.l === p;
  if (l1) {
    if (l2) this.root = llSplay(this.root!, chunk, p, pp);
    else this.root = lrSplay(this.root!, chunk, p, pp);
  } else {
    if (l2) this.root = rlSplay(this.root!, chunk, p, pp);
    else this.root = rrSplay(this.root!, chunk, p, pp);
  }
  updateLenOne(pp);
  updateLenOne(p);
  updateLenOneLive(chunk);
  this.splay(chunk);  // Recursive until root
}
```

After every insert, the new chunk is splayed to the root. This ensures that:
1. Sequential typing is O(1) amortized (cursor stays near root)
2. Frequently accessed regions stay near the top
3. Tombstones naturally sink lower in the tree over time

## Merge Algorithm

### Ordering Rules

Json-joy implements a simplified RGA algorithm. Unlike YATA (used by yjs and diamond-types) which uses dual origins, json-joy uses only a single "after" reference.

The `insertAfterRef` function handles conflict resolution:

```typescript
protected insertAfterRef(chunk: Chunk<T>, ref: ITimestampStruct, left: Chunk<T>): void {
  const id = chunk.id;
  const sid = id.sid;
  const time = id.time;
  let isSplit: boolean = false;
  
  for (;;) {
    const leftId = left.id;
    const leftNextTick = leftId.time + left.span;
    
    // Check if this is a continuation (split link)
    if (!left.s) {
      isSplit = leftId.sid === sid && leftNextTick === time && leftNextTick - 1 === ref.time;
      if (isSplit) left.s = chunk;
    }
    
    const right = next(left);
    if (!right) break;
    
    const rightId = right.id;
    const rightIdTime = rightId.time;
    const rightIdSid = rightId.sid;
    
    // Order by time first, then session ID
    if (rightIdTime < time) break;
    if (rightIdTime === time) {
      if (rightIdSid === sid) return;  // Duplicate
      if (rightIdSid < sid) break;     // Insert here
    }
    left = right;
  }
  
  // Merge content if this is a continuation of an existing chunk
  if (isSplit && !left.del) {
    this.mergeContent(left, chunk.data!);
    left.s = undefined;
  } else {
    this.insertAfter(chunk, left);
  }
}
```

The ordering rules are:
1. Walk right from the insertion point
2. Stop when we find a chunk with lower timestamp
3. For equal timestamps, lower session ID goes first
4. If this is a continuation of a previous chunk (same user, sequential timestamps), merge the content instead of creating a new chunk

### Conflict Resolution

Concurrent inserts at the same position are ordered by:
1. **Timestamp (descending)**: Higher timestamps come first
2. **Session ID (ascending)**: For equal timestamps, lower session ID comes first

This differs from YATA which uses the dual-origin approach. The simpler approach may produce different interleavings in some concurrent scenarios, but still maintains CRDT convergence.

### Split Links

The `s` (split) pointer tracks when a chunk has been fragmented by concurrent inserts. When a chunk is split, the original chunk and its fragments are connected via split links. This enables:

1. Efficient traversal of logically contiguous ranges
2. Tombstone merging after deletions
3. Content merging when sequential inserts can be coalesced

### Complexity Analysis

**Time Complexity:**

| Operation | Complexity | Notes |
|-----------|------------|-------|
| Insert (local) | O(log n) amortized | Splay brings cursor to root |
| Insert (remote) | O(log n) | ID lookup + position insertion |
| Delete | O(log n + d) | d = deleted range size |
| Position lookup | O(log n) | Position tree traversal |
| ID lookup | O(log n) | ID tree traversal |
| Merge | O(m log n) | m = number of operations |

**Space Complexity:**

- Per chunk: ~120 bytes (JavaScript object overhead)
- With run-length encoding: varies by editing pattern
- Chunk count: 12,387 for automerge-paper trace (vs 10,971 in yjs)

The slightly higher chunk count compared to yjs is likely due to the simpler merge algorithm producing more fragmentation in some cases.

## Optimizations

### Splay Tree Self-Adjusting

The splay operation after each insert exploits temporal locality:

1. **Sequential typing**: After inserting character N, the cursor for character N+1 is already at the root
2. **Local edits**: Editing the same region keeps those chunks near the root
3. **Tombstone sinking**: Deleted chunks are rarely accessed, so they naturally drift toward leaves

Future optimizations discussed in GitHub Issue #228:
- Triple-zip rotations to push tombstones lower faster
- Selective splaying (only splay high-clock-value non-tombstones)

### Block-wise RGA (Run-Length Encoding)

Consecutive insertions from the same user with sequential timestamps are merged into a single chunk:

```typescript
// In StrChunk
public merge(str: string) {
  this.data += str;
  this.span = this.data.length;
}
```

The split link (`s`) tracks fragmentation when chunks are split by concurrent edits:

```typescript
protected split(chunk: Chunk<T>, ticks: number): Chunk<T> {
  const s = chunk.s;
  const newChunk = chunk.split(ticks);
  const r = chunk.r;
  chunk.s = newChunk;
  newChunk.r = r;
  newChunk.s = s;
  chunk.r = newChunk;
  newChunk.p = chunk;
  this.insertId(newChunk);
  if (r) r.p = newChunk;
  return newChunk;
}
```

### Tombstone Merging

Adjacent tombstones with sequential IDs are merged to reduce fragmentation:

```typescript
protected mergeTombstones(ch1: Chunk<T>, ch2: Chunk<T>): boolean {
  if (!ch1.del || !ch2.del) return false;
  const id1 = ch1.id;
  const id2 = ch2.id;
  if (id1.sid !== id2.sid) return false;
  if (id1.time + ch1.span !== id2.time) return false;
  ch1.s = ch2.s;
  ch1.span += ch2.span;
  this.deleteChunk(ch2);
  return true;
}
```

### Subtree Length Caching

Each chunk tracks `len`, the total visible length of its subtree:

```typescript
const updateLenOne = (chunk: Chunk<unknown>): void => {
  const l = chunk.l;
  const r = chunk.r;
  chunk.len = (chunk.del ? 0 : chunk.span) + (l ? l.len : 0) + (r ? r.len : 0);
};
```

This enables O(log n) position lookups without traversing the entire tree:

```typescript
public findChunk(position: number): undefined | [chunk: Chunk<T>, offset: number] {
  let curr = this.root;
  while (curr) {
    const l = curr.l;
    const leftLength = l ? l.len : 0;
    let span: number;
    if (position < leftLength) curr = l;
    else if (curr.del) {
      position -= leftLength;
      curr = curr.r;
    } else if (position < leftLength + (span = curr.span)) {
      return [curr, position - leftLength];
    } else {
      position -= leftLength + span;
      curr = curr.r;
    }
  }
  return;
}
```

### Peritext Rich Text

Peritext is implemented on top of the base RGA in `src/json-crdt-extensions/peritext/`. It provides:

1. **Slices**: Annotations (bold, italic, links) stored as ranges over the RGA
2. **Overlay**: A tree structure for fast slice intersection queries
3. **Blocks**: Block-level formatting (paragraphs, headings, lists)
4. **Markers**: Sentinel characters for block boundaries

The Overlay uses dual trees similar to the RGA:

```typescript
export class Overlay<T = string> implements Printable, Stateful {
  public root: OverlayPoint<T> | undefined = undefined;   // Position order
  public root2: OverlayPoint<T> | undefined = undefined;  // Marker order
}
```

### sonic-forest Tree Library

Json-joy extracts its tree implementations into a separate package `sonic-forest` which provides:

- AVL trees and sorted maps
- Red-black trees
- Splay trees
- Radix trees (string and binary keys)
- Utility functions for all tree types

The library claims to be the fastest insertion implementation for self-balancing binary trees in JavaScript.

## Code Walkthrough

### Inserting Text Locally

```typescript
// In StrNode (extends AbstractRga)
public insAt(position: number, id: ITimestampStruct, content: T): ITimestampStruct | undefined {
  if (!position) {
    const rootId = this.id;
    this.insAfterRoot(rootId, id, content);
    return rootId;
  }
  // Find chunk at position-1 (the character to insert after)
  const found = this.findChunk(position - 1);
  if (!found) return undefined;
  const [at, offset] = found;
  const atId = at.id;
  const after = offset === 0 ? atId : new Timestamp(atId.sid, atId.time + offset);
  this.insAfterChunk(after, at, offset, id, content);
  return after;
}
```

### Applying Remote Insert

```typescript
public ins(after: ITimestampStruct, id: ITimestampStruct, content: T): void {
  const rootId = this.id;
  const afterTime = after.time;
  const afterSid = after.sid;
  const isRootInsert = rootId.time === afterTime && rootId.sid === afterSid;
  if (isRootInsert) {
    this.insAfterRoot(after, id, content);
    return;
  }
  
  // Find the chunk containing the 'after' ID using the ID tree
  let curr: Chunk<T> | undefined = this.ids;  // ID tree root
  let chunk: Chunk<T> | undefined = curr;
  while (curr) {
    const currId = curr.id;
    const currIdSid = currId.sid;
    if (currIdSid > afterSid) curr = curr.l2;
    else if (currIdSid < afterSid) {
      chunk = curr;
      curr = curr.r2;
    } else {
      // Same session, compare time
      const currIdTime = currId.time;
      if (currIdTime > afterTime) curr = curr.l2;
      else if (currIdTime < afterTime) {
        chunk = curr;
        curr = curr.r2;
      } else {
        chunk = curr;
        break;
      }
    }
  }
  if (!chunk) return;
  
  // Insert after the found chunk
  const offsetInInsertAtChunk = afterTime - chunk.id.time;
  this.insAfterChunk(after, chunk, offsetInInsertAtChunk, id, content);
}
```

### Deletion

```typescript
protected deleteSpan(span: ITimespanStruct): void {
  const len = span.span;
  const t1 = span.time;
  const t2 = t1 + len - 1;
  const start = this.findById(span);
  if (!start) return;
  
  let chunk: Chunk<T> | undefined = start;
  while (chunk) {
    const id = chunk.id;
    const chunkSpan = chunk.span;
    const c1 = id.time;
    const c2 = c1 + chunkSpan - 1;
    
    if (chunk.del) {
      // Already deleted, follow split link
      if (c2 >= t2) break;
      chunk = chunk.s;
      continue;
    }
    
    // Handle partial overlap cases
    const deleteStartsFromLeft = t1 <= c1;
    const deleteStartsInTheMiddle = t1 <= c2;
    
    if (deleteStartsFromLeft) {
      const deleteFullyContainsChunk = t2 >= c2;
      if (deleteFullyContainsChunk) {
        chunk.delete();
        dLen(chunk, -chunk.span);
        if (t2 <= c2) break;
      } else {
        // Split and delete left portion
        const range = t2 - c1 + 1;
        const newChunk = this.split(chunk, range);
        chunk.delete();
        updateLenOne(newChunk);
        dLen(chunk, -chunk.span);
        break;
      }
    }
    // ... more cases for middle splits
    
    chunk = chunk.s;
  }
  
  // Merge adjacent tombstones
  this.mergeTombstones2(start, last);
}
```

## Comparison with Previous Libraries

### vs Yjs

| Aspect | Yjs | Json-joy |
|--------|-----|----------|
| Language | JavaScript | TypeScript |
| Algorithm | YATA (dual origins) | RGA (single origin) |
| Position lookup | O(1) with markers, O(n) worst | O(log n) always |
| ID lookup | O(log n) via StructStore | O(log n) via ID tree |
| Data structure | Doubly-linked list | Dual splay trees |
| Content storage | Inline in Items | Inline in Chunks |
| Rich text | YText with inline formatting | Peritext extension |
| Memory | ~80 bytes/char (JS objects) | ~120 bytes/char (dual pointers) |

Json-joy is 3-10x faster than yjs on benchmarks but uses more memory due to dual tree pointers.

### vs Diamond-types

| Aspect | Diamond-types | Json-joy |
|--------|---------------|----------|
| Language | Rust | TypeScript |
| Content storage | Separate JumpRope | Inline in Chunks |
| Position lookup | B-tree O(log n) | Splay tree O(log n) amortized |
| ID lookup | B-tree O(log n) | Splay tree O(log n) amortized |
| Time travel | First-class with advance/retreat | Not built-in |
| Dual origins | Yes (YATA-compatible) | No |

Diamond-types (native) is still faster than json-joy due to Rust vs JavaScript. On the Martin Kleppmann trace:
- Diamond-types: 48ms
- Json-joy: 74ms
- Yjs: 922ms

### vs Cola

| Aspect | Cola | Json-joy |
|--------|------|----------|
| Language | Rust | TypeScript |
| Content storage | External (user-managed) | Inline in Chunks |
| Algorithm | Lamport + anchor | RGA + timestamp |
| Data structure | Gtree (grow-only B-tree) | Dual splay trees |
| Memory model | Grow-only | Tombstones |

Cola's approach of decoupling content storage is interesting. Json-joy stores content inline which is simpler but less flexible.

## Lessons for Our Implementation

### What to Adopt

1. **Dual tree indexing**: Maintaining both position-order and ID-order indices is essential for O(log n) operations in both local and remote contexts. Json-joy's approach of embedding both pointer sets in the same node is space-efficient compared to diamond-types' separate trees.

2. **Splay trees for temporal locality**: The self-adjusting property of splay trees is perfect for text editing where the cursor moves sequentially. After typing 100 characters, all 100 lookups are O(1) amortized.

3. **Subtree length caching**: Storing `len` in each node enables efficient position lookups without maintaining a separate index.

4. **Split links for fragmented chunks**: The `s` pointer elegantly handles chunk fragmentation from concurrent edits while enabling tombstone merging.

5. **sonic-forest extraction**: Extracting tree implementations into a reusable library is good engineering. We could adopt similar modularity.

### What to Consider Differently

1. **Simpler merge algorithm**: Json-joy uses only a single "after" reference, while YATA uses dual origins. The simpler approach works but may produce different interleavings. For compatibility with yjs and diamond-types, we may want YATA.

2. **JavaScript object overhead**: Each chunk in json-joy is a JavaScript object with high overhead. In Rust, we can use more compact representations with arena allocation.

3. **No time travel**: Json-joy lacks built-in time travel support. Diamond-types' advance/retreat approach is more flexible if we need version history.

4. **Dual pointers overhead**: Each chunk has 6 extra pointers for the second tree. In memory-constrained environments, diamond-types' separate index might be preferable.

5. **Peritext integration**: Json-joy's Peritext implementation shows how to layer rich text on top of a plain-text RGA. We should study this for our own rich text support.

### Primitives to Extract

From json-joy's implementation, useful primitives include:

- **Splay tree operations**: The `sonic-forest` library's splay implementation is well-tested and fast
- **Dual-pointer node structure**: Embedding two tree structures in one node
- **Timestamp with session ID**: Simple but effective ID scheme
- **Subtree length tracking**: `len` field with efficient updates
- **Split links**: The `s` pointer for tracking fragmentation

## Sources

- [json-joy GitHub](https://github.com/streamich/json-joy)
- [sonic-forest npm](https://www.npmjs.com/package/sonic-forest)
- [NLnet JSON-Joy Peritext](https://nlnet.nl/project/JSON-Joy-Peritext/)
- [List CRDT Benchmarks](https://jsonjoy.com/blog/list-crdt-benchmarks)
- [Blazing Fast List CRDT](https://jsonjoy.com/blog/performant-rga-list-crdt-algorithm)
- [JSON CRDT 2.0 Discussion](https://github.com/streamich/json-joy/issues/228)
- [JSON CRDT Specification](https://jsonjoy.com/specs/json-crdt)
- Source code analysis of `/tmp/json-joy/packages/json-joy/src/`
