---
model = "claude-opus-4-5"
created = "2026-01-31"
modified = "2026-01-31"
driver = "Isaac Clayton"
---

# JumpRope Analysis: How diamond-types Achieves High Performance

## Overview

Diamond-types uses JumpRope (https://github.com/josephg/jumprope-rs) for text storage. JumpRope is a skip list where each leaf node contains a gap buffer instead of a single item. This hybrid achieves excellent real-world performance.

## Key Architecture: Skip List + Gap Buffer

### Skip List Structure

```rust
pub struct JumpRope {
    rng: RopeRng,
    num_bytes: usize,
    head: Node,
    // head has variable-height nexts array
}

struct Node {
    str: GapBuffer<NODE_STR_SIZE>,  // 392 bytes in release, 10 in debug
    height: u8,
    nexts: [SkipEntry; MAX_HEIGHT+1],
}

struct SkipEntry {
    node: *mut Node,
    skip_chars: usize,  // Characters between current and next node
}
```

Each node contains a GapBuffer of text, not a single character. The skip_chars at each level tracks the total character count spanned.

### Gap Buffer per Node

Each node stores up to 392 bytes of text in a gap buffer:

```rust
struct GapBuffer<const LEN: usize> {
    data: [u8; LEN],
    gap_start_bytes: u16,
    gap_start_chars: u16,
    gap_len: u16,
    all_ascii: bool,  // Fast path optimization
}
```

Benefits:
1. Sequential inserts within a node are O(1) - just extend the gap
2. No skip list rebalancing for local edits
3. Amortized constant factor much lower than per-character nodes

### Cursor for Navigation

Navigation uses a cursor that records the path down:

```rust
struct MutCursor<'a> {
    inner: [SkipEntry; MAX_HEIGHT+1],  // Path from head
    rng: &'a mut RopeRng,
    num_bytes: &'a mut usize,
}
```

The cursor tracks position at each level. After finding a position:
- `inner[0].skip_chars` = offset within current node
- `inner[height-1].skip_chars` = global character position

This enables:
1. O(log n) navigation to any position
2. Efficient updates via cursor path
3. No separate index lookup needed

### Insert Algorithm

```rust
fn insert(&mut self, pos: usize, contents: &str) {
    let mut cursor = self.mut_cursor_at_char(pos, true);  // O(log n)
    Self::insert_at_cursor(&mut cursor, contents);
}
```

Insert at cursor:
1. If contents fit in current node's gap buffer -> just insert there
2. If node would overflow -> split node and/or create new nodes
3. Update skip_chars along the cursor path

### Node Sizing

Constants tuned for performance:
- `NODE_STR_SIZE = 392` bytes in release (10 in debug for testing)
- `MAX_HEIGHT = 20` levels
- `BIAS = 65` (probability of height increase, out of 256)

392 bytes is ~6 cache lines. Large enough to amortize skip list overhead, small enough for good cache behavior.

## Why This Is Fast

### 1. Batched Local Edits
Gap buffer makes sequential inserts at same location O(1). Typing "hello" is 5 O(1) ops, not 5 O(log n) skip list modifications.

### 2. Cache-Friendly Layout
Each node is a contiguous 400+ byte block. Linear scan within node is cache-efficient.

### 3. Skip List Only for Navigation
Skip list finds the right node in O(log n), then gap buffer handles the actual edit. Decouples navigation from mutation.

### 4. ASCII Fast Path
`all_ascii` flag enables byte-indexing instead of char-indexing for common case.

### 5. Minimal Allocations
Nodes are reused. Gap buffer avoids allocation for local edits.

## Comparison to Our Chunked List

| Aspect | JumpRope | Our Chunked List |
|--------|----------|------------------|
| Navigation | O(log n) skip list | O(sqrt n) linear chunk scan |
| Local edit | O(1) gap buffer | O(chunk_size) Vec insert |
| Memory | ~400 bytes/node | 64 items * size_of(T)/chunk |
| Rebalancing | Probabilistic | Chunk split at MAX_SIZE |

Our chunked list is simpler but has worse constants:
- We scan ~300 chunks linearly vs O(log n) skip list descent
- We Vec::insert within chunks vs O(1) gap buffer extension

## What We Could Adopt

### Option A: Add Gap Buffer to Chunks
Keep chunked list but use gap buffer within each chunk. Would help sequential inserts within a chunk.

**Benefit**: ~2x for sequential local edits
**Complexity**: Medium - need to track gap position per chunk

### Option B: Replace with Skip List + Gap Buffer
Port JumpRope-style architecture for WeightedList.

**Benefit**: O(log n) navigation, O(1) local edits
**Complexity**: High - significant rewrite

### Option C: Span Coalescing
Reduce span fragmentation by merging adjacent spans with same origin.

**Benefit**: Reduce span count from ~20k to potentially ~1k
**Complexity**: Low - modify RGA merge logic

## Recommendation

**Start with Option C (span coalescing)** because:
1. Low complexity, contained change
2. Addresses root cause of fragmentation
3. Reduces work for any data structure approach
4. Should give measurable improvement

If still not fast enough, then consider Option B (skip list + gap buffer hybrid).

## References

- JumpRope: https://github.com/josephg/jumprope-rs
- Diamond-types: https://github.com/josephg/diamond-types
- Gap buffer: https://en.wikipedia.org/wiki/Gap_buffer
