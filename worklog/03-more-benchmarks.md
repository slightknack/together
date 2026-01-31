# Benchmarking Together against diamond-types

## Repository
https://github.com/slightknack/together

## Benchmark Traces
Download from https://github.com/josephg/editing-traces

| Trace | Ops | Description |
|-------|-----|-------------|
| `sveltecomponent` | 19,749 | Editing a Svelte component file |
| `rustcode` | 40,173 | Editing Rust source code |
| `seph-blog1` | 137,993 | Writing a blog post |
| `automerge-paper` | 259,778 | Writing the Automerge academic paper |

## Libraries to Benchmark
1. **Together** - https://github.com/slightknack/together
2. **diamond-types** - https://github.com/josephg/diamond-types
3. **Automerge** - https://github.com/automerge/automerge
4. **Yrs** - https://github.com/y-crdt/y-crdt
5. **Loro** - https://github.com/loro-dev/loro

## Instructions

1. Clone all repositories
2. For each library, write a benchmark harness that:
   - Loads each trace file from editing-traces
   - Replays all operations (inserts/deletes) through the CRDT
   - Measures total time in milliseconds
3. Run each benchmark 10 times, report median
4. Output a table in this format:

| Trace | diamond-types (ms) | Together (ms) | Automerge (ms) | Yrs (ms) | Loro (ms) |
|-------|-------------------|---------------|----------------|----------|-----------|
| `sveltecomponent` | | | | | |
| `rustcode` | | | | | |
| `seph-blog1` | | | | | |
| `automerge-paper` | | | | | |

## Notes
- Use release builds with optimizations (`--release` for Rust)
- Warm up the CPU before benchmarking
- Ensure consistent hardware/environment across all runs
