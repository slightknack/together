---
model = "claude-opus-4-5"
created = "2026-01-31"
modified = "2026-01-31"
driver = "Isaac Clayton"
source = "https://jsonjoy.com/blog/list-crdt-benchmarks"
---

# json-joy: List CRDT Benchmarks

## Overview

The json-joy team benchmarked their Block-wise RGA implementation against other JavaScript list CRDTs, non-CRDT libraries, and native V8 strings. The results challenge common assumptions about CRDT performance.

## Benchmark Methodology

- Run each trace against each library
- Measure time to execute full trace
- Run 50 times, report average of last 45 (first 5 are warm-up)
- At end of each trace, materialize final document text to verify correctness

### Primary Benchmark Trace

**automerge-paper**: 259,778 single character insert/delete transactions
- Final document size: 104,852 bytes
- Results in 12,387 json-joy RGA blocks

## Benchmark Results

### CRDT Libraries Comparison

The existing JavaScript CRDT libraries presented "no contest" to json-joy. The team found they needed to compare against non-CRDT libraries to find meaningful competition.

### Non-CRDT Comparison Group

Libraries that perform very fast string inserts and deletes:
1. **Diamond Types** (Rust/WebAssembly)
2. **Rope.js** (specialized string library)
3. **Native V8 JavaScript strings**

### Key Findings

1. **All non-CRDT libraries**: Over 1 million operations per second
2. **Native V8 strings**: Drops to hundreds of thousands ops/sec for larger documents
3. **json-joy**: Faster than native strings (uses Rope internally for large strings)
4. **Diamond Types**: Almost as fast as json-joy in JavaScript, likely faster in native Rust

## Why json-joy is Faster Than Native Strings

For large strings, json-joy internally uses a **Rope data structure** to represent text contents. Rope operations are O(log n) for inserts/deletes, while native string operations are O(n) for mid-string modifications.

This explains why json-joy beats native V8 strings on larger documents - the algorithmic advantage outweighs the overhead.

## Performance Numbers

- **json-joy throughput**: ~5 million transactions per second (single thread)
- Each transaction usually contains a single operation but can contain multiple

## Bugs Found in Other Libraries

Through benchmarking, the json-joy team discovered issues:

### Y.rs Issues
- Failed to produce correct trace results in 3 out of 5 traces
- Produced approximately correct document size
- Text was "noticeably incorrect"

### Automerge Issues
- Crashed on all traces except the first
- Rust-specific errors from WebAssembly module after a few iterations
- Could not handle the rustcode trace even with all compute resources

## Common CRDT Performance Myths Debunked

Common claims the benchmarks contradict:
1. "CRDTs are slow" - json-joy is faster than native strings
2. "CRDTs are slow because of metadata overhead" - Metadata is minimal with block storage
3. "CRDTs are faster than OT, but both are slow" - json-joy achieves millions of ops/sec

## Key Insights for Together

### Benchmark Comparison Points

Our benchmarks use similar traces:
- sveltecomponent: 19,749 patches
- rustcode: 40,173 patches
- seph-blog1: 137,993 patches
- automerge-paper: 259,778 patches

### Our Current Standing

From our research, we beat diamond-types on 3/4 benchmarks. The automerge-paper trace (where json-joy excels with 12,387 blocks) is where we're still 1.77x slower.

### Optimization Opportunities

1. **Block compression ratio**: json-joy achieves 21:1 compression (259k ops -> 12k blocks)
   - Check our span coalescing ratio on automerge-paper
   - May need more aggressive coalescing

2. **Correctness verification**: json-joy verifies final document text matches expected
   - We should add similar verification to our benchmarks

3. **Rope for content**: json-joy uses Rope for text content storage
   - Our content is in per-user columns
   - Rope-based content could help large documents

## Performance Hierarchy (from benchmarks)

1. Diamond Types (native Rust) - fastest
2. json-joy (JavaScript CRDT) - very close
3. Rope.js (specialized string library)
4. Native V8 strings - slowest for large documents

## Implications

The benchmark results suggest that with proper implementation:
- CRDTs can be competitive with or faster than specialized non-CRDT libraries
- The algorithmic overhead of CRDTs can be amortized through block-wise storage
- WebAssembly (Diamond Types) provides near-native performance

## References

- Source: https://jsonjoy.com/blog/list-crdt-benchmarks
- automerge-paper trace: Standard CRDT benchmark trace
- Diamond Types: https://github.com/josephg/diamond-types
