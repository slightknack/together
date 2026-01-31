---
model = "claude-opus-4-5"
created = "2026-01-31"
modified = "2026-01-31"
driver = "Isaac Clayton"
---

# Optimization Plan: Beating Diamond-Types

## Current State

| Trace | Together | Diamond-types | Ratio | Patches |
|-------|----------|---------------|-------|---------|
| sveltecomponent | 2.59ms | 1.15ms | 2.3x slower | 19,749 |
| rustcode | 6.53ms | 2.72ms | 2.4x slower | 40,173 |
| seph-blog1 | 27.0ms | 6.57ms | 4.1x slower | 137,993 |

Goal: Faster than diamond-types on 3/4 benchmarks, no worse than 2x on any.

## Root Cause Analysis

### Why seph-blog1 is 4.1x slower (vs 2.3x for sveltecomponent)

seph-blog1 has 137k patches vs 20k for sveltecomponent. Our O(sqrt(n)) chunk scanning becomes:
- sveltecomponent: ~140 chunks, ~70 chunks scanned per lookup
- seph-blog1: ~1,000+ chunks, ~500 chunks scanned per lookup

Diamond-types uses O(log n) lookup via B-tree/skip list, so it scales better.

### Key Performance Gaps

1. **Position lookup**: O(sqrt n) vs O(log n) - explains 4.1x on large traces
2. **Span size**: 112 bytes vs 40 bytes - 2.8x more cache traffic
3. **No gap buffer**: Every insert requires list operations
4. **No cursor caching**: No amortization of sequential edits
5. **No buffered writes**: Each edit applied immediately

## Prioritized Optimization List (Ordered by Foundation)

### 1. Compact Span Structure (Foundation)
**Impact**: 1.2-1.5x speedup, enables other optimizations
**Risk**: Low

Current Span is 112 bytes:
- `user: KeyPub` = 32 bytes (could be u16 index = 2 bytes)
- `origin: Option<ItemId>` = 48 bytes (could be u32 span_idx + u32 offset = 8 bytes)
- `seq: u64` = 8 bytes
- `len: u64` = 8 bytes
- `content_offset: usize` = 8 bytes
- `deleted: bool` = 1 byte + 7 padding

Target: 24-32 bytes per span (fits in half a cache line).

### 2. Fenwick Tree for Chunk Weights (High Impact for Large Traces)
**Impact**: 2-3x speedup on seph-blog1
**Risk**: Medium

Add Fenwick tree over chunk weights for O(log n) chunk lookup.
Current chunk scan is O(n_chunks). With Fenwick tree:
- `find_chunk_by_weight`: O(log chunks) via Fenwick prefix query
- `update_weight`: O(log chunks) via Fenwick point update

### 3. Cursor Caching
**Impact**: 1.5-2x speedup on sequential editing patterns
**Risk**: Low

Cache the last lookup position. For sequential typing:
- If new position is adjacent to cached position, skip search
- JumpRopeBuf shows 10x improvement for sequential patterns

### 4. B-Tree or Skip List for Spans
**Impact**: 2-3x overall speedup
**Risk**: High (major refactor)

Replace chunked WeightedList with a proper O(log n) structure.
Options:
- Adapt existing skip_list.rs to track weights instead of counts
- Implement B-tree like diamond-types ContentTree
- Use arena-allocated nodes for cache locality

### 5. Gap Buffer for Content Storage
**Impact**: O(1) sequential inserts
**Risk**: Medium

Instead of storing content in per-user columns, use gap buffers.
JumpRope uses 392-byte gap buffers per skip list node.
Sequential typing becomes O(1) instead of O(chunk size).

### 6. Separate Text Storage (JumpRope-style)
**Impact**: Specialized optimization for text operations
**Risk**: High (architectural change)

Diamond-types separates:
- JumpRope for text content (optimized for editing)
- ContentTree for CRDT spans (optimized for merge)

This allows each structure to be optimized for its purpose.

### 7. Buffered Writes (JumpRopeBuf-style)
**Impact**: 10x for sequential editing patterns
**Risk**: Low

Buffer consecutive operations before applying:
- Adjacent inserts: merge content
- Adjacent deletes: merge range
- Flush on position change or explicit sync

### 8. SIMD/Vectorized Operations
**Impact**: 1.2-1.5x for scanning operations
**Risk**: Medium

Use SIMD for:
- Chunk weight scanning
- Content comparison
- UTF-8 character counting

## Implementation Order

The optimizations should be done in this order (most foundational first):

1. **Compact Span** - Reduces memory traffic, makes everything else faster
2. **Fenwick Tree** - O(log n) chunk lookup, biggest win for large traces
3. **Cursor Caching** - Quick win, low risk
4. **B-Tree/Skip List for Spans** - Major architectural improvement
5. **Gap Buffer** - O(1) sequential inserts
6. **Buffered Writes** - Amortize many operations
7. **Separate Text Storage** - Final optimization
8. **SIMD** - Polish

## Expected Outcomes

After implementing optimizations 1-4:
- sveltecomponent: 1.0-1.5x (target: <1.15ms)
- rustcode: 1.0-1.5x (target: <2.72ms)
- seph-blog1: 1.0-1.5x (target: <6.57ms)

The goal is achievable with optimizations 1-4. Optimizations 5-8 would push us ahead.

## Measurement Strategy

For each optimization:
1. Run benchmarks before
2. Implement
3. Run benchmarks after
4. If faster: commit to master
5. If slower: commit to branch `ibc/slow-<name>`, document lessons, revert
