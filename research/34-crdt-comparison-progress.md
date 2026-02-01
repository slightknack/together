// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

# CRDT Comparison Study Progress

This document tracks progress on the comprehensive CRDT library comparison study.

Libraries were researched and implemented serially (not in parallel) so learnings compound.

## Phase 1: Setup and Infrastructure ✓

- [x] Branch created: `crdt-comparison-study`
- [x] Rga trait defined: `src/crdt/rga_trait.rs`
- [x] Primitives library structure: `src/crdt/primitives/`
  - [x] `clock.rs` - LamportClock, VectorClock
  - [x] `id.rs` - OpId, ItemId, CompactOpId, UserIdx
  - [x] `span.rs` - CompactSpan with origins and split/coalesce
  - [x] `cursor.rs` - CursorCache for sequential access optimization
  - [x] `range_tree.rs` - Aggregate traits for range queries
  - [x] `user_table.rs` - User ID to index mapping
- [x] Conformance test infrastructure: `tests/rga_conformance.rs`

## Phase 2: Research and Implementation (Serial) ✓

Order: yjs -> diamond-types -> cola -> json-joy -> loro

| Library | Research | Implementation | Notes |
|---------|----------|----------------|-------|
| yjs | ✓ `research/crdt-yjs.md` | ✓ `src/crdt/yjs.rs` | YATA algorithm with dual origins |
| diamond-types | ✓ `research/crdt-diamond-types.md` | ✓ `src/crdt/diamond.rs` | B-tree based, JumpRope structure |
| cola | ✓ `research/crdt-cola.md` | ✓ `src/crdt/cola.rs` | Anchor-based positioning |
| json-joy | ✓ `research/crdt-json-joy.md` | ✓ `src/crdt/json_joy.rs` | Dual-tree indexing, splay trees |
| loro | ✓ `research/crdt-loro.md` | ✓ `src/crdt/loro.rs` | Fugue algorithm, best anti-interleaving |

## Phase 3: Implementation Status ✓

| Library | Status | Implementation | Conformance Tests |
|---------|--------|----------------|-------------------|
| yjs | ✓ Complete | `YjsRga` | 26/26 pass |
| diamond-types | ✓ Complete | `DiamondRga` | 26/26 pass |
| cola | ✓ Complete | `ColaRga` | 26/26 pass |
| json-joy | ✓ Complete | `JsonJoyRga` | 26/26 pass |
| loro | ✓ Complete | `LoroRga` | 26/26 pass |
| **optimized** | ✓ Complete | `OptimizedRga` | 26/26 pass |

**Total: 156 conformance tests pass**

## Phase 4: Comparative Analysis ✓

- [x] Benchmark suite created: `benches/rga_comparison.rs`, `benches/rga_quick.rs`
- [x] All implementations benchmarked
- [x] Comparison document: `research/crdt-comparison-results.md`

### Key Findings

| Implementation | Best At | Struggles With |
|----------------|---------|----------------|
| LoroRga | Best overall balance, Fugue algorithm | Slightly more complex |
| DiamondRga | Excellent delete perf (10x faster) | Random insert performance |
| ColaRga | Simple algorithm, fast deletes | O(n²) on random inserts |
| JsonJoyRga | Dual-tree indexing concept | Splay overhead slows overall |
| YjsRga | Battle-tested, simple | O(n) deletes, no coalescing |

## Phase 5: Synthesis ✓

- [x] Best techniques identified
- [x] Hybrid implementation created: `src/crdt/rga_optimized.rs`
- [x] Matches/beats LoroRga (previous best) on all benchmarks

**OptimizedRga combines:**
- Fugue algorithm (from loro) - best anti-interleaving
- B-tree weighted storage (from diamond/loro) - O(log n) position lookups
- Separate content storage per user - efficient merges
- Compact 16-bit user indices - reduced memory overhead

## Phase 6: Log Integration ✓

- [x] Log-compatible interface designed: `src/crdt/log_integration.rs`
- [x] OpLog trait with export/rebuild/apply operations
- [x] Binary encoding for network/storage
- [x] Design document: `research/crdt-log-integration.md`
- [x] 26 tests for round-trip, order independence, determinism
- [x] Final validation complete: 235 library tests pass

## Final Statistics

- **235 library tests** pass
- **156 conformance tests** pass (26 × 6 implementations)
- **5 research documents** created (~3000 lines total)
- **6 RGA implementations** (yjs, diamond, cola, json_joy, loro, optimized)
- **2 benchmark files** (criterion + quick comparison)

## Learnings Summary

### Key Data Structures

- **B-tree weighted list** - O(log n) position lookups via cached aggregate weights
- **Separate content buffers** - Per-user append-only storage, spans reference offsets
- **Splay trees** - Good for temporal locality but overhead often exceeds benefit
- **Dual-tree indexing** - Position tree + ID tree enables O(log n) for both lookups

### Key Optimizations

- **Cursor caching** - Amortizes sequential access to O(1) per operation
- **Span coalescing** - Merges consecutive operations from same user
- **Compact user IDs** - 16-bit indices instead of full public keys
- **ID index HashMap** - O(1) lookup during merge operations

### Key Algorithms

- **Fugue** (loro) - Best conflict resolution, prevents character interleaving
- **YATA** (yjs) - Original dual-origin approach, proven in production
- **Anchor-based** (cola) - Simpler single-anchor but less robust

### Recommended Architecture

For new implementations, use:
1. **Core**: B-tree with weight caching for O(log n) position lookups
2. **Conflict resolution**: Fugue algorithm with dual origins
3. **Content**: Per-user separate buffers, spans reference offsets
4. **Optimization**: Cursor cache for sequential access patterns
5. **Merge**: ID index (HashMap) for O(1) existing item lookup
