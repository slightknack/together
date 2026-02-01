---
model = "claude-opus-4-5"
created = "2026-01-31"
modified = "2026-01-31"
driver = "Isaac Clayton"
source = "https://jsonjoy.com/blog/fuzz-testing-rga-crdt"
---

# json-joy: Fuzz Testing RGA CRDT

## Overview

This blog post covers making sure collaborative text editing works correctly through fuzz testing. The json-joy team developed systematic testing approaches to ensure CRDT correctness.

## Testing Methodology

### Trace-Based Testing

The methodology is straightforward:
1. Run each editing trace against each library
2. Measure execution time
3. Run 50 times, report average of last 45 runs (first 5 are warm-up)
4. Materialize final document text into native JavaScript string
5. Verify correctness of final document state

### Correctness Verification

The last materialized string is used to verify the correctness of the final document state. This catches:
- Incorrect ordering of characters
- Missing characters
- Duplicate characters
- Incorrect merge results

## Bugs Found Through Testing

### Y.rs Bugs

- Failed to produce correct trace results in **3 out of 5 traces**
- Still produced approximately correct document size
- Text was "noticeably incorrect"
- Suggests issues with concurrent edit handling or merge logic

### Automerge Bugs

- Crashed on **all traces except the first one**
- After running for a few iterations, crashes with Rust-specific errors from WebAssembly module
- Could not handle the rustcode trace even when given all compute resources
- Suggests memory management or state accumulation issues

## Fuzzer Development

The json-joy changelog shows evidence of systematic fuzzer development:
- "Add sample collected Quill fuzzer traces to tests"
- Various bug fixes discovered through fuzzing
- Fixes for slice handling and content processing issues

### Fuzzer Trace Collection

Fuzz testing integrates with real editor (Quill) to collect realistic editing traces. This captures:
- Real user editing patterns
- Edge cases from actual collaborative sessions
- Complex interleaving of operations

## Key Testing Strategies

### 1. Deterministic Replay

Editing traces provide deterministic replay capability:
- Same trace produces same result
- Can bisect to find exact operation causing issues
- Enables regression testing

### 2. Final State Verification

Comparing final document text against expected result catches:
- Off-by-one errors in position calculation
- Incorrect tombstone handling
- Merge ordering issues

### 3. Cross-Library Comparison

Running same trace on multiple libraries provides oracle:
- If all libraries produce same result, likely correct
- Divergence indicates bugs (in one or more libraries)

### 4. Resource Stress Testing

Testing with "all compute resources" reveals:
- Memory leaks
- Unbounded state growth
- Performance degradation under load

## Implications for Together

### Testing Strategies to Adopt

1. **Final state verification**: After each benchmark trace, verify final text matches expected
   - We already materialize content; add comparison step
   - Could catch subtle bugs we're not aware of

2. **Cross-library oracle**: Compare our results with diamond-types on same traces
   - Any divergence indicates a bug in one or both implementations

3. **Fuzzer integration**: Develop fuzz tests that generate random operations
   ```rust
   // Pseudo-code for fuzzer
   fn fuzz_test() {
       let mut rga = Rga::new();
       for _ in 0..10000 {
           match random_op() {
               Insert(pos, char) => rga.insert(pos, char),
               Delete(pos) => rga.delete(pos),
           }
           assert!(rga.is_consistent());
       }
   }
   ```

4. **Memory/resource monitoring**: Track memory usage during long traces
   - Detect unbounded growth
   - Find memory leaks

### Edge Cases to Test

Based on bugs found in other libraries:
- Very long traces (100k+ operations)
- High concurrency (many interleaved operations)
- Repeated insert/delete at same position
- Operations at document boundaries (start/end)
- Empty document operations
- Unicode edge cases (multi-byte characters, combining marks)

## Correctness Properties

For any CRDT implementation, fuzz testing should verify:

1. **Convergence**: All replicas reach same state after receiving same operations
2. **Intent preservation**: User edits appear in reasonable positions
3. **Causality**: Operations respect causal ordering
4. **Idempotence**: Applying same operation twice doesn't change result

## References

- Source: https://jsonjoy.com/blog/fuzz-testing-rga-crdt
- Quill editor: https://quilljs.com/
- json-joy GitHub: https://github.com/streamich/json-joy
