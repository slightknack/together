---
model = "claude-opus-4-5"
created = "2026-01-31"
modified = "2026-01-31"
driver = "Isaac Clayton"
---

# Procedure: Implement Logical Versioning

Branch: `ibc/document-logical`

## Overview

Logical versioning adds timestamps to each span and filters spans when accessing historical versions. This is the simplest approach with O(n) historical access time.

## Implementation Steps

### 1. Create Branch
```bash
git checkout optimization-loop
git checkout -b ibc/document-logical
```

### 2. Modify Span Structure

Add timestamps to track when each span was inserted and deleted:

```rust
struct Span {
    // ... existing fields ...
    
    /// Lamport timestamp when this span was inserted
    insert_time: u64,
    /// Lamport timestamp when this span was deleted (0 = not deleted)
    delete_time: u64,
}
```

Note: This increases span size from 24 to 32 bytes.

### 3. Update Span Operations

When inserting:
```rust
span.insert_time = self.lamport;
span.delete_time = 0;
```

When deleting:
```rust
span.delete_time = self.lamport;
```

### 4. Implement Version Methods

```rust
pub fn to_string_at(&self, version: &Version) -> String {
    let mut result = Vec::new();
    for span in self.spans.iter() {
        // Skip if not yet inserted at this version
        if span.insert_time > version.lamport {
            continue;
        }
        // Skip if already deleted at this version
        if span.delete_time > 0 && span.delete_time <= version.lamport {
            continue;
        }
        // Include this span
        let column = &self.columns[span.user_idx as usize];
        let start = span.content_offset as usize;
        let end = start + span.len as usize;
        result.extend_from_slice(&column.content[start..end]);
    }
    String::from_utf8(result).unwrap_or_default()
}

pub fn len_at(&self, version: &Version) -> u64 {
    let mut len = 0;
    for span in self.spans.iter() {
        if span.insert_time > version.lamport {
            continue;
        }
        if span.delete_time > 0 && span.delete_time <= version.lamport {
            continue;
        }
        len += span.len as u64;
    }
    len
}

pub fn slice_at(&self, start: u64, end: u64, version: &Version) -> Option<String> {
    // Similar to slice() but with version filtering
}
```

### 5. Update Tests

Enable the ignored version tests in `tests/document_api.rs`.

### 6. Run Tests

```bash
cargo test --test document_api
cargo test --test document_api_proptest
```

### 7. Commit

```bash
git add -A
git commit -m "Implement logical versioning with Lamport timestamps"
```

## Quality Gates

- [ ] All version tests pass
- [ ] All existing tests still pass
- [ ] Property-based tests pass
- [ ] Benchmark shows expected O(n) historical access

## Trade-offs

Pros:
- Simple implementation
- No extra memory for versioning (timestamps in spans)
- O(1) for current version access

Cons:
- O(n) for historical access
- Span size increases by 8 bytes (insert_time) + potential 8 bytes (delete_time)
