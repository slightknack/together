---
model = "claude-opus-4-5"
created = "2026-01-31"
modified = "2026-01-31"
driver = "Isaac Clayton"
---

# Notes: JumpRope Deep Dive

Source: https://github.com/josephg/jumprope-rs

## Overview

JumpRope is a rope (string) data structure optimized for text editing operations.
It's "the world's fastest rope implementation" - processing ~35-40 million edits/second.

Key innovation: **Skip list + gap buffers** hybrid architecture.

## Architecture

```
JumpRope
├── head: Node (embedded, first node)
├── rng: RopeRng (for random skip list heights)
└── num_bytes: usize

Node
├── str: GapBuffer<NODE_STR_SIZE>  // 392 bytes of text
├── height: u8                     // skip list height (1-20)
└── nexts: [SkipEntry; MAX_HEIGHT+1]

SkipEntry
├── node: *mut Node       // next node at this level
└── skip_chars: usize     // characters skipped by this edge
```

## Key Constants

```rust
const NODE_STR_SIZE: usize = 392;  // bytes per gap buffer (release)
const MAX_HEIGHT: usize = 20;
const BIAS: u8 = 65;  // probability 65/256 ≈ 25% of height increase
```

Debug mode uses `NODE_STR_SIZE = 10` for easier testing.

## Skip List Navigation - O(log n)

```rust
fn mut_cursor_at_char(&mut self, char_pos: usize, stick_end: bool) -> MutCursor {
    let mut offset = char_pos;
    let mut height = head_height - 1;
    let mut e: *mut Node = &mut self.head;

    loop {
        let next = (*e).nexts[height];
        let skip = next.skip_chars;
        
        if offset > skip || (!stick_end && offset == skip && !next.node.is_null()) {
            // Go right
            offset -= skip;
            e = next.node;
        } else {
            // Record and go down
            cursor.inner[height] = SkipEntry { node: e, skip_chars: offset };
            if height == 0 { break; }
            height -= 1;
        }
    }
}
```

The cursor stores the path through the skip list - needed for updates.

## Gap Buffer - O(1) Sequential Edits

Each node stores text in a fixed-size gap buffer:

```rust
struct GapBuffer<const LEN: usize> {
    data: [u8; LEN],           // 392 bytes
    gap_start_bytes: u16,
    gap_start_chars: u16,
    gap_len: u16,
    all_ascii: bool,           // enables fast paths
}
```

### Gap Buffer Operations

1. **Insert at gap** - O(1):
   ```rust
   fn insert_in_gap(&mut self, s: &str) {
       self.data[start..start+len].copy_from_slice(s.as_bytes());
       self.gap_start_bytes += len;
       self.gap_start_chars += char_len;
       self.gap_len -= len;
   }
   ```

2. **Move gap** - O(node size):
   ```rust
   fn move_gap(&mut self, new_start_bytes: usize) {
       // Move bytes left or right to reposition gap
       self.data.copy_within(moved_chars, destination);
   }
   ```

3. **Insert anywhere** - move gap then insert:
   ```rust
   fn try_insert(&mut self, byte_pos: usize, s: &str) -> Result<(), ()> {
       self.move_gap(byte_pos);
       self.insert_in_gap(s);
   }
   ```

Sequential typing at same position: just extend the gap = O(1).
Random insertion: move gap first = O(node size), still cheap.

## Insert Algorithm

```rust
fn insert_at_cursor(cursor: &mut MutCursor, contents: &str) {
    // Fast path: if gap is at cursor position and has space
    if gap_at_cursor && gap_has_space {
        node.str.insert_in_gap(contents);
        cursor.update_offsets(num_inserted_chars);
        return;  // O(1)!
    }
    
    // Check if can insert in current node
    if current_len + contents.len() <= NODE_STR_SIZE {
        node.str.try_insert(offset_bytes, contents);
        cursor.update_offsets(num_inserted_chars);
        return;
    }
    
    // Must create new nodes
    // Split content into NODE_STR_SIZE chunks
    // Insert each chunk as a new skip list node
    for chunk in contents.chunks(NODE_STR_SIZE) {
        insert_node_at(cursor, chunk, ...);
    }
}
```

## Delete Algorithm

```rust
fn del_at_cursor(cursor: &mut MutCursor, mut length: usize) {
    while length > 0 {
        let removed = min(length, node.num_chars() - offset);
        
        if removed < num_chars || is_head_node {
            // Partial delete: just update gap buffer
            node.str.remove_chars(offset, removed);
            update_skip_entries(-removed);
        } else {
            // Full node removal from skip list
            for i in 0..node.height {
                prev_entry.node = node.next.node;
                prev_entry.skip_chars += node.next.skip_chars - removed;
            }
            drop(Box::from_raw(node));
        }
        
        length -= removed;
    }
}
```

## JumpRopeBuf - Buffered Writes

Wrapper that batches adjacent operations before applying:

```rust
struct JumpRopeBuf(RefCell<(JumpRope, BufferedOp)>);

struct BufferedOp {
    kind: Kind,           // Ins or Del
    ins_content: String,  // accumulated insert content
    range: Range<usize>,  // affected range
}
```

Adjacent inserts/deletes are merged:
- Insert at end of pending insert: extend content
- Delete at end of pending insert: trim pending
- Delete adjacent to pending delete: extend range

**Performance impact: ~10x faster** for sequential editing patterns.

## Why It's Fast

1. **O(log n) position lookup**: Skip list with ~log2(n) levels
2. **O(1) sequential typing**: Gap buffer absorbs consecutive inserts
3. **Large node size (392 bytes)**: Reduces tree depth
4. **Buffered writes**: JumpRopeBuf amortizes many small operations
5. **ASCII fast path**: `all_ascii` flag enables byte=char shortcuts
6. **No allocator per insert**: Gap buffer reuses space within node

## Benchmarks (from README)

On Ryzen 5800X, single core:

| Dataset         | Ropey    | JumpRope |
|-----------------|----------|----------|
| automerge-paper | 25.16 ms | 6.66 ms  |
| rustcode        | 4.71 ms  | 1.66 ms  |
| sveltecomponent | 2.31 ms  | 0.59 ms  |
| seph-blog1      | 13.04 ms | 3.81 ms  |

JumpRope is **~3-4x faster than Ropey**.

## Comparison to Our Implementation

| Aspect | JumpRope | Together RGA |
|--------|----------|--------------|
| Position lookup | O(log n) skip list | O(sqrt n) chunked list |
| Sequential insert | O(1) gap buffer | O(chunk size) |
| Node size | 392 bytes text + metadata | 112 byte spans |
| Memory layout | Pointer-based nodes | Vec of chunks |
| Buffering | JumpRopeBuf wrapper | None |

## Key Takeaways for Our Optimization

1. **Gap buffer for content**: Store actual text in gap buffers, not inline in spans
   - Sequential typing becomes O(1)
   - JumpRope proves this is the performance winner

2. **Skip list with proper weights**: Our skip_list.rs exists but tracks counts
   - Adapt it to track character weights
   - Would give O(log n) position lookup

3. **Buffered writes**: Consider a buffered wrapper
   - Diamond-types uses this via JumpRopeBuf
   - 10x improvement for editing traces

4. **Large node capacity**: 392 bytes > our approach
   - Reduces tree depth
   - Better cache utilization

5. **Separate text from CRDT metadata**:
   - JumpRope stores text efficiently
   - Diamond-types stores CRDT spans in separate B-tree
   - We mix content_offset into spans
