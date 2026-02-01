// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

# CRDT Comparison Study Progress

This document tracks progress on the comprehensive CRDT library comparison study.

Libraries are researched and implemented serially (not in parallel) so learnings compound.

## Phase 1: Setup and Infrastructure

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

## Phase 2: Research and Implementation (Serial)

Order: yjs -> diamond-types -> cola -> json-joy -> loro

| Library | Research | Implementation | Notes |
|---------|----------|----------------|-------|
| yjs | in progress | not started | YATA algorithm, foundational |
| diamond-types | pending | pending | Clear Rust reference |
| cola | pending | pending | Novel approach |
| json-joy | pending | pending | Advanced optimizations |
| loro | pending | pending | Most complex |

## Phase 3: Implementation

| Library | Status | Implementation | Conformance Tests |
|---------|--------|----------------|-------------------|
| diamond-types | not started | - | - |
| loro | not started | - | - |
| cola | not started | - | - |
| json-joy | not started | - | - |
| yjs | not started | - | - |

## Phase 4: Comparative Analysis

- [ ] Benchmark suite created
- [ ] All implementations benchmarked
- [ ] Comparison document written

## Phase 5: Synthesis

- [ ] Best techniques identified
- [ ] Hybrid implementation created
- [ ] Beats diamond-types on all benchmarks

## Phase 6: Log Integration

- [ ] Log-compatible interface designed
- [ ] Final validation complete

## Learnings Summary

(To be updated as work progresses)

### Key Data Structures

- B-tree weighted list (our current approach)
- Skip lists with gap buffers (diamond-types)
- Splay trees (json-joy)
- Rope structures

### Key Optimizations

- Cursor caching for sequential access
- Span coalescing for memory efficiency
- Buffered writes for batching
- ID indexing for fast lookup

### Key Algorithms

- YATA ordering (Yjs)
- FugueMax (theoretical improvement over YATA)
- RGA (basic ordering)
