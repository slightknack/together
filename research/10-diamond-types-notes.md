+++
model = "claude-opus-4-5"
created = 2026-01-31
modified = 2026-01-31
driver = "Isaac Clayton"
+++

# Notes: Diamond-types Deep Dive

Source: https://github.com/josephg/diamond-types

## Architecture Overview

Diamond-types separates concerns:

1. **JumpRope** (jumprope-rs): Text storage with O(log n) position operations
2. **ContentTree** (ost/content_tree.rs): B-tree for CRDT spans with position tracking
3. **IndexTree** (ost/index_tree.rs): Separate index for ID -> position lookup
4. **ListBranch**: Current document state (wraps JumpRopeBuf)
5. **ListOpLog**: Operation log / history

## JumpRope Architecture

JumpRope is a skip list where each node contains a **gap buffer** for text.

### Key Constants
```rust
const NODE_STR_SIZE: usize = 392;  // bytes per node (release)
const MAX_HEIGHT: usize = 20;
const BIAS: u8 = 65;  // probability of height increase (65/256 ≈ 25%)
```

### Node Structure
```rust
struct Node {
    str: GapBuffer<NODE_STR_SIZE>,  // 392 bytes of text storage
    height: u8,
    nexts: [SkipEntry; MAX_HEIGHT+1],
}

struct SkipEntry {
    node: *mut Node,
    skip_chars: usize,  // characters skipped by this edge
}
```

### Navigation Algorithm
```rust
fn mut_cursor_at_char(&mut self, char_pos: usize) -> MutCursor {
    let mut offset = char_pos;
    let mut height = self.head.height - 1;
    
    loop {
        let next = node.nexts[height];
        if offset > next.skip_chars {
            // Go right
            offset -= next.skip_chars;
            node = next.node;
        } else {
            // Record and go down
            cursor.inner[height] = SkipEntry { node, skip_chars: offset };
            if height == 0 { break; }
            height -= 1;
        }
    }
}
```

This is O(log n) because:
- Each level skips exponentially more nodes
- ~log2(n) levels in expectation

### Gap Buffer
Each node stores text in a gap buffer (392 bytes):
```rust
struct GapBuffer<const LEN: usize> {
    data: [u8; LEN],
    gap_start_bytes: u16,
    gap_start_chars: u16,
    gap_len: u16,
    all_ascii: bool,
}
```

Benefits:
- Sequential inserts at same position are O(1) - just extend the gap
- No skip list rebalancing for local edits
- ASCII fast path when `all_ascii = true`

## ContentTree (B-tree for CRDT spans)

### Constants
```rust
const NODE_CHILDREN: usize = 16;  // internal node fanout
const LEAF_CHILDREN: usize = 32;  // items per leaf
```

### Structure
```rust
struct ContentTree<V: Content> {
    leaves: Vec<ContentLeaf<V>>,
    nodes: Vec<ContentNode>,
    height: usize,
    root: usize,
    total_len: LenPair,
    cursor: Option<(Option<LenPair>, DeltaCursor)>,  // cached cursor
}

struct ContentLeaf<V> {
    children: [V; LEAF_CHILDREN],  // 32 spans per leaf
    next_leaf: LeafIdx,
    parent: NodeIdx,
}

struct ContentNode {
    child_indexes: [usize; NODE_CHILDREN],
    child_width: [LenPair; NODE_CHILDREN],  // cumulative widths
    parent: NodeIdx,
}
```

### LenPair - Dual Position Tracking
```rust
struct LenPair {
    cur: usize,  // current visible length
    end: usize,  // length in final merged state
}
```

This enables efficient position lookup for both:
- Current document state (for editing)
- Final merged state (for CRDT operations)

### Navigation
O(log n) descent through B-tree using cumulative widths at each level.

## CRDTSpan - The Item Type

```rust
struct CRDTSpan {
    id: DTRange,              // 16 bytes - (start, end) version range
    origin_left: LV,          // 8 bytes - parent on left
    origin_right: LV,         // 8 bytes - parent on right  
    current_state: SpanState, // 4 bytes - inserted/deleted state
    end_state_ever_deleted: bool, // 1 byte + 3 padding
}
// Total: 40 bytes
```

Compare to our Span: **40 bytes vs 112 bytes** (2.8x smaller)

Key differences:
1. **Agent as index**: They use `usize` IDs, maintain separate agent->name mapping
2. **Origin as index**: `origin_left/right` are span indices, not full ItemIds
3. **No content_offset**: Text stored separately in JumpRope

## Key Optimizations

### 1. Separation of Content and CRDT State
- **JumpRope**: Stores actual text, optimized for editing
- **ContentTree**: Stores CRDT spans, optimized for merge operations
- Two data structures, each optimized for its purpose

### 2. Agent ID Indirection
Instead of storing 32-byte public keys in every span:
```rust
// Their approach
agent_id: usize  // 8 bytes, index into agent table

// vs our approach  
user: KeyPub     // 32 bytes inline
```

### 3. Cursor Caching with Delta Updates
```rust
cursor: Option<(Option<LenPair>, DeltaCursor)>
```
- Cache last cursor position
- Accumulate delta updates lazily
- Flush on position change

### 4. Vec-based B-tree (no raw pointers)
```rust
leaves: Vec<ContentLeaf<V>>,
nodes: Vec<ContentNode>,
```
"Surprisingly, this turns out to perform better - because the CPU ends up caching runs of nodes."

### 5. RLE via `MergableSpan` trait
```rust
trait MergableSpan {
    fn can_append(&self, other: &Self) -> bool;
    fn append(&mut self, other: Self);
}
```
Spans automatically coalesce when adjacent and compatible.

## Comparison to Our Implementation

| Aspect | Diamond-types | Together |
|--------|---------------|----------|
| Text storage | JumpRope (skip list + gap buffer) | Inline in spans |
| CRDT storage | ContentTree (B-tree) | WeightedList (chunked Vec) |
| Position lookup | O(log n) | O(sqrt n) |
| Span size | 40 bytes | 112 bytes |
| Agent storage | Index (8 bytes) | Inline (32 bytes) |
| Origin storage | Index (8 bytes) | Option<ItemId> (48 bytes) |

## Lessons for Our Implementation

1. **Compact spans are critical**: 40 vs 112 bytes = 2.8x cache improvement
   - Use agent index instead of inline KeyPub
   - Use span index for origin instead of full ItemId

2. **B-tree > chunked list for large n**: O(log n) vs O(sqrt n)
   - Our skip list could work if adapted for weighted lookup

3. **Separate text from CRDT state**: 
   - JumpRope for fast text editing
   - ContentTree for CRDT operations
   - We combine both in spans

4. **Gap buffer for local edits**: 
   - Sequential typing is O(1)
   - We coalesce spans but still do list operations

5. **Vec-based trees work well**:
   - No raw pointers needed
   - Better cache behavior than pointer-based trees
