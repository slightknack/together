+++
model = "claude-opus-4-5"
created = 2026-01-31
modified = 2026-01-31
driver = "Isaac Clayton"
+++

# Procedure: Performance Optimization

This procedure describes how to systematically optimize performance through iterative improvements while maintaining correctness. Follow this when the driver requests performance work and provides done criteria.

## Prerequisites

- Done criteria from the driver (e.g., "faster than X on 3/4 benchmarks")
- Working benchmarks that can measure progress
- A passing test suite

## Phase 1: Research

Before writing optimization code, understand the problem space.

### 1.1 Establish Baseline

Run benchmarks and record exact numbers:

```bash
cargo bench --bench <benchmark_name>
```

Document in worklog:

```markdown
## Baseline Benchmarks

| Trace | Our Time | Competitor | Ratio |
|-------|----------|------------|-------|
| trace1 | Xms | Yms | Z.Zx slower |
```

### 1.2 Profile the Code

Identify where time is spent before optimizing:

```bash
# Using flamegraph
cargo install flamegraph
cargo flamegraph --bench <benchmark_name>

# Or using perf (Linux)
perf record --call-graph dwarf cargo bench --bench <benchmark_name>
perf report
```

Document hotspots in research notes.

### 1.3 Study Competing Implementations

Read how others solved the same problem:

1. Clone competing implementations to /tmp
2. Read their core data structures
3. Note key techniques they use
4. Document findings in `research/NN-topic.md`

Questions to answer:
- What data structures do they use?
- What is their algorithmic complexity?
- What constants do they optimize for?
- What tradeoffs did they make?

### 1.4 Generate Optimization Candidates

Create a prioritized list of potential optimizations:

```markdown
## Optimization Candidates

### 1. [Name] (Foundation)
- **Current**: [How it works now]
- **Proposed**: [How it would work]
- **Expected gain**: [X.Xx speedup]
- **Complexity**: [Low/Medium/High]
- **Risk**: [What could go wrong]

### 2. [Name] (Builds on #1)
...
```

Order by foundation: data structure changes before algorithmic tweaks. A better representation makes everything else easier.

### 1.5 Present to Driver

Share the prioritized list with the driver. Wait for approval before implementing. The driver may reorder, remove, or add items.

## Phase 2: Execution

Execute optimizations serially. Each optimization gets its own subagent.

### 2.1 Subagent Prompt Template

For each optimization, spawn a subagent with this structure:

```markdown
You are implementing Optimization N: [Name] for the [project] library.

## Context

Read these files first:
- /path/to/PROCESS.md (coding philosophy)
- /path/to/DESIGN.md (architecture)
- /path/to/research/NN-relevant-topic.md (research)
- /path/to/src/file.rs (code to modify)

## Current State

| Benchmark | Our Time | Competitor | Ratio |
|-----------|----------|------------|-------|
| ... | ... | ... | ... |

## Goal

[Specific, measurable outcome. E.g., "Reduce span size from 112 bytes to 24 bytes"]

## Implementation Plan

1. [Step 1]
2. [Step 2]
3. [Step 3]

## Constraints

- All tests must pass
- Must not regress other benchmarks by more than 5%
- Follow PROCESS.md coding style

## Tasks

1. Read all context files before writing code
2. Implement the optimization
3. Run tests: `cargo test`
4. Run benchmarks: `cargo bench --bench quick_bench`
5. If faster: commit with message "[Name]: Xms -> Yms (Z% speedup)"
6. If slower: commit to branch `ibc/slow-<name>`, document why in research/
7. Report results back including:
   - What was changed
   - Benchmark numbers before/after
   - Any issues encountered
   - Ideas for future optimizations discovered
```

### 2.2 Subagent Execution Rules

The subagent must:

1. **Read before writing.** Read all context files completely before modifying code. Understand existing patterns.

2. **Test after every change.** Run `cargo test` after each modification, not just at the end. Catch regressions early.

3. **Benchmark multiple times.** Run benchmarks at least twice to account for variance. Report the consistent result.

4. **Explain the mechanism.** Commit messages must explain why the optimization works:
   - Good: "Cursor caching: O(1) sequential inserts by remembering last lookup position"
   - Bad: "Add cursor cache"

5. **Document failures.** If the optimization is slower, still commit to a branch and write research notes explaining why. This prevents repeating failed experiments.

6. **Report completely.** Include all requested information in the report. The parent agent needs accurate data to decide next steps.

### 2.3 Parent Agent Responsibilities

After each subagent completes:

1. **Verify reported numbers.** Run benchmarks yourself. Do not blindly trust reports.

2. **Review the diff.** Check that the code follows project conventions. Look for obvious issues.

3. **Update worklog.** Record progress with timestamps:
   ```markdown
   ### Optimization N: [Name]
   
   Status: COMPLETE
   
   Changes:
   - [What changed]
   
   Results:
   | Benchmark | Before | After | Change |
   |-----------|--------|-------|--------|
   | ... | ... | ... | ... |
   
   Commit: [hash]
   ```

4. **Update todo list.** Mark completed items done. Add new ideas discovered. Remove dead ends.

5. **Check done criteria.** If met, stop and summarize. If not, spawn next subagent.

### 2.4 Handling Failures

When an optimization makes things slower:

1. Subagent commits to branch `ibc/slow-<name>`
2. Subagent documents failure in `research/NN-<name>-lessons.md`:
   ```markdown
   # Why [Name] Failed
   
   ## What We Tried
   [Description of the optimization]
   
   ## Results
   [Benchmark numbers showing regression]
   
   ## Why It Failed
   [Technical explanation]
   
   ## Lessons
   [What we learned]
   
   ## Future Alternatives
   [Other approaches that might work]
   ```
3. Parent agent ensures main branch is at the last good commit
4. Parent agent proceeds to next optimization

### 2.5 Handling Partial Success

When an optimization improves some benchmarks but regresses others:

1. Compare against done criteria
2. If acceptable within criteria (e.g., "no worse than 2x on any"), keep it
3. If unacceptable, try to fix the regression or revert
4. Document the tradeoff in worklog

## Phase 3: Quality Assurance

Maintain quality throughout the optimization process.

### 3.1 Correctness Checks

- **Tests must pass.** An optimization that breaks tests is not an optimization.
- **Property tests catch edge cases.** Add proptest/quickcheck tests for invariants.
- **Debug assertions verify invariants.** Use `debug_assert!` for checks that compile out in release.
- **Minimal reproductions for failures.** When a test fails, create the smallest case that reproduces it before fixing.

### 3.2 Code Quality Checks

- **Follow existing patterns.** New code should look like existing code.
- **No unnecessary changes.** Do not refactor unrelated code during optimization.
- **Comments explain why.** Document non-obvious optimizations.
- **No magic numbers.** Use named constants with explanatory comments.

### 3.3 Commit Quality Checks

Each commit must:
- Have a descriptive message with before/after numbers
- Contain exactly one optimization
- Pass all tests
- Not regress benchmarks unacceptably

Bad: "Optimize stuff"
Good: "Compact span structure: 112 bytes -> 24 bytes, 1.34x speedup on sveltecomponent"

## Phase 4: Completion

### 4.1 Success

When done criteria are met:

1. Run full benchmark suite one final time
2. Update worklog with final summary:
   ```markdown
   ## Final Results
   
   | Benchmark | Start | End | Improvement |
   |-----------|-------|-----|-------------|
   | ... | ... | ... | ... |
   
   ## Optimizations Applied
   1. [Name]: [one-line description]
   2. ...
   
   ## Key Insights
   - [What we learned]
   ```
3. Commit worklog and any remaining documentation
4. Report success to driver with summary

### 4.2 Incomplete

When done criteria cannot be met with planned optimizations:

1. Document current state in worklog
2. Write `research/NN-future-optimizations.md` describing:
   - What was achieved
   - Why further improvement is difficult
   - What architectural changes might help
   - Estimated complexity of those changes
3. Report to driver with:
   - Current performance numbers
   - What was tried
   - Recommendations for next steps

The driver decides whether to:
- Revise done criteria
- Approve architectural changes
- Accept current performance

## Appendix: Common Optimizations

### Data Structure Optimizations

| Technique | When to Use | Expected Gain |
|-----------|-------------|---------------|
| Compact structs | Struct > 64 bytes | 1.2-2x from cache |
| Index instead of pointer | Large referenced types | Memory savings |
| Arena allocation | Many small allocations | 1.1-1.3x |
| Pool/freelist | Frequent alloc/dealloc | 1.1-1.2x |

### Algorithmic Optimizations

| Technique | When to Use | Expected Gain |
|-----------|-------------|---------------|
| Caching/memoization | Repeated lookups | 1.5-10x on hot path |
| Batch operations | Many small operations | 1.2-2x |
| Better data structure | Wrong complexity class | 2-100x |
| Incremental update | Full recomputation | 1.5-5x |

### Low-Level Optimizations

| Technique | When to Use | Expected Gain |
|-----------|-------------|---------------|
| Inline hints | Hot small functions | 1.05-1.2x |
| Branch hints | Predictable branches | 1.05-1.1x |
| SIMD | Parallel data operations | 2-8x on vectorizable |
| Prefetch | Sequential access patterns | 1.1-1.3x |

### Quality vs Speed Tradeoffs

| Technique | Tradeoff | When Acceptable |
|-----------|----------|-----------------|
| Unsafe code | Safety for speed | Only if safe alternative measured slower |
| Debug-only checks | Debug slower, release faster | Always acceptable |
| Reduced precision | Accuracy for speed | When precision not critical |

## Appendix: Checklist

### Before Starting
- [ ] Done criteria received from driver
- [ ] Baseline benchmarks recorded
- [ ] Profile data collected
- [ ] Research notes written
- [ ] Optimization list prioritized
- [ ] Driver approved the list

### For Each Optimization
- [ ] Subagent spawned with full context
- [ ] Tests pass after change
- [ ] Benchmarks run multiple times
- [ ] Results verified by parent agent
- [ ] Commit message includes numbers
- [ ] Worklog updated
- [ ] Todo list updated

### Before Completing
- [ ] Final benchmarks match done criteria
- [ ] All tests pass
- [ ] Worklog has final summary
- [ ] Future optimizations documented
- [ ] Driver notified of completion
