# Procedure: Checkpoint-Based Versioning

## Goal
Implement versioning using periodic checkpoints (snapshots) with geometric retention policy to bound memory usage while maintaining logarithmic access to historical versions.

## Approach
Instead of storing every version or filtering by timestamps, we periodically create full snapshots and use a retention policy to prune old checkpoints while maintaining good historical coverage.

### Key Concepts

1. **Checkpoints**: Full snapshots of the document state (spans + length)
2. **Retention Policy**: Logarithmically-spaced snapshots based on "first zero bit" algorithm
3. **Forward Replay**: For versions between checkpoints, replay operations from nearest checkpoint

### Algorithm (from madebyevan.com)

For snapshot at time `n`, delete snapshot at time `n - (firstZeroBit(n) << d)` where:
- `firstZeroBit(n)` returns the position of the first zero bit (1, 2, 4, 8, ...)
- `d` is a density parameter (higher = more snapshots retained)

This gives us O(log n) snapshots covering the full history with geometric spacing.

Example with d=0:
- After operation 1: keep [1]
- After operation 2: keep [2] (delete 1)
- After operation 3: keep [2, 3]
- After operation 4: keep [4] (delete 2, 3)
- After operation 8: keep [8] (delete all previous)
- After operation 9: keep [8, 9]
- ...

## Implementation Steps

1. Add checkpoint storage: `Vec<(u64, Snapshot)>` (lamport, snapshot)
2. After each edit, apply retention policy
3. Version holds lamport timestamp
4. version() just returns current lamport (no snapshot created)
5. to_string_at() finds nearest checkpoint <= version.lamport, returns that snapshot
6. For simplicity, we'll only support exact checkpoint versions initially

## Trade-offs

**Pros:**
- O(log n) memory for historical versions
- O(1) to create a version (just increment lamport)
- No overhead on current document operations

**Cons:**
- Can only access exact checkpoint versions (no arbitrary version access)
- Accessing old versions requires finding nearest checkpoint
- Slightly more complex than persistent approach

## Simplification for MVP
For fair comparison with other approaches:
- Create checkpoint after every edit (like persistent approach)
- Apply retention policy to bound total checkpoints
- This gives similar semantics but with logarithmic memory

## Testing
- Reuse existing version tests
- Verify retention policy correctly prunes old checkpoints
- Verify O(log n) checkpoint count
