+++
model = "claude-opus-4-5"
created = 2026-01-31
modified = 2026-01-31
driver = "Isaac Clayton"
+++

# Procedure: Fixing Algorithmic Bugs

This procedure describes how to systematically fix algorithmic bugs through research, hypothesis testing, and controlled exploration. It treats bug fixing as a search problem with checkpoints, backtracking, and knowledge propagation.

## When to Use This Procedure

Use this procedure when:
- A bug resists simple fixes (multiple failed attempts)
- The root cause involves algorithmic correctness, not typos
- You suspect the design itself may be flawed
- You need to explore multiple solution directions

Do not use for:
- Simple bugs with obvious fixes
- Performance issues (use `00-optimize.md`)
- Feature additions (use normal development workflow)

## Philosophy

"I want you to do research, look into other implementations. Don't overcomplicate things. Learn about RGA and crdts. I want you to go on a research arc. Read every major CRDT library. Take notes. Form opinions. Figure out, from first principles, what the best approach is; then, using the pieces we have worked so hard to forge, rebuild the codebase in the correct image of the ideal."
— Driver, on approaching a stubborn bug

The key principles:

1. **Work until the work is done.** Do not give up. Do not declare victory prematurely.

2. **Be systematic, not whack-a-mole.** Develop deep intuition. Triangulate the error. Ladder the king like in chess: methodical progress, not random moves.

3. **Test early and often.** Each test should answer a specific question: "Is it wrong because of X or Y?" Cut the state space drastically at each step.

4. **Know when to backtrack.** Recognize dead ends. Return to the most promising earlier point. Carry forward what you learned about why the dead end failed.

5. **Fix the right layer.** Make the core work before adding complexity. Do not introduce nonlinearities. If the RGA is broken, fix the RGA before testing it through buffered wrappers.

6. **Maintain the solution set.** Track all promising branches. When one fails, pop the next.

7. **Refactor as you work.** Clean code is easier to debug. Split types into their own files. Remove dead code. Make the problem visible.

## Phase 0: Stabilize

Before exploring solutions, ensure you have a stable foundation.

### 0.1 Create a Minimal Reproduction

Find the smallest test case that demonstrates the bug:

```rust
#[test]
fn minimal_repro() {
    // The smallest possible case that fails
}
```

A minimal reproduction:
- Has no unnecessary operations
- Uses deterministic inputs
- Produces consistent failures
- Can run in under 1 second

### 0.2 Capture the Invariant

State precisely what property is violated:

```markdown
## Bug Statement

**Invariant:** merge(A, B) == merge(B, A) (commutativity)

**Violation:** When user1 > user2 lexicographically and both insert 
at the same origin, merge(A, B) produces a different order than merge(B, A).

**Minimal case:** [describe the minimal reproduction]
```

### 0.3 Create a Checkpoint

Commit the current state with a clear message:

```bash
git add -A
git commit -m "checkpoint: before fixing [bug name]"
git tag checkpoint-[bug-name]-start
```

This is your safe return point.

## Phase 1: Research

Before fixing, understand. This phase can be skipped only if you deeply understand the problem domain already.

### 1.1 Study the Problem Domain

Research how others solve this class of problem:

1. Identify major implementations in the same domain
2. Clone them to /tmp and read source code
3. Read academic papers if applicable
4. Document findings in `research/NN-topic.md`

Questions to answer:
- What invariants do correct implementations maintain?
- What data structures do they use?
- What are the known failure modes?
- What are the key differences from our approach?

### 1.2 Identify Root Cause

Based on research, form a hypothesis about the root cause:

```markdown
## Root Cause Hypothesis

**Symptom:** [What we observe]

**Root cause:** [What fundamental issue causes this]

**Evidence:**
1. [Evidence point 1]
2. [Evidence point 2]
3. [How other implementations avoid this]

**Why our current approach fails:**
[Technical explanation]
```

### 1.3 Generate Solution Candidates

Create a prioritized solution set (tree of possible approaches):

```markdown
## Solution Set

### Direction A: [Name]
Approach: [Description]
Confidence: [High/Medium/Low]
Complexity: [Low/Medium/High]
Sub-approaches:
  A1: [Variant 1]
  A2: [Variant 2]

### Direction B: [Name]
...
```

Order by: simplicity first, then confidence. Simple high-confidence solutions should be tried before complex speculative ones.

## Phase 2: Systematic Exploration

Execute the solution set methodically.

### 2.1 Solution Attempt Structure

For each solution attempt:

```markdown
## Attempt N: [Direction.Sub-approach]

### Hypothesis
If we [change], then [expected outcome] because [reasoning].

### Test Design
This hypothesis will be tested by:
1. [Specific test that will pass if hypothesis is correct]
2. [Specific test that will fail if hypothesis is wrong]

### Implementation
[Brief description of changes]

### Result
- [ ] Tests pass
- [ ] Regression tests still pass
- [ ] Proptest finds no violations

### Conclusion
[CONFIRMED/REFUTED]: [What we learned]
```

### 2.2 Checkpoint Discipline

Create checkpoints at meaningful points:

```bash
# Before starting an attempt
git stash  # or commit
git tag attempt-N-start

# If attempt succeeds
git commit -m "fix: [description]"
git tag attempt-N-success

# If attempt fails
git checkout attempt-N-start
git tag attempt-N-failed-[reason]
```

### 2.3 Knowledge Propagation

When backtracking, carry forward lessons:

1. Document why the attempt failed in research notes
2. Update the solution set with new information
3. Prune branches that the failed attempt rules out
4. Add new branches that the attempt revealed

Example:
```markdown
## Attempt 3 Post-Mortem

**What we tried:** [Description]

**Why it failed:** [Technical explanation]

**What this rules out:**
- Solution A2 (same flaw)
- Any approach that [has property X]

**What this suggests:**
- We likely need [property Y]
- Direction C now looks more promising because [reason]
```

### 2.4 Test-Driven Debugging

Each test should ask a specific question:

```rust
#[test]
fn is_it_the_subtree_detection() {
    // Tests subtree detection specifically
    // If this passes but the bug persists, subtree detection is not the cause
}

#[test]
fn is_it_the_sibling_ordering() {
    // Tests sibling ordering specifically
    // If this fails, we've isolated the problem
}
```

Good test design:
- Each test isolates one hypothesis
- Passing/failing cuts the state space in half
- Tests are named as questions

### 2.5 Layered Testing

Test from the bottom up:

1. **Core algorithm:** Does the basic algorithm work in isolation?
2. **Data structure integration:** Does it work with our data structures?
3. **API layer:** Does it work through the public API?
4. **Buffered/optimized layer:** Does it work with optimizations?

Fix failures at lower layers before testing higher layers. A failing core algorithm cannot be fixed by adjusting the buffered layer.

## Phase 3: Implementation

When a solution attempt appears to work:

### 3.1 Verify Thoroughly

Run all levels of testing:

```bash
# Unit tests
cargo test

# Property tests (run longer than usual)
PROPTEST_CASES=10000 cargo test --test rga_fuzz

# Any benchmark tests
cargo test --test integration
```

### 3.2 Refactor for Clarity

Clean up the fix:

1. Remove debug code
2. Add comments explaining non-obvious logic
3. Split large functions if needed
4. Ensure naming is clear

### 3.3 Document the Fix

Update or create documentation:

```markdown
## Fix Summary

**Bug:** [What was broken]

**Root cause:** [Why it was broken]

**Solution:** [What we changed]

**Why this works:** [Technical explanation]

**Testing:** [How we verified correctness]
```

## Phase 4: Completion

### 4.1 Success Criteria

The bug is fixed when:
- [ ] The minimal reproduction passes
- [ ] All existing tests pass
- [ ] Property tests find no violations (many iterations)
- [ ] The fix is understood (can explain why it works)
- [ ] Code is clean and documented

### 4.2 Cleanup

1. Remove failed attempt branches if not useful for history
2. Update any affected documentation
3. Close any related issues
4. Write postmortem if the bug was significant

### 4.3 Capture Lessons

Add to `research/NN-postmortem.md`:

```markdown
## Postmortem: [Bug Name]

### Timeline
- [Date]: Bug discovered
- [Date]: Root cause identified
- [Date]: Fix implemented

### What Went Wrong
[Technical explanation of the bug]

### Why It Wasn't Caught Earlier
[What testing gaps allowed this]

### How We Fixed It
[The solution]

### Lessons Learned
1. [Lesson 1]
2. [Lesson 2]

### Process Improvements
- [ ] Add test for [edge case]
- [ ] Add invariant check for [property]
```

## Appendix: Current Bug Context

This procedure was created for the RGA merge commutativity bug. Here is the context for that specific bug:

### Bug Statement

**Invariant:** merge(A, B) == merge(B, A)

**Violation:** When two users insert at the same origin with different user orderings, the merge result depends on merge order.

### Root Cause (from research)

Our implementation uses **only left origin** to order concurrent inserts. This makes it fundamentally impossible to reliably detect subtree boundaries during merge.

**Evidence from research:**
- Yjs/YATA uses both `origin` (left) AND `rightOrigin` (right)
- Diamond-types confirms: "YjsMod / FugueMax items generate identical merge behaviour"
- Fugue paper: uses leftOrigin and rightOrigin with formal correctness proofs
- Automerge's RGA has similar single-origin limitations

**Why single origin fails:**
When inserting a span among siblings, we must skip past any sibling with higher precedence AND that sibling's entire subtree. With only left origins, we cannot distinguish:
- A descendant of our sibling (should skip)
- A descendant of a different branch (should not skip)

### Solution Set

**Direction A: Add Right Origin (Recommended)**
- A1: Full dual-origin (store both left and right origin on every span)
- A2: Computed right origin (compute right origin during insert, don't store)
Confidence: High (matches all production implementations)
Complexity: Medium

**Direction B: Explicit Tree Structure**
- B1: Maintain actual tree with child pointers
- B2: Compute tree lazily during insert only
Confidence: Medium (Fugue reference does this)
Complexity: High

**Direction C: Alternative Algorithm**
- C1: Switch to pure Fugue (tree-based)
- C2: Switch to pure YATA (Yjs-style)
Confidence: High (proven correct)
Complexity: High (rewrite)

### Implementation Plan

For Direction A1 (recommended):

1. **Span structure changes:**
   ```rust
   struct Span {
       // ... existing fields ...
       
       // Left origin (character we inserted after)
       origin_user_idx: u16,
       origin_seq: u32,
       
       // Right origin (character that was to our right when inserted)
       right_origin_user_idx: u16,  // NEW
       right_origin_seq: u32,       // NEW
   }
   ```

2. **Insert algorithm changes:**
   Replace the "ultra-conservative" subtree detection with proven YATA/FugueMax ordering:
   - Compare tuples: (leftOrigin, rightOrigin, uid)
   - No heuristics needed; right origin acts as boundary marker

3. **Testing strategy:**
   - Unit test: Specific cases that failed before
   - Property test: Commutativity with 10000+ cases
   - Comparison test: Match diamond-types output

### Why This Will Work

The dual-origin approach eliminates the "subtree boundary detection" problem entirely. With right origin:
- We know we've exited a subtree when an item's right origin doesn't match expected
- No heuristics, no edge cases, no "conservative" guessing
- Mathematically proven correct by Fugue paper

All production CRDTs converged on this approach independently:
- Yjs (since 2015)
- Diamond-types (by the author of JSON-OT)
- Automerge (recent versions)
- Fugue (academic proof)

## Appendix: Checklist

### Before Starting
- [ ] Minimal reproduction created
- [ ] Invariant clearly stated
- [ ] Checkpoint created
- [ ] Research complete (if needed)
- [ ] Root cause hypothesized
- [ ] Solution set generated

### For Each Attempt
- [ ] Hypothesis stated
- [ ] Test designed to confirm/refute
- [ ] Checkpoint before implementation
- [ ] Implementation done
- [ ] Tests run
- [ ] Result documented
- [ ] Lessons captured if failed

### Before Completing
- [ ] All tests pass
- [ ] Property tests pass (many iterations)
- [ ] Fix is understood and documented
- [ ] Code is clean
- [ ] Postmortem written (if significant)
