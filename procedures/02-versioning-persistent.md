# Procedure: Persistent Data Structure Versioning

## Goal
Implement versioning using a persistent (immutable) data structure approach where each version is a separate snapshot that shares structure with previous versions.

## Approach
Instead of storing timestamps and filtering, we maintain immutable versions of the document. Each edit creates a new "version" that shares unchanged structure with the previous version through structural sharing.

### Design Options

#### Option A: Copy-on-Write Spans List
- Each Version holds a reference to an immutable spans list
- Edits create a new spans list, copying only the modified portions
- Simple but may have high memory overhead for small edits

#### Option B: Persistent B-tree  
- Use an immutable B-tree where path from root to modified leaf is copied
- Structural sharing for unchanged subtrees
- O(log n) extra space per edit
- More complex to implement

#### Option C: Version Chain with Deltas
- Store base version as full snapshot
- Subsequent versions store forward deltas (what changed)
- Reconstruct by replaying deltas
- Good for sequential access, slow for random access

### Chosen Approach: Option A - Copy-on-Write with Arc

We'll use Rust's `Arc` (atomic reference counting) for structural sharing:
- `Version` holds `Arc<Vec<Span>>` 
- Current state tracks `Arc<Vec<Span>>`
- Each mutating operation clones the Arc, modifies the inner Vec, stores new Arc
- Old versions remain accessible through their Arc references

## Implementation Steps

1. Add `Arc<Vec<Span>>` for version storage
2. Track version history as `Vec<(u64, Arc<Vec<Span>>)>` (lamport, spans)
3. Implement `snapshot() -> Version`
4. Implement `to_string_at(version)`, `slice_at(version)`, `len_at(version)`
5. Add version retention policy (keep last N or geometric spacing)

## Trade-offs

**Pros:**
- Fast version access (O(1) to get snapshot, then normal operations)
- No filtering overhead during reads
- Clean separation between current state and history

**Cons:**  
- Memory overhead: each version snapshot takes O(n) space in worst case
- Need retention policy to bound memory usage
- Clone overhead on every edit (though Arc makes this cheap)

## Testing
- Reuse existing version tests from document_api.rs
- Verify version isolation (changes to current don't affect old versions)
- Verify memory is shared (Arc strong_count)
