---
model = "claude-opus-4-5"
created = "2026-01-31"
modified = "2026-01-31"
driver = "Isaac Clayton"
---

# Checkpoint Retention Strategies

Research on efficient checkpoint retention strategies for document versioning.

## Problem Statement

When maintaining version history for a document, we want:
1. High granularity for recent versions (easy undo)
2. Lower granularity for older versions (save space)
3. O(1) time per operation
4. O(log n) space for n operations

## Approaches

### 1. Logarithmically-Spaced Snapshots

Source: [Made by Evan - Log-Spaced Snapshots](https://madebyevan.com/algos/log-spaced-snapshots/)

This algorithm keeps approximately `2 * log2(n)` snapshots for `n` updates.

#### Algorithm

After each step `n`:
1. Add a new snapshot numbered `n`
2. Delete the snapshot at position: `n - (firstZeroBit(n) << d)`
   - `firstZeroBit(x)` returns the least significant zero bit of x
   - `d` is a density parameter (higher = more snapshots)

#### Implementation of firstZeroBit

```rust
fn first_zero_bit(x: u64) -> u64 {
    (x + 1) & !x
}
```

#### Properties
- O(1) time per update
- O(log n) space
- No complex bookkeeping required
- Recent snapshots are dense, old snapshots are sparse

#### Example (d=0)

```
Step  Snapshots retained
1     [1]
2     [1, 2]
3     [1, 3]
4     [1, 3, 4]
5     [1, 5]
6     [1, 5, 6]
7     [1, 5, 7]
8     [1, 5, 7, 8]
...
```

### 2. Geometric/Exponential Spacing

Keep snapshots at exponentially increasing intervals:
- Keep all snapshots from last N operations
- Keep every 2nd snapshot from N to 2N operations ago  
- Keep every 4th snapshot from 2N to 4N operations ago
- etc.

#### Properties
- O(log n) space
- Requires periodic "compaction" pass
- More predictable retention pattern

### 3. Fibonacci Spacing

Keep snapshots at Fibonacci intervals: 1, 1, 2, 3, 5, 8, 13, 21...

This gives slightly denser coverage than pure exponential while still achieving O(log n) space.

## Decision

For the checkpoint approach, we'll use **logarithmically-spaced snapshots** because:
1. O(1) time per operation (no periodic compaction)
2. O(log n) space
3. Simple implementation
4. Well-analyzed algorithm

The density parameter `d` can be tuned:
- d=0: ~2 * log2(n) snapshots (most sparse)
- d=1: ~4 * log2(n) snapshots
- d=2: ~8 * log2(n) snapshots
- etc.

For document editing, d=1 or d=2 seems reasonable to provide good granularity.

## Implementation Plan

### Checkpoint Structure

```rust
struct Checkpoint {
    /// Version at this checkpoint
    version: u64,
    /// Full snapshot of document state
    content: String,
    /// Or more efficiently: snapshot of Rga state
    spans: Vec<Span>,
    columns: Vec<Column>,
}

struct CheckpointStore {
    /// Map from version to checkpoint
    checkpoints: HashMap<u64, Checkpoint>,
    /// Current version counter
    version: u64,
    /// Density parameter
    density: u32,
}
```

### Operations

```rust
impl CheckpointStore {
    fn on_operation(&mut self, rga: &Rga) {
        self.version += 1;
        
        // Add new checkpoint
        self.checkpoints.insert(self.version, Checkpoint::from(rga));
        
        // Remove old checkpoint according to algorithm
        let to_remove = self.version - (first_zero_bit(self.version) << self.density);
        if to_remove > 0 {
            self.checkpoints.remove(&to_remove);
        }
    }
    
    fn at_version(&self, version: u64) -> Option<&Checkpoint> {
        // Find nearest checkpoint <= version
        // Then replay operations from checkpoint to version
        // (This requires also storing the operation log)
    }
}
```

### Trade-offs vs Other Approaches

| Approach | Time per op | Space | Historical access time | Implementation complexity |
|----------|-------------|-------|----------------------|--------------------------|
| Logical filtering | O(1) | O(1) | O(n) | Low |
| Persistent | O(log n) | O(log n) per op | O(log n) | High |
| Checkpoint | O(1)* | O(log n) checkpoints | O(replay) | Medium |

*Checkpoint creation may have O(n) cost for full snapshot, but can be amortized.

## Sources

- [Logarithmically-Spaced Snapshots](https://madebyevan.com/algos/log-spaced-snapshots/)
- [Exponential Backoff - Wikipedia](https://en.wikipedia.org/wiki/Exponential_backoff)
