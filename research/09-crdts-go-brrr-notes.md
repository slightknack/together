+++
model = "claude-opus-4-5"
created = 2026-01-31
modified = 2026-01-31
driver = "Isaac Clayton"
+++

# Notes: CRDTs Go Brrr (josephg.com)

Source: https://josephg.com/blog/crdts-go-brrr/

## Core Insight

Academic benchmarks showed CRDTs were slow, but this reflected poor implementation, not algorithmic limits. Same algorithm in JS vs C showed 200x performance difference.

Key distinction: **behavior** (how concurrent edits merge) vs **implementation** (data structures and optimizations). Papers describe behavior but implementations vary wildly.

## Performance Journey on automerge-paper trace (260k edits)

| Implementation | Time | RAM | Speedup |
|---------------|------|-----|---------|
| Automerge | 291s | 880MB | baseline |
| reference-crdts | 31s | 28MB | 10x |
| Yjs | 0.97s | 3.3MB | 300x |
| Diamond (Rust) | 0.056s | 1.1MB | 5200x |

For reference: plain JS string editing took 0.61s - Diamond beats naive native!

## Key Optimizations

### 1. Flat List vs Tree
Automerge used tree structure. reference-crdts flattened to array with insertion sort. 10x faster, 30x less memory.

### 2. Run-Length Encoding (RLE)
Yjs combines consecutive typed characters into single items. Typing "hello" = 1 item, not 5.
- 14x reduction in array size on benchmark
- This is why span coalescing helps us

### 3. Cursor Caching
"Humans don't bounce around documents randomly" - cache last edit position for O(1) sequential access.

### 4. B-Tree / Range Tree
Diamond uses B-tree with character counts at internal nodes.
- O(log n) position lookup: ~3 memory reads vs scanning 75k items
- Tight memory packing in 32-entry blocks
- Contiguous memory = CPU cache friendly

### 5. Memory Layout
JavaScript objects scatter data via pointers = random memory access.
Rust packs data contiguously = cache-line efficient.

"A single main-memory read takes about 100ns. At human scale, that's like a 2-minute delay."

## What Diamond Does

1. **Range tree** for position indexing (like B-tree with counts at nodes)
2. **RLE** for consecutive insertions
3. **Rust** for memory layout control and no GC
4. **Cursor caching** for sequential access

## What We're Missing

Comparing to our implementation:
- We have RLE via span coalescing (79-91% coalesce rate) ✓
- We have chunking for cache locality ✓
- We DON'T have O(log n) position lookup - we have O(sqrt n) chunk scan
- We DON'T have tight memory layout - Span is 112 bytes

## Key Quotes

"I was reading papers which described the behaviour of different systems and incorrectly assumed those papers defined optimal implementation approaches."

"The list representation is 10x faster and uses 30x less memory than the tree representation."

"We could probably make this another 10x faster by switching from javascript to rust."

## Relevance to Our Work

1. **B-tree/range tree is the key** - This is what gives Diamond O(log n) lookup
2. **Memory layout matters** - Our 112-byte Span hurts cache utilization  
3. **RLE is already helping** - Our span coalescing is the RLE equivalent
4. **Skip list is similar to B-tree** - Both give O(log n) with good cache behavior if nodes are chunked

The skip list we have is essentially a probabilistic B-tree. The question is adapting it for weighted (character count) lookup instead of index lookup.
