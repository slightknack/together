+++
title = "insert_span_rga Control Flow Analysis and Refactoring"
date = 2026-02-01
+++

## Current Structure (~260 lines)

The function has three main branches:

### 1. Empty Document Case (5 lines)
```rust
if self.spans.is_empty() {
    // Set right origin, insert at 0
}
```

### 2. With Left Origin (180 lines)
```rust
if let Some(ref origin_id) = left_origin {
    // 2a. Set origins on span (10 lines)
    // 2b. Find origin span & maybe split it (15 lines)
    // 2c. YATA scan loop with subtree tracking (155 lines)
}
```

### 3. No Left Origin / Root Level (60 lines)
```rust
} else {
    // 3a. Set right origin (5 lines)
    // 3b. YATA scan loop (55 lines) - almost identical to 2c!
}
```

## Key Observations

### Observation 1: Duplicated YATA Comparison Logic

The comparison rules in branches 2c and 3b are **identical**:

1. Compare right origins (null right_origin = "inserted at end" = infinity)
2. If equal, tiebreak by (user, seq) descending

Both branches implement:
```rust
// Right origin comparison
if other.has_right_origin() != span.has_right_origin() {
    if !other.has_right_origin() && span.has_right_origin() {
        break;  // We come before other
    } else {
        pos = self.skip_subtree(pos);
        continue;  // Other comes before us
    }
}

if other.has_right_origin() && span.has_right_origin() {
    // Compare right origin IDs...
}

// User/seq tiebreaker
if (other_user, other.seq) > (span_user, span.seq) {
    pos = self.skip_subtree(pos);
    continue;
}
break;
```

### Observation 2: Three Traversal Patterns

1. **`skip_subtree(pos)`**: Skip span at `pos` and all descendants
2. **Subtree membership scan**: Track `subtree_ranges: Vec<(u16, u32, u32)>` and check `any()` on each iteration
3. **Sibling-only iteration**: Only compare with spans sharing our left origin

### Observation 3: The Subtree Tracking is Complex

The `subtree_ranges` vector grows as we scan, and we do O(n) membership checks on each iteration. This is potentially O(n²) in the worst case.

## Refactoring Opportunities

### Opportunity 1: Extract YATA Comparison

```rust
/// Result of YATA comparison between two items.
enum YataOrder {
    /// New item comes BEFORE existing item
    Before,
    /// New item comes AFTER existing item  
    After,
}

impl Rga {
    /// Compare using YATA/FugueMax rules.
    /// Returns whether `span` should come before `other`.
    fn yata_comes_before(&self,
        span_right_origin: OriginId,
        span_user: KeyPub,
        span_seq: u32,
        other: &Span,
    ) -> bool {
        let other_right_origin = other.right_origin();
        
        // Null right_origin = "inserted at end" = infinity
        // Non-null < null, so non-null comes first
        match (span.has_right_origin(), other.has_right_origin()) {
            (true, false) => return true,   // We're finite, other is infinity
            (false, true) => return false,  // We're infinity, other is finite
            (false, false) => {},           // Both infinity, use tiebreaker
            (true, true) => {
                // Compare right origin IDs
                let span_ro_key = self.origin_to_key(span_right_origin);
                let other_ro_key = self.origin_to_key(other_right_origin);
                
                if other_ro_key > span_ro_key {
                    return false;  // Other was inserted later, comes first
                } else if other_ro_key < span_ro_key {
                    return true;   // We were inserted later, come first
                }
                // Equal, fall through to tiebreaker
            }
        }
        
        // Tiebreaker: higher (user, seq) comes first
        let other_user = self.users.get_key(other.user_idx).unwrap();
        (span_user, span_seq) > (*other_user, other.seq)
    }
}
```

### Opportunity 2: Sibling Iterator

Abstract the subtree-skipping traversal into an iterator:

```rust
/// Iterates over siblings (spans with same left origin), skipping subtrees.
struct SiblingIter<'a> {
    rga: &'a Rga,
    left_origin: Option<OriginId>,
    pos: usize,
    subtree_ranges: Vec<(u16, u32, u32)>,
}

impl<'a> Iterator for SiblingIter<'a> {
    type Item = (usize, &'a Span);  // (position, span)
    
    fn next(&mut self) -> Option<Self::Item> {
        while self.pos < self.rga.spans.len() {
            let span = self.rga.spans.get(self.pos)?;
            
            if self.is_sibling(span) {
                let result = (self.pos, span);
                self.add_to_subtree(span);
                self.pos += 1;
                return Some(result);
            }
            
            if self.is_descendant(span) {
                self.add_to_subtree(span);
                self.pos += 1;
                continue;
            }
            
            // Exited subtree
            return None;
        }
        None
    }
}
```

### Opportunity 3: Unified Insert Logic

With the above primitives, the main function becomes:

```rust
fn insert_span_rga(&mut self, mut span: Span, left_origin: Option<ItemId>, right_origin: Option<ItemId>) {
    self.set_origins(&mut span, &left_origin, &right_origin);
    
    if self.spans.is_empty() {
        self.spans.insert(0, span, span.visible_len() as u64);
        return;
    }
    
    // Find start position (after left origin, or 0 if none)
    let start_pos = self.prepare_insertion_point(&left_origin);
    
    // Find position among siblings using YATA rules
    let insert_idx = self.find_yata_position(start_pos, &left_origin, &span);
    
    self.spans.insert(insert_idx, span, span.visible_len() as u64);
}

fn find_yata_position(&self, start: usize, left_origin: &Option<ItemId>, span: &Span) -> usize {
    for (pos, other) in self.siblings_from(start, left_origin) {
        if self.yata_comes_before(span, other) {
            return pos;
        }
    }
    // No sibling we should come before - insert at end of siblings
    self.end_of_subtree(start, left_origin)
}
```

## Performance Consideration: Subtree Tracking

The current `subtree_ranges: Vec<(u16, u32, u32)>` with linear search is O(n²) worst case.

### Alternative: Use a HashMap or interval tree

For large documents with many concurrent edits:
```rust
// Track which (user, seq) ranges are in subtree
subtree: HashMap<u16, Vec<(u32, u32)>>  // user -> [(start, end), ...]
```

Or simpler: since we process in order, we could track just the "frontier" - the maximum seq seen for each user in the subtree. But this doesn't work because subtree membership isn't monotonic.

### Alternative: Don't track - use skip_subtree

Instead of tracking membership, we could call `skip_subtree()` when we want to skip a sibling. But we already do this when skipping due to YATA ordering. The issue is we ALSO need to recognize descendants when iterating.

Actually, looking more carefully: the subtree tracking is necessary because when we encounter a non-sibling, we need to know if it's:
1. A descendant (skip it, keep scanning)
2. A different branch (stop)

The `skip_subtree()` function already solves this! We could restructure to:
1. Only look at siblings
2. When encountering a non-sibling, call `skip_subtree()` on the PREVIOUS sibling to jump past all its descendants

But wait, that doesn't work either because we need to know which sibling the descendant belongs to.

## Recommended Refactoring (Minimal)

Focus on the most impactful change: **extract YATA comparison**.

This reduces duplication between the two branches from ~100 lines to ~30 lines, and makes the algorithm clearer.

```rust
fn insert_span_rga(&mut self, mut span: Span, left_origin: Option<ItemId>, right_origin: Option<ItemId>) {
    self.set_origins_on_span(&mut span, &left_origin, &right_origin);
    
    if self.spans.is_empty() {
        self.spans.insert(0, span, span.visible_len() as u64);
        return;
    }
    
    let insert_idx = if let Some(ref origin_id) = left_origin {
        self.find_position_with_origin(origin_id, &span)
    } else {
        self.find_position_at_root(&span)
    };
    
    self.spans.insert(insert_idx, span, span.visible_len() as u64);
}
```

This is a moderate refactoring that improves readability without changing the algorithm.
