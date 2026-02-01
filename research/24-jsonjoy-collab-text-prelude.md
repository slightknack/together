---
model = "claude-opus-4-5"
created = "2026-01-31"
modified = "2026-01-31"
driver = "Isaac Clayton"
source = "https://jsonjoy.com/blog/collaborative-text-sync-prelude"
---

# json-joy: Collaborative Text Editors (Part 1) - Prelude

## Overview

This is the first post of a four-part series introducing collaborative text editing concepts. It provides foundational context for understanding how json-joy approaches real-time collaboration.

## Collaborative Editing Approaches

### Two Main Paradigms

1. **Operational Transformation (OT)**: Traditional approach, centralized coordination
2. **CRDTs (Conflict-free Replicated Data Types)**: Newer approach, decentralized

### OT vs CRDT Comparison

| Aspect | OT | CRDT |
|--------|----|----|
| Design philosophy | Centralized coordination | Decentralized, merge later |
| Conflict handling | Transform operations | Automatic merge |
| Server requirement | Required for coordination | Optional |
| Offline support | Limited | Full |
| Complexity | High (transformation functions) | Lower (automatic merge) |
| Intent preservation | Explicit (capture in operations) | Implicit (structural) |

### Key Trade-offs

**OT trades complexity for intent capture**: 
- Can define high-level operations like "split text node"
- Operations are well-understood and intentional
- Transformation logic is complex

**CRDT trades intent for simplicity**:
- Works at low level (individual characters/elements)
- Guarantees eventual consistency automatically
- May lose sight of higher-level user intent

## Common Performance Myths

### "CRDTs are slow"
- Actually: json-joy CRDT is faster than native V8 strings
- Proper implementation matters more than algorithm choice

### "CRDTs have metadata overhead"
- Actually: Block-wise storage minimizes metadata
- 2-3 bytes per block (not per character)

### "CRDTs are faster than OT, but both are slow"
- Actually: json-joy achieves ~5M transactions/second
- Performance depends on implementation quality

## json-joy's Position

json-joy claims to offer:
- **Fastest list CRDT implementation in JavaScript**
- **Fastest text OT implementation in JavaScript**

This suggests they support both paradigms, allowing users to choose based on their needs.

## Algorithm Comparison: RGA vs YATA

### RGA (Replicated Growable Array)
Used by: json-joy, Automerge

- Single reference: points to character after which new character is inserted
- Simpler structure
- Lower metadata per operation

### YATA (Yet Another Transformation Approach)
Used by: Y.js, Y.rs

- Double reference: points to character before AND after
- More complex structure
- Additional metadata per operation

Both use **logical clocks** to timestamp operations for ordering.

## Key Concepts

### Eventual Consistency

Both OT and CRDT guarantee eventual consistency:
- Regardless of edit order
- All users end with same state

But paths differ:
- OT: Coordination ensures consistency
- CRDT: Mathematical properties ensure consistency

### Intent vs Data

**OT perspective**: Operations represent user intent
- "Bold this text" is an operation
- Transformations preserve intent across concurrent edits

**CRDT perspective**: Operations represent data changes
- Character inserted at position X
- Merge rules handle concurrent changes structurally

For rich text, this difference matters:
- OT can reason about "split text node" semantically
- CRDT sees individual character insertions

## Implications for Together

### Algorithm Choice

Together uses RGA (like json-joy and Automerge). This is a good choice:
- Simpler than YATA
- Well-understood in literature
- Efficient with proper implementation

### Performance Focus

json-joy demonstrates that:
- CRDT performance depends on implementation, not algorithm limits
- Block-wise storage dramatically reduces overhead
- Proper data structures (Splay trees, Rope) are key

### Future Considerations

If Together ever needs rich text (beyond plain text):
- Intent preservation becomes more important
- May need hybrid approach (CRDT for characters, OT-like for formatting)
- json-joy's approach of supporting both paradigms is interesting

## Technical Foundation

### Logical Clocks

Both RGA and YATA use logical timestamps:
- Each operation gets unique ID
- IDs determine ordering
- Enables merge without coordination

### Block-wise Storage

Key optimization across implementations:
- Consecutive operations with consecutive timestamps = 1 block
- Dramatically reduces node count
- json-joy achieves 21:1 compression on automerge-paper

### Tree Structures

Various implementations use different trees:
- json-joy: Splay trees (self-optimizing for access patterns)
- diamond-types: B-tree / content tree
- Y.js: Doubly-linked list with skip pointers

## References

- Source: https://jsonjoy.com/blog/collaborative-text-sync-prelude
- RGA paper: "Replicated abstract data types: Building blocks for collaborative applications"
- YATA paper: "YATA: A Concurrent CRDT for Collaborative Text Editing"
- OT paper: "Operational Transformation in Real-Time Group Editors"
