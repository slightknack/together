+++
model = "claude-opus-4-5"
created = 2026-02-01
modified = 2026-02-01
driver = "Isaac Clayton"
+++

# Procedure: Comprehensive CRDT Library Comparison Study

## Overview

This procedure guides the implementation of multiple sequence CRDT approaches from scratch, benchmarking them, and synthesizing the best techniques into a final optimized implementation.

The goal is to deeply understand each major CRDT library's approach, implement it cleanly, and extract reusable primitives that can be composed into a best-of-breed solution.

## Target Libraries

1. **diamond-types** - Joseph Gentle's high-performance Rust CRDT
2. **loro** - Modern CRDT with rich types and time travel
3. **cola** - Composable local-first algorithms
4. **json-joy** - TypeScript CRDT with novel optimizations
5. **yjs** - The original production CRDT, widely deployed

## Phase 1: Setup and Infrastructure

### 1.1 Create Branch and Trait Definition

```bash
git checkout -b crdt-comparison-study
```

Create `src/crdt/rga_trait.rs`:

```rust
/// The Rga trait defines the interface for a replicated growable array.
///
/// Implementors must provide:
/// - Insert operations with CRDT ordering
/// - Delete operations (tombstones or real deletion)
/// - Merge with another replica
/// - Conversion to/from visible content
pub trait Rga: Clone + Default {
    /// The user/agent identifier type.
    type UserId: Clone + Eq + Hash;
    
    /// Insert content at a visible position.
    fn insert(&mut self, user: &Self::UserId, pos: u64, content: &[u8]);
    
    /// Delete a range of visible characters.
    fn delete(&mut self, start: u64, len: u64);
    
    /// Merge another replica into this one.
    fn merge(&mut self, other: &Self);
    
    /// Get the visible content as a string.
    fn to_string(&self) -> String;
    
    /// Get the visible length.
    fn len(&self) -> u64;
    
    /// Check if empty.
    fn is_empty(&self) -> bool { self.len() == 0 }
}
```

### 1.2 Create Primitives Library

Create a shared primitives module at `src/crdt/primitives/mod.rs`:

```rust
pub mod clock;      // Lamport, vector, hybrid logical clocks
pub mod btree;      // B-tree variants (weighted, indexed)
pub mod splay;      // Splay trees
pub mod skiplist;   // Skip lists with gap buffers
pub mod cache;      // Cursor caching strategies
pub mod map;        // Fast hash maps (FxHashMap, etc.)
pub mod id;         // ID types (user, operation, item)
pub mod span;       // Span/run representations
```

Each primitive should:
- Have comprehensive property-based tests
- Be parameterized where sensible
- Document complexity guarantees
- Include benchmarks

### 1.3 Create Test Infrastructure

Create `tests/rga_conformance.rs`:

```rust
/// Conformance test suite that all Rga implementations must pass.
/// 
/// Tests:
/// - Basic insert/delete operations
/// - Merge commutativity: merge(A,B) == merge(B,A)
/// - Merge associativity: merge(A,merge(B,C)) == merge(merge(A,B),C)
/// - Merge idempotence: merge(A,A) == A
/// - Concurrent edit resolution
/// - Interleaving patterns
/// - Large document handling
/// - Unicode correctness
```

Use proptest for property-based testing of CRDT invariants.

## Phase 2: Research Each Library

For each library, create a research document at `research/crdt-{name}.md` with:

### 2.1 Document Structure

```markdown
# {Library Name} Deep Dive

## Overview
- Repository URL
- Language
- Primary use cases
- Key innovations

## Data Structure

### Core Types
[Describe the main data structures with diagrams]

### Memory Layout
[How data is laid out in memory, cache considerations]

### Indexing Structures
[How position, ID, and time lookups work]

## Merge Algorithm

### Ordering Rules
[YATA, Fugue, RGA, or custom ordering]

### Conflict Resolution
[How concurrent edits at same position are resolved]

### Complexity Analysis
[Time and space complexity for operations]

## Optimizations

### Batching
[How consecutive operations are batched]

### Caching
[Cursor caching, memoization]

### Memory
[Span coalescing, compression, garbage collection]

### Parallelism
[If any parallel/concurrent optimizations]

## Code Walkthrough
[Key functions with pseudocode]

## Lessons for Our Implementation
[What we should adopt]
```

### 2.2 Research Process for Each Library

1. **Clone and build** the library
2. **Read the papers** - Find academic papers or blog posts explaining the approach
3. **Trace execution** - Use a debugger to step through key operations
4. **Benchmark** - Run their benchmarks to understand performance characteristics
5. **Extract essence** - Identify the core ideas separate from implementation details

## Phase 3: Implement Each Approach

For each library, create `src/crdt/{name}.rs` implementing the `Rga` trait.

### 3.1 Implementation Guidelines

1. **Start simple** - Get correctness first, optimize later
2. **Use shared primitives** - Build on the primitives library
3. **Document heavily** - Explain why, not just what
4. **Test incrementally** - Run conformance tests after each major addition
5. **Benchmark early** - Know your baseline

### 3.2 Implementation Order

Suggested order from simpler to more complex:

1. **yjs** - Well-documented, straightforward YATA
2. **diamond-types** - Clear Rust code to reference
3. **cola** - Novel but well-explained approach
4. **json-joy** - Advanced optimizations (splay trees, dual indexing)
5. **loro** - Most complex, rich type system

### 3.3 Per-Implementation Checklist

For each implementation:

- [ ] Research document complete
- [ ] Basic structure compiles
- [ ] Insert at position works
- [ ] Delete works
- [ ] Merge with empty works
- [ ] Self-merge (idempotence) works
- [ ] Two-way merge works
- [ ] Three-way merge works
- [ ] Concurrent edit resolution correct
- [ ] Conformance tests pass
- [ ] Benchmark baseline established
- [ ] Optimizations applied
- [ ] Final benchmark recorded

## Phase 4: Comparative Analysis

### 4.1 Benchmark Suite

Create `benches/rga_comparison.rs`:

```rust
// Benchmarks:
// 1. Sequential typing (forward)
// 2. Sequential typing (backward/backspace)
// 3. Random inserts
// 4. Random deletes
// 5. Mixed insert/delete
// 6. Large document merge
// 7. Many small merges
// 8. Real editing traces (sveltecomponent, rustcode, etc.)
```

### 4.2 Metrics to Track

For each implementation, record:

| Metric | Description |
|--------|-------------|
| Insert time | ns per character |
| Delete time | ns per character |
| Merge time | ms per merge |
| Memory usage | bytes per character |
| Span count | internal fragmentation |
| Cache hit rate | cursor/index cache effectiveness |

### 4.3 Analysis Document

Create `research/crdt-comparison-results.md`:

```markdown
# CRDT Implementation Comparison

## Benchmark Results
[Tables and charts]

## Tradeoff Analysis
[Where each approach excels/struggles]

## Common Patterns
[Techniques used by multiple libraries]

## Novel Techniques
[Unique innovations worth adopting]

## Recommended Synthesis
[What to combine for best result]
```

## Phase 5: Synthesize Best Approach

### 5.1 Create Hybrid Implementation

Create `src/crdt/rga_optimized.rs`:

```rust
//! Optimized RGA combining best techniques from:
//! - diamond-types: JumpRope structure, gap buffers
//! - json-joy: splay tree caching, dual indexing
//! - loro: efficient tombstone handling
//! - yjs: proven YATA ordering
//! - cola: novel conflict resolution
```

### 5.2 Optimization Priorities

Based on profiling, focus on:

1. **Cache locality** - Keep hot data together
2. **Cursor caching** - Avoid redundant lookups
3. **Batching** - Coalesce consecutive operations
4. **Allocation** - Minimize heap allocations
5. **Branching** - Reduce unpredictable branches

### 5.3 Final Validation

- [ ] All conformance tests pass
- [ ] Beats diamond-types on all traces
- [ ] Memory usage is competitive
- [ ] Code is maintainable
- [ ] Documentation is complete

## Phase 6: Log Integration

### 6.1 Design Log-Compatible Interface

Ensure the final implementation can:
- Export operations as a log
- Replay operations from a log
- Produce deterministic output from log replay
- Support authenticated append-only log structure

### 6.2 Log Format

Design operation encoding compatible with the signed append-only log described in DESIGN.md.

## Agent Coordination

### Primary Agent Responsibilities

The primary agent (coordinator) should:

1. **Maintain progress tree** - Track which phases/libraries are complete
2. **Spawn research agents** - One per library for parallel research
3. **Review implementations** - Ensure quality and correctness
4. **Synthesize learnings** - Combine insights across libraries
5. **Write summary documents** - Keep learnings accessible

### Subagent Spawning Pattern

For each library research phase:

```
Task: Research and implement {library}
Subagent type: general-purpose
Prompt: |
  Research the {library} CRDT library in depth.
  
  1. Clone the repository and study the code
  2. Find and read any papers/blog posts about it
  3. Create research/{library}.md with the structure from the procedure
  4. Implement src/crdt/{library}.rs implementing the Rga trait
  5. Ensure all conformance tests pass
  6. Run benchmarks and record results
  
  Use shared primitives from src/crdt/primitives where possible.
  Add any new primitives needed.
```

### Progress Tracking

Maintain a progress file at `research/crdt-comparison-progress.md`:

```markdown
# CRDT Comparison Progress

## Phase 1: Setup
- [x] Branch created
- [x] Rga trait defined
- [x] Primitives library structure
- [ ] Conformance tests

## Phase 2: Research
- [ ] diamond-types: [status]
- [ ] loro: [status]
- [ ] cola: [status]
- [ ] json-joy: [status]
- [ ] yjs: [status]

## Phase 3: Implementation
- [ ] diamond-types: [status]
- [ ] loro: [status]
- [ ] cola: [status]
- [ ] json-joy: [status]
- [ ] yjs: [status]

## Phase 4: Analysis
- [ ] Benchmark suite
- [ ] Comparison document

## Phase 5: Synthesis
- [ ] Hybrid implementation
- [ ] Final optimization

## Phase 6: Log Integration
- [ ] Log-compatible interface
- [ ] Final validation

## Learnings Summary
[Updated as work progresses]
```

## Completion Criteria

The procedure is complete when:

1. All five libraries have been researched and implemented
2. All implementations pass conformance tests
3. Benchmark comparison is complete
4. Synthesis implementation beats all individual approaches
5. Log integration is designed
6. Summary document captures all learnings

Only after completion should you read the "Scratchpad" section in DESIGN.md.

## Appendix: Key Questions to Answer

For each library, answer:

1. How does it handle the core CRDT ordering problem?
2. What data structure holds the document content?
3. How are positions mapped to internal IDs?
4. How is the merge operation implemented?
5. What caching/indexing strategies are used?
6. How are consecutive operations batched?
7. What is the memory overhead per character?
8. How does it handle large documents (>1MB)?
9. What are the known limitations or edge cases?
10. What would you change about its design?

## Appendix: Primitives to Build

Essential primitives needed across implementations:

### Clocks
- Lamport clock
- Vector clock
- Hybrid logical clock

### Trees
- B-tree (with weight tracking)
- Splay tree
- AVL tree
- Red-black tree

### Lists
- Skip list
- Gap buffer
- Rope

### Maps
- FxHashMap (fast hashing)
- BTreeMap (ordered)
- Interval map

### IDs
- User ID (public key)
- Operation ID (user, seq)
- Item ID (user, seq, offset)

### Spans
- Compact span (minimal memory)
- Rich span (with metadata)
- Run-length encoded spans

### Caches
- Cursor cache (position → internal location)
- ID cache (ID → position)
- LRU cache (general purpose)
