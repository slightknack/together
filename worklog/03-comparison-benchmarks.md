+++
model = "claude-opus-4-5"
created = 2026-01-31
modified = 2026-01-31
driver = "Isaac Clayton"
+++

# Worklog: CRDT Library Comparison Benchmarks

Goal: Benchmark Together against all major CRDT libraries to establish performance baseline.

## Libraries Tested

### Rust
- Together (this project)
- diamond-types
- Cola (cola-crdt)
- Loro
- Yrs
- Automerge

### JavaScript
- json-joy

### Other (analyzed but not benchmarked in harness)
- Zed text CRDT (extracted and benchmarked separately)

## Final Results

| Library | sveltecomponent | rustcode | seph-blog1 | automerge-paper |
|---------|----------------|----------|------------|-----------------|
| **Together** | 1.29 ms | 3.06 ms | 4.94 ms | 4.37 ms |
| diamond-types | 1.53 ms | 3.83 ms | 9.50 ms | 15.02 ms |
| Cola | 2.48 ms | 21.47 ms | 38.96 ms | 142.98 ms |
| json-joy (JS) | 7.64 ms | 25.13 ms | 53.49 ms | 99.19 ms |
| Loro | 15.16 ms | 36.16 ms | 77.20 ms | 144.84 ms |
| Automerge | 165.20 ms | 1180.60 ms | 431.83 ms | 303.34 ms |
| Yrs | 359.38 ms | 1182.30 ms | 5563.46 ms | 6520.55 ms |
| Zed text | ~1320 ms | ~2990 ms | ~10100 ms | ~18800 ms |

## Key Findings

1. **Together is fastest on all traces** - 1.2-3.4x faster than diamond-types (next fastest)

2. **Performance tiers emerge clearly:**
   - Tier 1 (< 20ms): Together, diamond-types
   - Tier 2 (20-150ms): Cola, json-joy, Loro
   - Tier 3 (150-500ms): Automerge
   - Tier 4 (> 1s): Yrs, Zed

3. **Cola scales poorly** - 1.9x slower on small traces, 32.7x slower on automerge-paper

4. **json-joy impressive for JavaScript** - faster than Loro (Rust) on some traces

5. **Zed's CRDT prioritizes features over speed** - designed for rich editor features (anchors, selections, undo groups)

## Research Produced

- research/20-25: json-joy blog analysis (dual splay tree, block-wise RGA)
- research/26: Zed CRDT architecture analysis
- research/27: Comprehensive benchmark summary

## Repository Organization

Reorganized benchmark structure:
- `benches/` - Together-only benchmarks (version_bench.rs)
- `comparisons/rust/` - Multi-library Rust comparison
- `comparisons/js/` - json-joy benchmark
- `comparisons/criterion/` - Criterion benchmarks (Together vs diamond-types)

Removed diamond-types and other external libraries from main Cargo.toml dev-dependencies.

## Session Notes

- Extracted Zed's text CRDT (text, clock, rope, sum_tree crates) for benchmarking
- Discovered Zed's "rope" is just storage; actual CRDT is in "text" crate
- json-joy uses novel dual splay tree design worth studying
- Published to github.com/slightknack/together with old history preserved in v0-2021 branch
