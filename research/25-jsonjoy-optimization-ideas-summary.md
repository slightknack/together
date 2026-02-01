---
model = "claude-opus-4-5"
created = "2026-01-31"
modified = "2026-01-31"
driver = "Isaac Clayton"
source = "json-joy blog posts research"
---

# json-joy Research: New Optimization Ideas for Together

## Summary

After analyzing five json-joy blog posts and comparing with Together's existing optimizations, this document catalogs **new ideas not yet implemented** in Together.

## Already Implemented in Together

Based on existing research documents, Together already has:

1. **Span coalescing** (79-91% rate) - equivalent to json-joy's block-wise RGA
2. **Chunked storage** - WeightedList with chunks
3. **Fenwick tree** - O(log n) chunk lookup
4. **Cursor caching** - in RgaBuf
5. **Buffered writes** - RgaBuf buffers inserts
6. **Compact spans** - reduced from 112 bytes
7. **Skip list** - for O(log n) navigation

## NEW Optimization Ideas from json-joy

### High Priority (Performance Impact)

#### 1. Dual Tree Structure (Spatial + Temporal)

**json-joy approach**: Every node has two sets of tree pointers:
- Spatial tree: In-order traversal = document layout
- Temporal tree: In-order traversal = editing history

**Why it helps**: 
- Navigation by position uses spatial tree
- Merge operations use temporal tree
- Each optimized for its purpose

**Implementation for Together**:
```rust
struct Span {
    // Existing fields...
    
    // Spatial tree pointers (position-based)
    spatial_left: Option<SpanIdx>,
    spatial_right: Option<SpanIdx>,
    
    // Temporal tree pointers (causality-based)
    temporal_left: Option<SpanIdx>,
    temporal_right: Option<SpanIdx>,
}
```

**Status**: NOT implemented. Currently we have only spatial indexing.

#### 2. Splay Tree Self-Optimization

**json-joy approach**: Uses Splay trees which rotate recently accessed nodes to root.

**Why it helps**:
- Optimizes for access locality
- "Inserting at the last place I inserted" becomes O(1) amortized
- Adapts to actual usage patterns

**Implementation for Together**:
- Consider splay tree for span storage
- Or add splay-like rotation to skip list

**Status**: NOT implemented. Our skip list doesn't self-optimize for access patterns.

#### 3. Fast Text Diff Algorithm

**json-joy approach**: Implements specialized diff algorithm with fast paths:
- Single character insertion: O(1) detection
- Sequential typing: Append detection
- Small edits: Optimized comparison

**Why it helps**:
- Editor sync requires detecting changes
- Fast diff enables responsive UI updates
- Reduces overhead for common patterns

**Implementation for Together**:
```rust
fn fast_diff(old: &str, new: &str, cursor_hint: usize) -> Vec<Op> {
    // Fast path: single char append
    if new.len() == old.len() + 1 && new.starts_with(old) {
        return vec![Insert { pos: old.len(), char: new.chars().last().unwrap() }];
    }
    // Fast path: single char delete at cursor
    // ...
    // Fallback: full diff
}
```

**Status**: NOT implemented. We don't have text diff functionality.

### Medium Priority (Quality/Testing)

#### 4. Cross-Library Correctness Verification

**json-joy approach**: 
- Materialize final document after each trace
- Compare against expected result
- Found bugs in Y.rs (3/5 traces wrong) and Automerge (crashed)

**Why it helps**:
- Catches subtle merge bugs
- Validates implementation correctness
- Provides confidence in algorithm

**Implementation for Together**:
```rust
#[test]
fn verify_trace_correctness() {
    let result = run_trace("automerge-paper");
    let expected = load_expected_result("automerge-paper");
    assert_eq!(result.content(), expected);
}
```

**Status**: NOT implemented (as systematic verification). We should add expected result comparison to benchmarks.

#### 5. Fuzz Testing Framework

**json-joy approach**:
- Collect traces from real editor usage (Quill)
- Random operation generation
- Verify consistency after each operation

**Why it helps**:
- Finds edge cases humans don't think of
- Ensures robustness under random input
- Catches memory issues and crashes

**Implementation for Together**:
```rust
#[test]
fn fuzz_rga_operations() {
    let mut rga = Rga::new();
    let mut rng = rand::thread_rng();
    
    for _ in 0..100_000 {
        match rng.gen_range(0..3) {
            0 => { /* random insert */ }
            1 => { /* random delete */ }
            2 => { /* random merge with clone */ }
        }
        assert!(rga.verify_invariants());
    }
}
```

**Status**: PARTIAL. We likely have some tests, but not systematic fuzzing.

### Low Priority (Polish/Future)

#### 6. Editor Integration Architecture

**json-joy approach**: 
- `collaborative-editor` package as abstraction layer
- Specific bindings for CodeMirror, Monaco, Ace
- React component wrappers

**Why it helps**:
- Clean separation of CRDT from UI
- Reusable sync logic
- Easy editor integration

**Implementation for Together**:
- Create `together-editor` crate
- Implement traits for editor abstraction
- Provide bindings via FFI/WASM

**Status**: NOT applicable for core benchmarks, but relevant for future integration.

#### 7. Inline Content Storage (Per-Block)

**json-joy approach**: Content stored within blocks, not separate columns.

**Why it helps**:
- Better cache locality (content near metadata)
- No indirection for content access
- Simpler memory layout

**Current Together approach**: Per-user content columns with offset arithmetic.

**Trade-off**: 
- Inline: Better locality, more complex splitting
- Columns: Simpler structure, indirection overhead

**Status**: NOT implemented. Currently using column-based storage.

## Priority Ranking

### Immediate Value (Implement Now)
1. **Cross-library correctness verification** - Low effort, high value for confidence
2. **Fuzz testing framework** - Medium effort, catches bugs early

### Future Optimization (If Needed)
3. **Dual tree structure** - High effort, helps merge-heavy workloads
4. **Splay tree self-optimization** - Medium effort, helps locality patterns
5. **Fast text diff** - Medium effort, needed for editor integration

### Architecture Decisions (Long-term)
6. **Inline content storage** - High effort, architectural change
7. **Editor integration architecture** - When needed for UI integration

## Benchmark-Specific Recommendations

### For automerge-paper (where we're 1.77x slower)

The trace has 259,778 operations resulting in 12,387 json-joy blocks (21:1 ratio).

**Check our block ratio**: If we have more spans, we need better coalescing.

```rust
// After running automerge-paper trace
println!("Operations: {}", op_count);
println!("Spans: {}", rga.span_count());
println!("Ratio: {:.1}:1", op_count as f64 / rga.span_count() as f64);
```

If ratio is worse than 21:1, investigate why coalescing isn't as aggressive.

### For all traces

Add correctness verification:
```rust
// In benchmark harness
let result = run_trace(trace);
if let Some(expected) = load_expected(trace) {
    assert_eq!(result.content(), expected, "Trace {} produced incorrect result", trace.name);
}
```

## Conclusion

The most impactful NEW ideas from json-joy research are:

1. **Dual tree structure** - Novel architecture we haven't considered
2. **Splay tree optimization** - Self-optimizing for access patterns
3. **Systematic correctness testing** - Verify against known-good results

The testing improvements (correctness verification, fuzzing) provide the best effort-to-value ratio and should be prioritized to build confidence in our implementation.

## References

- Research doc 20: Blazing Fast List CRDT
- Research doc 21: List CRDT Benchmarks
- Research doc 22: Fuzz Testing RGA CRDT
- Research doc 23: Plain Text Synchronization
- Research doc 24: Collaborative Text Editors Prelude
- Existing: research/13-future-optimizations.md
- Existing: research/06-optimization-lessons.md
