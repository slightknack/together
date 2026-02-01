+++
model = "claude-opus-4-5"
created = 2026-02-01
modified = 2026-02-01
driver = "Isaac Clayton"
+++

# CRDT Implementation Comparison Results

This document presents benchmark results and analysis comparing five RGA (Replicated Growable Array) implementations:

1. **YjsRga**: YATA algorithm (yjs-style) with Vec-based storage
2. **DiamondRga**: B-tree based with span coalescing (diamond-types style)
3. **ColaRga**: Anchor-based with Lamport timestamps
4. **JsonJoyRga**: Dual-tree with splay optimization
5. **LoroRga**: Fugue algorithm with B-tree storage

## Benchmark Results

### Small Scale (100 character operations)

All times in microseconds (us):

| Implementation | Seq Forward | Random Insert | Random Delete | Merge |
|----------------|-------------|---------------|---------------|-------|
| YjsRga         | 31          | 46            | 53            | 1     |
| DiamondRga     | 28          | 29            | 17            | 1     |
| ColaRga        | 20          | 162           | 12            | 1     |
| JsonJoyRga     | 42          | 54            | 21            | 0     |
| LoroRga        | 16          | 21            | 12            | 1     |

### Medium Scale (1000 character operations)

All times in microseconds (us):

| Implementation | Seq Forward | Random Insert | Random Delete | Merge |
|----------------|-------------|---------------|---------------|-------|
| YjsRga         | 1412        | 2572          | 1138          | 0     |
| DiamondRga     | 940         | 2311          | 108           | 1     |
| ColaRga        | 933         | (slow)        | 109           | 1     |
| JsonJoyRga     | 3572        | 3511          | 217           | 1     |
| LoroRga        | 935         | 2460          | 107           | 1     |

### Large Scale (Criterion benchmarks, 10000 chars)

From criterion detailed benchmarks:

| Implementation | Seq Forward (ms) | Throughput (Kelem/s) |
|----------------|------------------|----------------------|
| YjsRga         | 147.4            | 67.8                 |
| DiamondRga     | 83.9             | 119.2                |
| ColaRga        | 84.3             | 118.6                |
| JsonJoyRga     | 361.7            | 27.6                 |
| LoroRga        | 83.7             | 119.4                |

### Span Count (Fragmentation Measure)

After 1000 operations:

| Implementation | Seq Insert | Random Insert | After Merge |
|----------------|------------|---------------|-------------|
| YjsRga         | 1000       | 1000          | 2           |
| DiamondRga     | 1000       | 1000          | 2           |
| ColaRga        | 1000       | (slow)        | 2           |
| JsonJoyRga     | 1000       | 1000          | 2           |
| LoroRga        | 1000       | 1000          | 2           |

Note: None of these implementations currently coalesce spans during character-by-character insertion. The span count equals the number of individual insert operations. Merge results in 2 spans because each user's bulk insert creates one span.

## Tradeoff Analysis

### Where Each Approach Excels

**LoroRga (Fugue algorithm)**
- Best overall balance of performance
- Fastest at small-scale operations (16us for 100 sequential inserts)
- Excellent random insert performance (21us for 100 random inserts)
- Clean separation of concerns with Fugue's dual-origin approach
- Prevents interleaving of concurrent text passages

**DiamondRga (B-tree based)**
- Excellent delete performance (108us for 1000 random deletes vs 1138us for YjsRga)
- Good sequential performance at scale
- B-tree structure provides O(log n) position lookups
- Separate content storage reduces memory churn

**ColaRga (Anchor-based)**
- Simplest algorithm conceptually (single anchor, not dual origins)
- Fastest at small-scale deletes (12us for 100 deletes)
- Good sequential insert performance (933us for 1000 inserts)
- Timestamp-based ordering is intuitive

**YjsRga (YATA algorithm)**
- Most widely deployed and battle-tested algorithm
- Proven correctness in production systems
- Good merge performance
- Simple implementation using Vec

**JsonJoyRga (Dual-tree with splay)**
- Dual-tree indexing enables O(log n) lookups by both position and ID
- Splay tree optimization benefits repeated access patterns
- Good for workloads with temporal locality

### Where Each Approach Struggles

**YjsRga**
- Delete operations are O(n) due to linear scan for position lookup
- 10x slower on random deletes compared to B-tree implementations
- No span coalescing leads to fragmentation

**DiamondRga**
- Slightly more complex implementation than LoroRga
- Random insert performance could be better

**ColaRga**
- Severe O(n^2) degradation on random inserts (>100x slower than others)
- The anchor_precedes check during conflict resolution causes linear scans
- Not suitable for random-access editing patterns

**JsonJoyRga**
- Slowest overall performance (3.5ms for 1000 sequential inserts)
- Splay tree overhead outweighs benefits for these workloads
- Complex implementation with two tree structures to maintain

**LoroRga**
- Slightly more complex than ColaRga (dual origins vs single anchor)
- Fugue algorithm requires careful implementation to prevent interleaving

## Common Patterns Across Libraries

### 1. Separate Content Storage
All implementations (except basic YjsRga) separate CRDT metadata from content:
- User content stored in per-user append-only buffers
- Spans reference content by (user_idx, offset, len)
- Reduces memory duplication during merges

### 2. User Index Tables
All implementations map KeyPub (32 bytes) to compact indices (2 bytes):
- FxHashMap for O(1) lookup
- Vec for O(1) reverse lookup
- Reduces span memory footprint significantly

### 3. Lamport Clocks
All implementations use Lamport timestamps for ordering:
- Monotonically increasing per operation
- Merged by taking maximum during sync
- Provides total ordering for conflict resolution

### 4. Dual Origins (YATA/Fugue)
YjsRga, DiamondRga, JsonJoyRga, and LoroRga all use dual origins:
- left_origin: Character inserted after
- right_origin: Character that was to the right at insertion time
- Prevents interleaving of concurrent passages
- ColaRga uses single anchor but sacrifices random insert performance

### 5. B-tree Based Storage
DiamondRga, ColaRga, and LoroRga all use weighted B-trees:
- O(log n) position lookup via weight-based navigation
- O(log n) insertion and deletion
- Better cache locality than linked structures

## Novel Techniques Worth Adopting

### From Diamond-Types
1. **JumpRope structure**: Gap buffers within B-tree leaves for efficient local edits
2. **Cursor caching**: Amortize sequential access to O(1) per operation
3. **Run-length encoding**: Coalesce consecutive same-user insertions

### From Json-Joy
1. **Dual-tree indexing**: O(log n) lookups by both position and ID
2. **Splay tree optimization**: Exploit temporal locality in editing patterns
3. **Chunk-based ID indexing**: Map (user, seq) directly to chunk indices

### From Loro
1. **Fugue algorithm**: Cleaner anti-interleaving semantics than YATA
2. **Delete counters**: Space-efficient alternative to tombstones
3. **Rich type system**: Support for nested structures beyond text

### From Cola
1. **Single anchor simplicity**: Easier to reason about (when random insert is rare)
2. **Timestamp-primary ordering**: Intuitive conflict resolution

## Recommended Synthesis Approach

Based on the benchmark results and analysis, the optimal synthesis would combine:

### Core Architecture
1. **B-tree weighted list** (from DiamondRga/LoroRga)
   - O(log n) position lookups
   - Efficient weight-based navigation
   - Good cache locality

2. **Fugue algorithm** (from LoroRga)
   - Dual origins for anti-interleaving
   - Cleaner semantics than YATA
   - Proven correctness

3. **Separate content storage** (from DiamondRga)
   - Per-user append-only buffers
   - Spans reference by offset
   - Efficient merging

### Optimizations to Add

1. **Cursor caching** (from Diamond-Types)
   - Cache last lookup position
   - Amortize sequential access to O(1)
   - Invalidate on non-local edits

2. **Span coalescing** (from Diamond-Types)
   - Merge consecutive same-user spans
   - Reduce fragmentation
   - Improve cache locality

3. **ID index** (from JsonJoy)
   - HashMap from (user_idx, seq) to span index
   - O(1) lookup during merge
   - Update incrementally on insert/delete

### Performance Targets

Based on benchmarks, a well-optimized implementation should achieve:

- Sequential insert: <1ms per 1000 chars (currently: 935us for LoroRga)
- Random insert: <2ms per 1000 chars (currently: 2311us for DiamondRga)
- Random delete: <100us per 1000 chars (currently: 107us for LoroRga)
- Merge: <1us for bulk document merge (currently achieved)

### Implementation Priority

1. First: Ensure correctness with Fugue algorithm
2. Second: Add B-tree weighted list for O(log n) operations
3. Third: Implement cursor caching for sequential typing
4. Fourth: Add span coalescing to reduce fragmentation
5. Fifth: Add ID index for O(1) merge lookups

## Conclusion

The benchmark results clearly show that:

1. **LoroRga** (Fugue with B-tree) provides the best balance of performance and correctness
2. **ColaRga**'s single-anchor approach causes severe degradation on random inserts
3. **JsonJoyRga**'s splay tree adds overhead that outweighs benefits for typical editing
4. **YjsRga**'s Vec-based storage causes O(n) delete operations

The recommended approach is to build on LoroRga's foundation, adding cursor caching and span coalescing from Diamond-Types to achieve optimal performance across all operation types.
