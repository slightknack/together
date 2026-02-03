# Test Coverage Analysis

## Current Test Inventory

### Unit Tests (src/)
- **rga.rs**: 35 tests covering basic operations (insert, delete, merge, apply, cursor cache, RgaBuf)
- **btree_list.rs**: 14 tests for B-tree weighted list operations
- **op.rs**: 3 tests for OpBlock creation
- **key.rs**: 10 tests for cryptographic operations
- **log.rs**: 15 tests for signed append-only logs

### Integration Tests (tests/)
- **rga_fuzz.rs**: 96 tests including proptests for CRDT properties
- **document_api.rs**: 27 tests for slice, anchor, and version APIs
- **document_api_proptest.rs**: 8 property-based tests for API invariants
- **trace_correctness.rs**: 4 tests comparing against diamond-types on real editing traces

**Total: ~220 tests**

## Coverage Gaps Identified

### Gap 1: Concurrent Editing Traces (HIGH PRIORITY)
The `data/editing-traces/concurrent_traces/` directory contains `clownschool.json.gz` and `friendsforever.json.gz` but they are NOT tested. These are multi-user concurrent editing scenarios that would stress-test the merge algorithm far more than our synthetic tests.

**Missing test**: Replay concurrent traces and verify convergence matches diamond-types.

### Gap 2: Right Origin / Subtree Detection
We just fixed a bug in `insert_span_rga` where the right-split span was incorrectly treated as a sibling. This suggests the sibling detection logic is fragile. There's no systematic test for:
- Deep trees of concurrent insertions (A inserts, B inserts after A, C inserts after B, etc.)
- Subtree boundary detection across multiple generations

**Missing test**: Multi-generational concurrent insert trees.

### Gap 3: Origin Index Consistency
The `origin_index` is used for O(k) sibling lookup during merge, but:
- Index entries can become stale after span removal
- No test verifies the index remains consistent after many operations
- No test for index rebuild or garbage collection

**Missing test**: Verify origin_index consistency after many insert/delete cycles.

### Gap 4: Span Coalescing Edge Cases
Sequential typing should coalesce into single spans, but edge cases may break this:
- Insert, delete last char, insert again (can't coalesce across tombstone)
- Insert by user A, insert by user B at same position, user A continues typing
- Span coalescing after merge

**Missing test**: Span count validation after various editing patterns.

### Gap 5: BTreeList Stress Testing
The BTreeList is critical for O(log n) lookups but:
- No test for very deep trees (height > 3)
- No test for concurrent-like access patterns (random position lookups)
- No test for weight update consistency after many modifications

**Missing test**: BTreeList stress test with 1M+ items and random access.

### Gap 6: Version Snapshot Memory Sharing
Versions use `Arc<Snapshot>` for cheap cloning, but:
- No test verifies actual memory sharing (two versions share spans)
- No test for snapshot isolation (modifying doc doesn't affect old version)

**Missing test**: Memory sharing verification for versions.

### Gap 7: Unicode / Multi-byte Character Handling
Only one test (`slice_unicode`) touches UTF-8, but:
- No test for inserting in middle of UTF-8 sequence (should be byte-level safe)
- No test for slice boundaries landing inside UTF-8 characters
- No test for emoji (4-byte UTF-8 sequences)

**Missing test**: Comprehensive UTF-8 edge cases.

### Gap 8: Pathological User Key Ordering
RGA uses lexicographic key ordering to break ties. No test for:
- Users with very similar keys (differ only in last bytes)
- Maximum number of users (65534 limit)
- User key collision (should be impossible but worth testing)

**Missing test**: Key ordering edge cases.

### Gap 9: Operation Ordering Dependencies
The `apply` function requires sequential seq numbers. No test for:
- Out-of-order operation application (should fail gracefully)
- Gaps in sequence numbers
- Replay from partial operation log

**Missing test**: Operation ordering violation handling.

### Gap 10: Merge with Self-Referential Origins
No test for pathological cases like:
- Span with origin pointing to itself (invalid, should not occur)
- Circular origin chains (A -> B -> A, also invalid)

**Missing test**: Invalid origin detection.

## What's Well Covered

1. **CRDT Properties**: Commutativity, associativity, idempotence of merge
2. **Basic Operations**: Insert, delete, slice at various positions
3. **Anchors**: Position tracking through edits
4. **Versions**: Snapshot creation and reconstruction
5. **OpLog**: Operation serialization and replay
6. **Real Traces**: Sequential editing traces from actual editors
7. **Scale**: 100KB documents, 10000 operations, 100 users

## Hardest Stress Test Design

The ultimate stress test would combine all the gaps into one scenario:

```
Adversarial Concurrent Editing Stress Test
==========================================

Setup:
- 50 users with carefully chosen keys (some nearly identical)
- Each user has their own replica
- 1000 operations per user (50,000 total)

Operations:
- 40% insert at random position (uniform distribution)
- 20% insert at position 0 (maximum conflict)
- 20% insert after previous user's last character (chain formation)
- 15% delete random range (1-10 chars)
- 5% delete at position 0

Sync Pattern:
- After every 100 operations per user, random partial syncs
- Some users sync frequently, others rarely (divergence testing)
- Occasional "star burst" sync (one user syncs with all others)

Verification After Each Sync Round:
1. All synced replicas have identical content
2. Content length equals sum of visible spans
3. Span count is reasonable (not exploding due to fragmentation)
4. Version snapshots from before sync still reconstruct correctly
5. Origin index entries are valid (no stale references)
6. BTreeList invariants hold (weights sum correctly)

Final Verification:
1. Full mesh merge: every replica merges with every other
2. All replicas converge to identical content
3. Compare with diamond-types on equivalent operation sequence
4. Memory usage is within 10x of raw content size
5. Final span count is within 5x of operation count
```

This test would catch:
- Sibling ordering bugs (50 users = many concurrent edits)
- Subtree detection bugs (chain insertions create deep trees)
- Origin index bugs (many syncs = index churn)
- Memory leaks (50,000 operations with versions)
- Performance regressions (must complete in reasonable time)

## Recommendations

1. **Immediate**: Add concurrent trace tests for clownschool/friendsforever
2. **Short-term**: Add multi-generational concurrent insert tests
3. **Medium-term**: Implement the adversarial stress test
4. **Ongoing**: Run proptests with higher case counts in CI (10,000+)
