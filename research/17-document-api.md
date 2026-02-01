+++
model = "claude-opus-4-5"
created = 2026-01-31
modified = 2026-01-31
driver = "Isaac Clayton"
+++

# Document API: Slices, Anchors, and Versioning

This document tracks the design and implementation of three document API features:

1. **slice(start, end)**: Read a range of characters without allocating the full document
2. **Anchors**: Opaque positions that move with edits
3. **Versioning**: Access historical document states

## Goals

- Design clean, ergonomic APIs following the process (API before implementation)
- Test-driven development with real-world and property-based tests
- Implement three versioning approaches on separate branches:
  - `ibc/document-logical`: Filter spans by timestamp
  - `ibc/document-persistent`: Persistent/immutable B-tree structure
  - `ibc/document-checkpoint`: Periodic snapshots with geometric retention
- Benchmark and merge the fastest

## Progress

### Phase 1: Core API Design and Implementation (main branch)

- [ ] Design API for slice and anchors
- [ ] Write real-world-like tests
- [ ] Write property-based tests
- [ ] Implement slice(start, end)
- [ ] Implement Anchor type

### Phase 2: Versioning Implementations (separate branches)

- [ ] ibc/document-logical: Logical timestamp filtering
- [ ] ibc/document-persistent: Persistent data structure
- [ ] ibc/document-checkpoint: Checkpoint with geometric retention

### Phase 3: Benchmarking and Selection

- [ ] Create benchmarks for versioning operations
- [ ] Run benchmarks on all three approaches
- [ ] Merge fastest to main

---

## API Design

### 1. slice(start, end)

Read characters in the range [start, end) efficiently using the B-tree.

```rust
impl Rga {
    /// Read characters in the range [start, end) without allocating the full document.
    /// Returns None if the range is out of bounds.
    /// 
    /// # Example
    /// ```
    /// let mut rga = Rga::new();
    /// // ... insert "hello world"
    /// assert_eq!(rga.slice(0, 5), Some("hello".to_string()));
    /// assert_eq!(rga.slice(6, 11), Some("world".to_string()));
    /// ```
    pub fn slice(&self, start: u64, end: u64) -> Option<String>;
}
```

### 2. Anchors

An anchor is an opaque reference to a position in the document that moves with edits.

```rust
/// A position in the document that tracks a specific character.
/// Anchors move with edits: if text is inserted before the anchor,
/// the anchor's resolved position increases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    // Internal: references a specific ItemId
    user_idx: u16,
    seq: u32,
    bias: AnchorBias,
}

/// Whether the anchor stays before or after its target character
/// when text is inserted exactly at the anchor position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorBias {
    /// Anchor stays before the character (insertion at anchor pushes anchor right)
    Before,
    /// Anchor stays after the character (insertion at anchor keeps anchor in place)
    After,
}

impl Rga {
    /// Create an anchor at the given visible position.
    /// Returns None if position is out of bounds.
    pub fn anchor_at(&self, pos: u64, bias: AnchorBias) -> Option<Anchor>;
    
    /// Resolve an anchor to its current visible position.
    /// Returns None if the anchored character has been deleted.
    pub fn resolve_anchor(&self, anchor: &Anchor) -> Option<u64>;
}

/// A range defined by two anchors.
pub struct AnchorRange {
    pub start: Anchor,
    pub end: Anchor,
}

impl Rga {
    /// Create an anchor range for [start, end).
    /// The start anchor has After bias (expands when inserting at start).
    /// The end anchor has Before bias (expands when inserting at end).
    pub fn anchor_range(&self, start: u64, end: u64) -> Option<AnchorRange>;
    
    /// Get the current slice for an anchor range.
    pub fn slice_anchored(&self, range: &AnchorRange) -> Option<String>;
}
```

### 3. Versioning

Each approach will implement the same interface:

```rust
/// A version identifier (implementation varies by approach)
pub struct Version { /* ... */ }

impl Rga {
    /// Get the current version.
    pub fn version(&self) -> Version;
    
    /// Read a slice at a specific version.
    pub fn slice_at(&self, start: u64, end: u64, version: &Version) -> Option<String>;
    
    /// Get the full document at a specific version.
    pub fn to_string_at(&self, version: &Version) -> String;
    
    /// Get the document length at a specific version.
    pub fn len_at(&self, version: &Version) -> u64;
}
```

---

## Test Design

### Real-World Tests

These tests simulate actual editing patterns:

```rust
#[test]
fn test_slice_basic() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "hello world");
    assert_eq!(rga.slice(0, 5), Some("hello".to_string()));
    assert_eq!(rga.slice(6, 11), Some("world".to_string()));
    assert_eq!(rga.slice(0, 11), Some("hello world".to_string()));
}

#[test]
fn test_anchor_tracks_insertions() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "a cat on a rug");
    
    // Create anchors around "a cat"
    let start = rga.anchor_at(0, AnchorBias::After).unwrap();
    let end = rga.anchor_at(5, AnchorBias::Before).unwrap();
    
    // Insert "blue " before "cat"
    insert_text(&mut rga, 2, "blue ");
    
    // Anchors should now span "a blue cat"
    assert_eq!(rga.resolve_anchor(&start), Some(0));
    assert_eq!(rga.resolve_anchor(&end), Some(10)); // moved from 5 to 10
    
    let range = AnchorRange { start, end };
    assert_eq!(rga.slice_anchored(&range), Some("a blue cat".to_string()));
}

#[test]
fn test_version_rewind() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "hello");
    let v1 = rga.version();
    
    insert_text(&mut rga, 5, " world");
    let v2 = rga.version();
    
    delete_range(&mut rga, 0, 6); // delete "hello "
    let v3 = rga.version();
    
    assert_eq!(rga.to_string_at(&v1), "hello");
    assert_eq!(rga.to_string_at(&v2), "hello world");
    assert_eq!(rga.to_string_at(&v3), "world");
}
```

### Property-Based Tests

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn slice_equals_substring_of_to_string(
        ops in prop::collection::vec(arbitrary_op(), 1..100),
        start in 0u64..1000,
        len in 0u64..100,
    ) {
        let mut rga = Rga::new();
        for op in ops {
            apply_op(&mut rga, op);
        }
        
        let full = rga.to_string();
        let end = (start + len).min(rga.len());
        let start = start.min(rga.len());
        
        if start <= end {
            let expected = &full[start as usize..end as usize];
            assert_eq!(rga.slice(start, end), Some(expected.to_string()));
        }
    }
    
    #[test]
    fn anchor_position_consistent(
        ops in prop::collection::vec(arbitrary_op(), 1..50),
        anchor_pos in 0u64..100,
    ) {
        let mut rga = Rga::new();
        // First build up some content
        for op in &ops[..ops.len()/2] {
            apply_op(&mut rga, op.clone());
        }
        
        if rga.len() == 0 { return Ok(()); }
        let pos = anchor_pos % rga.len();
        let anchor = rga.anchor_at(pos, AnchorBias::After).unwrap();
        
        // Apply more ops
        for op in &ops[ops.len()/2..] {
            apply_op(&mut rga, op.clone());
        }
        
        // Anchor should resolve to a valid position or None (if deleted)
        if let Some(resolved) = rga.resolve_anchor(&anchor) {
            assert!(resolved <= rga.len());
        }
    }
    
    #[test]
    fn version_content_matches_snapshot(
        ops in prop::collection::vec(arbitrary_op(), 1..100),
    ) {
        let mut rga = Rga::new();
        let mut snapshots = vec![];
        
        for op in ops {
            apply_op(&mut rga, op);
            snapshots.push((rga.version(), rga.to_string()));
        }
        
        // Verify all versions
        for (version, expected) in snapshots {
            assert_eq!(rga.to_string_at(&version), expected);
        }
    }
}
```

---

## Implementation Notes

### slice(start, end)

1. Use B-tree's weighted index to seek to `start` position in O(log n)
2. Iterate spans, collecting content until we've read `end - start` characters
3. Skip deleted spans (they have 0 visible weight)

### Anchors

1. Store `(user_idx, seq)` to identify the character
2. Need reverse lookup: given ItemId, find span index
3. Options:
   - HashMap<ItemId, span_idx>: O(1) lookup, O(n) memory
   - Binary search on spans: O(log n) lookup, O(1) extra memory
4. For MVP, use binary search since spans are ordered by (user_idx, seq) within each user's column

### Versioning Approaches

#### Logical (ibc/document-logical)
- Add Lamport timestamp to each span
- `slice_at(version)` filters spans with timestamp <= version
- For deleted spans, track deletion timestamp
- O(n) worst case for historical reads

#### Persistent (ibc/document-persistent)
- Use immutable/persistent B-tree (copy-on-write)
- Each operation creates new root sharing most nodes
- O(log n) access to any version
- Higher memory usage

#### Checkpoint (ibc/document-checkpoint)
- Store full snapshots at intervals
- Use geometric/Fibonacci retention to keep more recent checkpoints
- To access version V: find nearest checkpoint <= V, replay ops
- Tunable tradeoff between memory and replay cost

---

## Checkpoint Retention Research

Need to research optimal checkpoint retention strategies:
- Fibonacci spacing
- Exponential/geometric spacing  
- Logarithmic spacing
- Comparison of approaches

Will add findings here after research.
