+++
model = "claude-opus-4-5"
created = 2026-02-01
modified = 2026-02-01
driver = "Isaac Clayton"
+++

# Worklog: CRDT Comparison Study

## Goal

Research 5 major CRDT libraries, implement each approach from scratch, benchmark, and synthesize the best techniques into an optimized implementation.

## Procedure

Followed `procedures/03-crdt-comparison-study.md`. Libraries researched and implemented serially in order: yjs, diamond-types, cola, json-joy, loro.

## Phase 1: Setup and Infrastructure

Created branch `crdt-comparison-study` with:

- `src/crdt/rga_trait.rs` - Common `Rga` trait all implementations must satisfy
- `src/crdt/primitives/` - Shared building blocks:
  - `clock.rs` - LamportClock, VectorClock
  - `id.rs` - OpId, ItemId, CompactOpId, UserIdx
  - `span.rs` - CompactSpan with origins
  - `cursor.rs` - CursorCache for sequential access
  - `range_tree.rs` - Aggregate traits
  - `user_table.rs` - User ID to index mapping
- `tests/rga_conformance.rs` - 26 conformance tests per implementation

## Phase 2: Research and Implementation

### yjs (YATA algorithm)

Research: `research/35-crdt-yjs.md` (643 lines)

Key insights:
- Dual-origin approach: each item stores left_origin and right_origin
- Conflict resolution: compare left origins, then right origins, then peer ID
- Items form implicit tree structure via origin references
- Battle-tested in production (Google Docs, Notion, etc.)

Implementation: `src/crdt/yjs.rs`
- Linked list of Items with YATA ordering
- O(n) position lookup, O(n) merge
- Simple but not optimized for large documents

### diamond-types (B-tree + YATA)

Research: `research/36-crdt-diamond-types.md` (508 lines)

Key insights:
- JumpRope: skip list with 392-byte gap buffers
- Separate content storage in per-user buffers
- Cursor-based access for sequential operations
- Run-length encoding for consecutive operations

Implementation: `src/crdt/diamond.rs`
- B-tree weighted list for O(log n) position lookups
- Separate content buffers (spans reference offsets)
- Span coalescing for memory efficiency

### cola (Anchor-based)

Research: `research/37-crdt-cola.md` (578 lines)

Key insights:
- Single anchor instead of dual origins
- Simpler conflict resolution: Lamport timestamp + peer ID
- "Replaying" model: anchor is position at time of insert
- Minimal implementation, easy to understand

Implementation: `src/crdt/cola.rs`
- Anchor-based positioning
- Descending Lamport, ascending user for ordering
- Simple but O(n^2) on random inserts (no tree structure)

### json-joy (Dual-tree indexing)

Research: `research/38-crdt-json-joy.md` (600 lines)

Key insights:
- Two tree structures: position tree + ID tree
- Splay tree for temporal locality (recently accessed near root)
- YATA-compatible conflict resolution
- Written in TypeScript, targets browser environments

Implementation: `src/crdt/json_joy.rs`
- Dual-tree: splay tree for position, BST for ID
- Splay operations exploit temporal locality
- O(log n) amortized for sequential access

### loro (Fugue algorithm)

Research: `research/39-crdt-loro.md` (600 lines)

Key insights:
- Fugue algorithm from "The Art of the Fugue" paper
- Prevents character interleaving in concurrent edits
- B-tree with rich metadata caching
- Separates OpLog (history) from DocState (current state)

Implementation: `src/crdt/loro.rs`
- Fugue conflict resolution with dual origins
- B-tree weighted storage
- Best anti-interleaving properties

## Phase 4: Benchmarking and Analysis

Created `benches/rga_comparison.rs` (Criterion) and `benches/rga_quick.rs` (quick comparison).

Benchmarks:
- Sequential forward typing (100, 1000 ops)
- Sequential backward typing (100, 1000 ops)
- Random inserts (100, 1000 ops)
- Random deletes (100, 1000 ops)
- Mixed insert/delete
- Large document merge
- Many small merges

Results summary (`research/40-crdt-comparison-results.md`):

| Implementation | Strengths | Weaknesses |
|----------------|-----------|------------|
| LoroRga | Best overall, Fugue algorithm | Slightly complex |
| DiamondRga | Fast deletes (10x faster than Yjs) | Random insert perf |
| ColaRga | Simple, fast deletes | O(n^2) random inserts |
| JsonJoyRga | Dual-tree concept | Splay overhead |
| YjsRga | Battle-tested, simple | O(n) deletes |

## Phase 5: Synthesis

Created `src/crdt/rga_optimized.rs` combining best techniques:

1. **Fugue algorithm** (from loro) - Best anti-interleaving
2. **B-tree weighted storage** (from diamond/loro) - O(log n) position lookups
3. **Separate content storage** - Per-user append-only buffers
4. **Compact user indices** - 16-bit instead of full public keys

Performance matches/beats LoroRga across all benchmarks.

## Phase 6: Log Integration

Created `src/crdt/log_integration.rs`:

- `Operation` enum (Insert, Delete) with binary encoding
- `OpLog` trait for export/rebuild/apply operations
- `LogEntry` with parent hash for chaining
- `VersionVector` for causality tracking

Design document: `research/41-crdt-log-integration.md`

26 tests for:
- Round-trip (export → rebuild → same content)
- Order independence (any operation order → same result)
- Determinism (same log → same state)

## Final Statistics

| Metric | Count |
|--------|-------|
| Library tests | 235 |
| Conformance tests | 156 (26 × 6 implementations) |
| Research documents | 8 files (~4500 lines) |
| RGA implementations | 6 (yjs, diamond, cola, json_joy, loro, optimized) |
| Benchmark files | 2 |
| Git commits | 12 |

## Key Learnings

### Algorithms

1. **Fugue > YATA** - Fugue prevents interleaving that YATA allows
2. **Dual origins essential** - Single anchor (cola) is simpler but less robust
3. **Tree structure matters** - B-tree beats linked list at scale

### Data Structures

1. **Weighted B-tree** - Best balance of complexity and performance
2. **Separate content storage** - Avoids duplication during merge
3. **ID index** - HashMap for O(1) merge lookups (from json-joy concept)
4. **Cursor caching** - Amortizes sequential access

### Tradeoffs

1. **Complexity vs simplicity** - Cola is simple but O(n^2) on random inserts
2. **Memory vs speed** - Splay trees optimize access patterns but add overhead
3. **Generality vs specialization** - Loro's rich types vs diamond's pure text focus

## Files Created

Research:
- `research/34-crdt-comparison-progress.md`
- `research/35-crdt-yjs.md`
- `research/36-crdt-diamond-types.md`
- `research/37-crdt-cola.md`
- `research/38-crdt-json-joy.md`
- `research/39-crdt-loro.md`
- `research/40-crdt-comparison-results.md`
- `research/41-crdt-log-integration.md`

Source:
- `src/crdt/rga_trait.rs`
- `src/crdt/primitives/` (6 files)
- `src/crdt/yjs.rs`
- `src/crdt/diamond.rs`
- `src/crdt/cola.rs`
- `src/crdt/json_joy.rs`
- `src/crdt/loro.rs`
- `src/crdt/rga_optimized.rs`
- `src/crdt/log_integration.rs`

Tests and benchmarks:
- `tests/rga_conformance.rs`
- `benches/rga_comparison.rs`
- `benches/rga_quick.rs`

## Next Steps

Per procedure: "Only after completion should you read the 'Scratchpad' section in DESIGN.md."

Potential follow-up work:
1. Integrate OptimizedRga with existing RgaBuf
2. Replace current RGA with optimized version
3. Implement full log-based sync protocol
4. Add cursor caching to OptimizedRga
