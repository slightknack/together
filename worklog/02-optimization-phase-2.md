+++
model = "claude-opus-4-5"
created = 2026-01-31
modified = 2026-01-31
driver = "Isaac Clayton"
+++

# Worklog: Optimization Phase 2

Goal: Beat diamond-types on ALL 4 benchmarks.

## Baseline (2026-01-31)

| Trace | Patches | Together | Diamond | Ratio |
|-------|---------|----------|---------|-------|
| sveltecomponent | 19,749 | 1.92ms | 1.76ms | 1.1x slower |
| rustcode | 40,173 | 4.13ms | 4.23ms | 0.98x (2% faster) |
| seph-blog1 | 137,993 | 9.29ms | 9.31ms | 1.0x (parity) |
| automerge-paper | 259,778 | 32.13ms | 15.55ms | 2.1x slower |

Target:
- sveltecomponent: < 1.76ms
- rustcode: < 4.23ms (already achieved)
- seph-blog1: < 9.31ms (already achieved)
- automerge-paper: < 15.55ms (need 2x improvement)

## Optimization Plan

Following research/14-optimization-phase-2.md:

1. B-Tree for Spans - Replace WeightedList with O(log n) B-tree
2. Delete Buffering - Buffer adjacent deletes in RgaBuf
3. Gap Buffer - Add gap buffers to nodes for O(1) sequential edits
4. Dual-Width Tracking - Track visible and total length per subtree

## Session Log

### Session Start

Baseline established. The automerge-paper trace is the main challenge at 2.1x slower.
The critical optimization is the B-tree for spans, which will give O(log n) operations
instead of the current O(sqrt n) within-chunk operations.

