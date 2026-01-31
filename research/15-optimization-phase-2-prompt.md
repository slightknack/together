---
model = "claude-opus-4-5"
created = "2026-01-31"
modified = "2026-01-31"
driver = "Isaac Clayton"
---

# Prompt: Optimization Phase 2

Copy the text below and paste it into a new Claude context.

---

## Prompt

Read PROCESS.md, then read all the code and docs, especially research/10 onwards and procedures/00-optimize.md. Then do the following:

**Goal**: Beat diamond-types on ALL 4 benchmarks (sveltecomponent, rustcode, seph-blog1, automerge-paper). Currently we beat 2/4. We need to be faster on all four.

**Secondary Goal**: Separate the CRDT layer from the text layer for cleanliness. Diamond-types uses JumpRope for text and ContentTree for CRDT ordering. We should have a similar separation: Rope for text, Rga for CRDT.

**Optimization List** (see research/14-optimization-phase-2.md for details):

1. Rope Integration - Use rope.rs for text storage, spans reference rope positions
2. Gap Buffer in Rope Nodes - O(1) sequential typing within nodes
3. Delete Buffering - Buffer adjacent deletes in RgaBuf, not just inserts
4. B-Tree for Spans - Replace WeightedList with O(log n) B-tree
5. Dual-Width B-Tree - Track visible and total length per subtree
6. Arena Allocation - Typed arena for span storage
7. SIMD Chunk Scanning - Vectorized weight scanning in B-tree leaves
8. Prefetch Hints - Hide memory latency in tree traversal

**Process**: Follow procedures/00-optimize.md. For each optimization serially, spawn a subagent that:
1. Researches the optimization deeply (read diamond-types, JumpRope source)
2. Implements it with tests
3. Runs all 4 benchmarks
4. If faster or neutral on all: commits with descriptive message
5. If slower on any: documents lessons, reverts, tries different approach

**Done Criteria**: Faster than diamond-types on all 4 benchmarks:
- sveltecomponent: < 1.77ms (currently ~1.94ms)
- rustcode: < 4.25ms (currently ~4.10ms, already faster)
- seph-blog1: < 9.63ms (currently ~8.40ms, already faster)
- automerge-paper: < 11ms (currently ~19ms, need 1.7x improvement)

**Never give up**. If an optimization doesn't work, try a different approach. Use profiling, proptests, and refactoring. The automerge-paper trace is the main challenge - the B-tree optimization is essential for it.

Write research notes to research/ and session logs to worklog/ as you work. Update research/14-optimization-phase-2.md with results.

---

## Context Files to Read

Essential (read these first):
- PROCESS.md - Project philosophy and conventions
- DESIGN.md - Technical architecture
- procedures/00-optimize.md - Optimization procedure
- research/14-optimization-phase-2.md - This phase's plan

Source code:
- src/crdt/rga.rs - Main RGA implementation (1760 lines)
- src/crdt/weighted_list.rs - Current span storage (875 lines)
- src/crdt/rope.rs - Existing rope implementation (602 lines)
- src/crdt/skip_list.rs - Skip list (may be useful for B-tree) (1582 lines)

Research (phase 1 learnings):
- research/10-diamond-types-notes.md - How diamond-types works
- research/11-jumprope-notes.md - How JumpRope works
- research/13-future-optimizations.md - Optimization ideas

Benchmarks:
- benches/quick_bench.rs - Fast iteration benchmarks
- benches/rga_trace.rs - Full Criterion benchmarks

## Current Benchmark Results

Run `cargo bench --bench quick_bench` to verify current state:

| Trace | Together | Diamond-types | Status |
|-------|----------|---------------|--------|
| sveltecomponent | 1.94ms | 1.77ms | 1.1x slower |
| rustcode | 4.10ms | 4.25ms | 3% faster |
| seph-blog1 | 8.40ms | 9.63ms | 13% faster |
| automerge-paper | ~19ms | ~11ms | 1.7x slower |
