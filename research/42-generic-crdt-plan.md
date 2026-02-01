+++
model = "claude-opus-4-5"
created = 2026-02-01
modified = 2026-02-01
driver = "Isaac Clayton"
+++

# Research Plan: Generic and Nested CRDTs

## Problem Statement

Currently, RGA operates on bytes (`&[u8]`). This limits its use to text documents. We want to explore:

1. Making RGA generic over the item type
2. Supporting nested CRDTs (items that are themselves CRDTs)

## Current Implementation

```rust
// Current: bytes only
impl Rga {
    pub fn insert(&mut self, user: &KeyPub, pos: u64, content: &[u8]);
}
```

Content is stored in per-user `Column` buffers as `Vec<u8>`. Spans reference slices into these buffers.

## Goal 1: Generic RGA

Make RGA work with arbitrary item types:

```rust
pub trait RgaItem: Clone + Eq {
    // Serialization for storage/network
    fn to_bytes(&self) -> Vec<u8>;
    fn from_bytes(bytes: &[u8]) -> Self;
}

impl<T: RgaItem> Rga<T> {
    pub fn insert(&mut self, user: &KeyPub, pos: u64, items: &[T]);
}
```

### Challenges

1. **Storage**: Currently uses contiguous byte buffers. Generic items may have variable size.
   - Option A: Store `Vec<T>` instead of `Vec<u8>` per column
   - Option B: Serialize items to bytes, deserialize on access
   - Option C: Arena allocation with stable indices

2. **Span coalescing**: Currently merges consecutive byte ranges. With generic items:
   - May not be meaningful to coalesce (items may not be "adjacent" conceptually)
   - Could coalesce by count instead of byte length

3. **Slicing**: `slice(start, end)` returns `String`. With generic items:
   - Return `Vec<T>` instead
   - Or return an iterator

4. **Performance**: Byte operations are cache-friendly. Generic items may not be.
   - Consider `SmallVec` or inline storage for small items
   - Profile before optimizing

### Implementation Approach

```rust
pub struct Rga<T: RgaItem = u8> {
    spans: BTreeList<Span>,
    columns: FxHashMap<u16, Column<T>>,
    // ...
}

pub struct Column<T> {
    content: Vec<T>,
}

pub struct Span {
    // Same structure, but len is item count, not byte count
    len: u32,
    // ...
}
```

### Use Cases

- Text editor: `Rga<char>` or `Rga<u8>`
- Todo list: `Rga<TodoItem>`
- Spreadsheet cells: `Rga<Cell>`
- JSON array: `Rga<JsonValue>`

## Goal 2: Nested CRDTs

Items that are themselves CRDTs, enabling recursive data structures.

```rust
// A CRDT whose items are also CRDTs
struct Document {
    paragraphs: Rga<Paragraph>,
}

struct Paragraph {
    text: Rga<char>,
    formatting: LWWMap<Range, Style>,
}
```

### Challenges

1. **Identity**: Nested CRDTs need stable IDs for conflict resolution.
   - Parent RGA assigns ID to each item slot
   - Nested CRDT uses parent ID as namespace

2. **Conflict resolution**: When two users modify the same nested CRDT:
   - The outer RGA sees the item slot as unchanged
   - The inner CRDT must resolve its own conflicts
   - Need to propagate operations through the hierarchy

3. **Merge semantics**: Merging outer RGA merges inner CRDTs.
   ```rust
   // When merging paragraphs[i]:
   self.paragraphs[i].text.merge(&other.paragraphs[i].text);
   ```

4. **Versioning**: Version vectors must include nested operations.
   - Option A: Flat version vector (all ops in one namespace)
   - Option B: Hierarchical version vectors

5. **Garbage collection**: When is it safe to remove tombstones?
   - Must consider nested CRDT state
   - Causal stability across the hierarchy

### Nested CRDT Trait

```rust
pub trait NestedCrdt: Clone {
    type Op;
    type Version;
    
    fn apply(&mut self, op: Self::Op);
    fn merge(&mut self, other: &Self);
    fn version(&self) -> Self::Version;
    
    // For outer CRDT to propagate operations
    fn pending_ops(&self) -> Vec<Self::Op>;
    fn acknowledge(&mut self, version: Self::Version);
}
```

### Architecture Options

**Option A: Transparent nesting**
- Inner CRDTs are unaware they're nested
- Outer CRDT handles all coordination
- Simpler inner CRDT implementation
- More complex outer CRDT

**Option B: Aware nesting**
- Inner CRDTs know their parent ID
- Can generate globally unique operation IDs
- More complex inner CRDT
- Simpler coordination

**Option C: Operation log approach**
- All operations go to a single log
- Path-based addressing: `["paragraphs", 3, "text", 15]`
- Similar to Loro's approach
- Good for time travel and undo

### Recommended Approach

Start with Option C (operation log) because:
1. Aligns with DESIGN.md's log-based architecture
2. Enables time travel naturally
3. Single source of truth for all operations
4. Proven by Loro

## Implementation Phases

### Phase 1: Generic items (simpler)

1. Parameterize `Rga<T>` over item type
2. Update storage from `Vec<u8>` to `Vec<T>`
3. Adjust span coalescing for item counts
4. Update tests for generic items
5. Benchmark performance impact

### Phase 2: Simple nested CRDTs

1. Define `NestedCrdt` trait
2. Implement for a simple CRDT (e.g., LWWRegister)
3. Create `Rga<LWWRegister<String>>` as proof of concept
4. Test merge and conflict resolution

### Phase 3: Recursive nesting

1. Implement `NestedCrdt` for `Rga<T>`
2. Create `Rga<Rga<char>>` (list of strings)
3. Handle version vector propagation
4. Test deep nesting

### Phase 4: Operation log integration

1. Design path-based operation addressing
2. Integrate with log from DESIGN.md
3. Implement time travel for nested structures
4. Add undo/redo support

## Open Questions

1. **Performance**: How much overhead does generic dispatch add?
   - Measure with monomorphization vs dynamic dispatch
   - Consider `#[inline]` on hot paths

2. **Memory**: How to handle large nested CRDTs?
   - Lazy loading?
   - Streaming merge?

3. **Garbage collection**: When can nested tombstones be removed?
   - Need causal stability across all replicas
   - May require protocol coordination

4. **Undo**: How does undo work with nested CRDTs?
   - Undo outer operation (remove paragraph)?
   - Undo inner operation (remove character)?
   - Both?

5. **Rich text**: Is nested CRDT the right model for rich text?
   - Peritext uses different approach (marks)
   - Could combine: `Rga<char>` + separate mark CRDT

## Related Work

- **Loro**: Rich nested CRDT with operation log
- **Yjs**: Nested types with shared doc coordination
- **Automerge**: Nested objects with JSON-like structure
- **Peritext**: Rich text without nesting (marks instead)

## Next Steps

1. Prototype `Rga<T>` with simple generic items
2. Benchmark against `Rga<u8>`
3. If acceptable overhead, proceed to nested CRDTs
4. Design operation log format for nested operations
