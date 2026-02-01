+++
model = "claude-opus-4-5"
created = 2026-02-01
modified = 2026-02-01
driver = "Isaac Clayton"
+++

# Origin Index Optimization

## Goal

Implement Approach 3 (Origin Index) from procedures/04-insert-optimization.md to reduce remote insert complexity from O(n) to O(k) where k is the number of concurrent edits at the same position.

## Changes Made

### 1. Changed Origin Representation

Changed from unstable `(span_idx, offset)` to stable `(user_idx, seq)`:

**Before (OriginRef):**
```rust
struct OriginRef {
    span_idx: u32,  // Changes on insert/split!
    offset: u32,
}
```

**After (OriginId):**
```rust
struct OriginId {
    user_idx: u16,  // Stable - identifies user
    seq: u32,       // Stable - identifies character
}
```

The stable representation is essential because span indices change when spans are inserted or split, but `(user_idx, seq)` uniquely identifies a character regardless of document structure.

### 2. Added Origin Index

Added to Rga struct:
```rust
origin_index: FxHashMap<(u16, u32), SmallVec<[usize; 4]>>
```

Maps from origin ID to list of span indices that share that origin (siblings).

### 3. Updated insert_span_rga

The function now:
1. Sets origin using stable `OriginId::some(user_idx, seq)`
2. Looks up siblings via `origin_index.get(&origin_key)`
3. Still scans from origin+1 for correctness (catches unindexed siblings)
4. Updates the index after inserting

### 4. Updated Span::split

When splitting a span, the right part's origin is now correctly set to the last character of the left part: `(self.user_idx, self.seq + offset - 1)`.

## Benchmark Results

Comparison with diamond-types on standard editing traces:

| Trace | Patches | Baseline (main) | With Origin Index | Change |
|-------|---------|-----------------|-------------------|--------|
| sveltecomponent | 19,749 | 1.2x slower | 1.0-1.3x slower | Similar |
| rustcode | 40,173 | 1.2x slower | 0.8-0.9x slower | ~25% faster |
| seph-blog1 | 137,993 | 0.8x slower | 0.6x slower | ~25% faster |
| automerge-paper | 259,778 | 0.4x slower | 0.3x slower | ~25% faster |

Note: "0.3x slower" means we are 3.3x faster than diamond-types.

## Analysis

The origin index optimization shows modest improvement (20-25%) on larger traces. The improvement is limited on sequential editing traces because:

1. **Sequential edits rarely create siblings.** When a user types sequentially, each character has a unique origin (the previous character), so the origin index has at most 1 entry per origin.

2. **The optimization helps most with concurrent edits.** When two users edit at the same position simultaneously, both their characters share the same origin. The index lets us find siblings in O(k) instead of O(n).

3. **The baseline was already fast.** The main branch already had optimizations like cursor caching and span coalescing that handle the common (sequential) case efficiently.

## Test Results

All tests pass:
- `cargo test --lib`: 83 tests passed
- `cargo test --test trace_correctness`: 4 tests passed (all traces match diamond-types output)

## Future Work

The origin index will be more valuable when:
- Implementing merge of concurrent edits
- Processing traces with multiple concurrent users
- Handling adversarial input (many inserts at same position)

Consider creating adversarial test cases to demonstrate the O(k) vs O(n) difference more clearly.
