+++
model = "claude-opus-4-5"
created = 2026-01-31
modified = 2026-01-31
driver = "Isaac Clayton"
+++

# Optimization Phase 2: Beat Diamond-Types on All Benchmarks

## Current State (2026-01-31)

| Trace | Patches | Together | Diamond-types | Ratio |
|-------|---------|----------|---------------|-------|
| sveltecomponent | 19,749 | 1.94ms | 1.77ms | 1.1x slower |
| rustcode | 40,173 | 4.10ms | 4.25ms | 0.97x (3% faster) |
| seph-blog1 | 137,993 | 8.40ms | 9.63ms | 0.87x (13% faster) |
| automerge-paper | 259,778 | ~19ms | ~11ms | 1.7x slower |

Goal: Beat diamond-types on ALL 4 benchmarks.

## Architectural Goal: Separate CRDT from Text

Diamond-types succeeds by separating concerns:
- **JumpRope**: Fast text storage with gap buffers and skip list
- **ContentTree**: B-tree for CRDT span ordering

Our current Rga mixes these concerns. The refactor:
- **Rope** (src/crdt/rope.rs): Pure text storage, O(log n) operations
- **Rga** (src/crdt/rga.rs): CRDT ordering, references rope for content

## Prioritized Optimization List

### Phase 2A: Close the Gap on Small Traces (sveltecomponent)

1. **Rope Integration**
   - Use existing rope.rs for text storage instead of inline content
   - Spans reference rope positions instead of storing content_offset
   - Expected gain: Better cache locality for text operations

2. **Gap Buffer in Rope Nodes**
   - Add gap buffer to rope nodes (like JumpRope's 392-byte buffers)
   - Sequential typing becomes O(1) within a node
   - Expected gain: 1.3-1.5x for sequential patterns

3. **Delete Buffering in RgaBuf**
   - Buffer adjacent deletes, not just inserts
   - Merge backspace sequences before applying
   - Expected gain: 1.1-1.2x on delete-heavy traces

### Phase 2B: Close the Gap on Large Traces (automerge-paper)

4. **B-Tree for Spans**
   - Replace WeightedList with a proper B-tree
   - Each node stores cumulative weights for O(log n) lookup
   - Expected gain: 1.5-2x on automerge-paper

5. **Dual-Width B-Tree**
   - Track both visible length and total length per subtree
   - Enables O(log n) for both editing (visible) and CRDT ops (total)
   - Diamond-types calls this LenPair { cur, end }

6. **Arena Allocation for Spans**
   - Allocate spans from a typed arena
   - Stable indices, no reallocation during inserts
   - Expected gain: 1.1x from reduced allocator pressure

### Phase 2C: Final Polish

7. **SIMD Chunk Scanning**
   - Use SIMD to scan weights within B-tree leaves
   - Vectorized comparison for 4-8 weights at once
   - Expected gain: 1.1-1.2x for within-node operations

8. **Prefetch Hints**
   - Add prefetch instructions before B-tree descent
   - Hide memory latency during tree traversal
   - Expected gain: 1.05-1.1x

## Implementation Order

The order matters because each optimization builds on previous ones:

1. **Rope Integration** - Foundation for content/CRDT separation
2. **Gap Buffer** - Makes rope competitive with JumpRope
3. **B-Tree for Spans** - Core algorithmic improvement
4. **Dual-Width Tracking** - Enables efficient CRDT operations
5. **Delete Buffering** - Builds on B-tree for batched updates
6. **Arena Allocation** - Optimization of B-tree storage
7. **SIMD** - Micro-optimization of B-tree leaves
8. **Prefetch** - Final memory optimization

## Key Architectural Decisions

### Content Storage

Current:
```rust
struct Span {
    content_offset: u32,  // offset into user's column
    // ...
}
struct Rga {
    columns: HashMap<u16, Vec<u8>>,  // per-user content
}
```

Proposed:
```rust
struct Span {
    rope_start: u32,  // position in shared rope
    rope_len: u32,    // length in rope (may differ from visible len if deleted)
}
struct Document {
    rope: Rope,       // shared text storage
    crdt: Rga,        // CRDT ordering
}
```

### Span Storage

Current (WeightedList with Fenwick):
- O(log n) chunk lookup via Fenwick
- O(sqrt n) within-chunk operations
- Total: O(sqrt n) for large n

Proposed (B-tree):
- O(log n) at every level
- 16-32 children per node for cache efficiency
- Total: O(log n)

## Measurement Strategy

For each optimization:
1. Profile with `cargo flamegraph` to identify bottleneck
2. Implement optimization
3. Run all 4 benchmarks
4. If all faster or neutral: commit
5. If any regresses: investigate or revert

## Success Criteria

Beat diamond-types on all 4 traces:
- sveltecomponent: < 1.77ms
- rustcode: < 4.25ms (already achieved)
- seph-blog1: < 9.63ms (already achieved)
- automerge-paper: < 11ms (currently 19ms, need 1.7x improvement)

The automerge-paper trace is the main challenge. The B-tree optimization is essential.
