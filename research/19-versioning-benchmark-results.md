# Versioning Implementation Benchmark Results

## Summary

Three versioning approaches were implemented and benchmarked:
1. **Logical** (ibc/document-logical): Lamport timestamps on spans, filter at read time
2. **Persistent** (ibc/document-persistent): Arc-based snapshots, copy spans on version()
3. **Checkpoint** (ibc/document-checkpoint): Checkpoints with geometric retention policy

## Core CRDT Performance (Trace Benchmarks)

All three approaches have nearly identical core performance:

| Trace | Baseline | Logical | Persistent | Checkpoint |
|-------|----------|---------|------------|------------|
| sveltecomponent (19K ops) | 1.7ms | 1.8ms | 1.7ms | 1.7ms |
| rustcode (40K ops) | 3.5ms | 3.7ms | 3.5ms | 3.5ms |
| seph-blog1 (138K ops) | 5.9ms | 6.0ms | 6.1ms | 5.6ms |
| automerge-paper (260K ops) | 6.0ms | 5.9ms | 6.1ms | 5.3ms |

**Conclusion**: No significant difference in core insert/delete performance.

## Version API Performance

Test: 10,000 sequential inserts, then measure version operations.

| Metric | Logical | Persistent | Checkpoint |
|--------|---------|------------|------------|
| **Span count** | 10,000 | 1 | 1 |
| **version()** | 3ns | 91ns | 330ns |
| **to_string_at()** | 54.9µs | 49.5µs | 40.4µs |
| **slice_at(0,1000)** | 16.9µs | 551ns | 522ns |
| **len_at()** | 15.0µs | 0ns | 0ns |

### Analysis

**Logical Versioning**
- Pros: version() is O(1), no memory overhead per version
- Cons: 
  - **Disables span coalescing** (10,000 spans vs 1 for same content)
  - Read operations are O(n) due to timestamp filtering
  - Significant memory overhead from span explosion
  - Span size increased 24→32 bytes

**Persistent Versioning**
- Pros: 
  - Preserves coalescing (1 span)
  - Fast reads (direct access to snapshot)
  - len_at() is O(1)
- Cons:
  - version() is O(n) to clone spans
  - Each version stores full span list (memory grows with versions)

**Checkpoint Versioning**
- Pros:
  - Preserves coalescing (1 span)
  - Fast reads (direct access to checkpoint)
  - len_at() is O(1)
  - Geometric retention bounds memory to O(log n) checkpoints
- Cons:
  - version() is O(n) + retention logic overhead
  - Slight overhead from Arc indirection

## Recommendation

**Use Persistent Versioning** for these reasons:

1. **Preserves coalescing**: Critical for memory efficiency
2. **Simple implementation**: No complex retention policy needed
3. **Fast reads**: O(1) len_at(), fast slice_at()
4. **Good balance**: version() cost is acceptable for explicit version creation

The Checkpoint approach adds complexity for marginal benefit (geometric retention), and Logical approach has unacceptable span explosion.

If memory-bounded versioning is needed in the future, Checkpoint's retention policy can be added to Persistent (they're structurally similar - both use Arc<Vec<Span>> snapshots).
