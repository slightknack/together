+++
model = "claude-opus-4-5"
created = 2026-02-02
modified = 2026-02-02
driver = "Isaac Clayton"
+++

# Procedure: Systematic Test Coverage Gap Closure

This procedure describes how to systematically identify and close test coverage gaps through targeted test creation. It treats testing as an exploration problem: each test should reveal new information about system behavior.

## When to Use This Procedure

Use this procedure when:
- A coverage analysis has identified gaps
- New features need comprehensive testing
- Bugs suggest missing test scenarios
- Preparing for production readiness

Do not use for:
- Performance testing (use `00-optimize.md`)
- Bug fixing (use `02-fix-algorithm.md`)
- Quick one-off test additions

## Philosophy

"A test that cannot fail is not a test. A test that always fails is not useful. The best tests are those that would catch bugs we haven't written yet."

Key principles:

1. **Each test should have a hypothesis.** Before writing a test, state what you expect and why. A test without a hypothesis is just ceremony.

2. **Tests should be orthogonal.** Each test should exercise a different code path or scenario. Redundant tests waste time and obscure coverage.

3. **Edge cases are more valuable than happy paths.** The center of the input space is usually well-tested. The edges are where bugs hide.

4. **Determinism is non-negotiable.** Flaky tests are worse than no tests. They erode confidence and waste debugging time.

5. **Tests are documentation.** A well-named test explains what the system does. A well-structured test explains how.

6. **Property tests find what you didn't think to test.** When a property test fails, it often reveals an edge case you wouldn't have written manually.

## Phase 0: Gap Analysis

Before writing tests, understand what's missing.

### 0.1 Inventory Existing Tests

Create a map of what's tested:

```markdown
## Test Inventory

### Unit Tests (src/)
- module_a: N tests covering [aspects]
- module_b: M tests covering [aspects]

### Integration Tests (tests/)
- test_file_1.rs: N tests for [scenarios]
- test_file_2.rs: M tests for [scenarios]

### Property Tests
- property_1: [what it verifies]
- property_2: [what it verifies]
```

### 0.2 Identify Coverage Gaps

For each module/feature, ask:
- What inputs are not tested?
- What code paths are not exercised?
- What error conditions are not triggered?
- What combinations are not tried?
- What scale is not reached?

Document gaps with severity:

```markdown
## Coverage Gaps

### Gap 1: [Name] (CRITICAL)
**What's missing:** [Description]
**Why it matters:** [Impact of bugs here]
**Test approach:** [How to test it]

### Gap 2: [Name] (HIGH)
...
```

### 0.3 Prioritize by Risk

Order gaps by:
1. **Likelihood of bugs** - Complex code, recent changes, known issues
2. **Impact of bugs** - Data loss, security, correctness
3. **Difficulty to test** - Easy wins first, unless critical

## Phase 1: Test Design

Design tests before writing them.

### 1.1 Test Specification Template

For each gap, create a specification:

```markdown
## Test: [test_name]

### Hypothesis
If we [action], then [expected outcome] because [reasoning].

### Category
- [ ] Unit test
- [ ] Integration test
- [ ] Property test
- [ ] Stress test

### Inputs
- [Input 1]: [description and range]
- [Input 2]: [description and range]

### Expected Behavior
1. [Step 1 expectation]
2. [Step 2 expectation]
3. [Final state expectation]

### Edge Cases to Cover
- [Edge case 1]
- [Edge case 2]

### Determinism Strategy
[How to ensure the test is deterministic]
```

### 1.2 Property Test Design

For property tests, define:

```markdown
## Property: [property_name]

### Statement
For all [inputs satisfying preconditions], [property] holds.

### Generator Strategy
- Input A: [how to generate]
- Input B: [how to generate]

### Shrinking Hints
- [What minimal case looks like]

### Expected Failure Modes
- [What violations would look like]
```

### 1.3 Stress Test Design

For stress tests, define:

```markdown
## Stress Test: [test_name]

### Scale Parameters
- N = [size/count]
- M = [operations/iterations]

### Resource Limits
- Time: [max duration]
- Memory: [max usage]
- CPU: [expected utilization]

### Success Criteria
- [Criterion 1]
- [Criterion 2]

### Monitoring
- [What to measure during execution]
```

## Phase 2: Implementation

Write tests systematically.

### 2.1 Test File Organization

Organize tests by category:

```
tests/
├── unit/           # Fast, focused tests
├── integration/    # Cross-module tests
├── property/       # Proptest-based tests
├── stress/         # Scale and performance tests
└── regression/     # Bug reproduction tests
```

Or by feature:

```
tests/
├── rga_basic.rs        # Core operations
├── rga_merge.rs        # CRDT merge behavior
├── rga_concurrent.rs   # Concurrent edit scenarios
├── rga_stress.rs       # Scale testing
└── rga_regression.rs   # Bug reproductions
```

### 2.2 Test Implementation Checklist

For each test:

- [ ] Name clearly describes what's tested
- [ ] Setup is minimal and documented
- [ ] Assertions are specific and informative
- [ ] Failure messages explain what went wrong
- [ ] Test is deterministic (no random without seed)
- [ ] Test runs in reasonable time (< 1s for unit, < 60s for integration)
- [ ] Test is independent (no shared state with other tests)

### 2.3 Property Test Implementation

```rust
proptest! {
    #![proptest_config(Config {
        cases: 1000,           // Enough to find edge cases
        max_shrink_iters: 500, // Enough to minimize failures
        timeout: 10000,        // 10s max per case
        ..Config::default()
    })]

    #[test]
    fn property_name(
        input_a in generator_a(),
        input_b in generator_b(),
    ) {
        // Setup
        let mut system = setup();
        
        // Action
        system.do_thing(input_a, input_b);
        
        // Property assertion
        prop_assert!(
            system.invariant_holds(),
            "Invariant violated with inputs: {:?}, {:?}",
            input_a, input_b
        );
    }
}
```

### 2.4 Stress Test Implementation

```rust
#[test]
#[ignore] // Run explicitly with --ignored
fn stress_test_name() {
    let start = Instant::now();
    let mut system = setup();
    
    // Scale parameters
    const NUM_USERS: usize = 50;
    const OPS_PER_USER: usize = 1000;
    
    // Execute at scale
    for round in 0..OPS_PER_USER {
        for user in 0..NUM_USERS {
            system.do_operation(user, round);
        }
        
        // Periodic verification
        if round % 100 == 0 {
            assert!(system.invariants_hold(), "Failed at round {}", round);
        }
    }
    
    // Final verification
    assert!(system.final_state_valid());
    
    // Performance bounds
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(60), "Too slow: {:?}", elapsed);
}
```

## Phase 3: Verification

Ensure tests are effective.

### 3.1 Run All Tests

```bash
# Unit and integration tests
cargo test

# Property tests with more cases
PROPTEST_CASES=10000 cargo test

# Stress tests
cargo test --ignored
```

### 3.2 Verify Coverage Improvement

If using coverage tools:

```bash
cargo tarpaulin --out html
# Review coverage report
```

### 3.3 Mutation Testing (Optional)

Verify tests catch bugs:

```bash
cargo mutants
# Check that tests catch most mutations
```

### 3.4 Test Quality Checklist

- [ ] All new tests pass
- [ ] No existing tests broken
- [ ] No flaky tests introduced
- [ ] Tests run in reasonable time
- [ ] Coverage gaps are closed
- [ ] Tests document expected behavior

## Phase 4: Documentation

Document what was tested and why.

### 4.1 Update Test Documentation

Add comments explaining test strategy:

```rust
// =============================================================================
// Gap 3: Origin Index Consistency
// =============================================================================
//
// The origin_index maps (user_idx, seq) to span indices for O(k) sibling lookup.
// These tests verify the index remains consistent after various operations.
//
// Coverage:
// - Index populated on insert
// - Index entries valid after span splits
// - Stale entries handled gracefully
// - Index correct after merge

#[test]
fn test_origin_index_populated_on_insert() { ... }
```

### 4.2 Update Coverage Analysis

Update the coverage analysis document:

```markdown
## Gap 3: Origin Index Consistency

**Status:** CLOSED

**Tests Added:**
- test_origin_index_populated_on_insert
- test_origin_index_after_split
- test_origin_index_stale_entries
- test_origin_index_after_merge

**Remaining Concerns:**
- [Any edge cases still not covered]
```

## Appendix: Test Patterns

### Pattern: Deterministic Randomness

```rust
#[test]
fn test_with_seeded_random() {
    // Fixed seed for reproducibility
    let seed = 12345u64;
    let mut rng = StdRng::seed_from_u64(seed);
    
    for _ in 0..1000 {
        let value = rng.gen_range(0..100);
        // Test with value
    }
}
```

### Pattern: Comparing Against Reference

```rust
#[test]
fn test_matches_reference_implementation() {
    let input = generate_input();
    
    let our_result = our_implementation(input);
    let reference_result = reference_implementation(input);
    
    assert_eq!(our_result, reference_result);
}
```

### Pattern: Invariant Checking

```rust
fn check_invariants(system: &System) -> Result<(), String> {
    if !system.invariant_1() {
        return Err("Invariant 1 violated".to_string());
    }
    if !system.invariant_2() {
        return Err("Invariant 2 violated".to_string());
    }
    Ok(())
}

#[test]
fn test_invariants_maintained() {
    let mut system = System::new();
    
    for op in operations {
        system.apply(op);
        check_invariants(&system).unwrap();
    }
}
```

### Pattern: Concurrent Scenario

```rust
#[test]
fn test_concurrent_scenario() {
    // Setup independent replicas
    let mut replicas: Vec<_> = (0..N).map(|_| System::new()).collect();
    
    // Each replica makes independent edits
    for (i, replica) in replicas.iter_mut().enumerate() {
        replica.edit(i);
    }
    
    // Merge all replicas
    for i in 0..N {
        for j in 0..N {
            if i != j {
                let other = replicas[j].clone();
                replicas[i].merge(&other);
            }
        }
    }
    
    // All should converge
    let first = replicas[0].state();
    for replica in &replicas[1..] {
        assert_eq!(replica.state(), first);
    }
}
```

## Appendix: Checklist

### Before Starting
- [ ] Coverage analysis complete
- [ ] Gaps prioritized by risk
- [ ] Test specifications written

### For Each Gap
- [ ] Test hypothesis stated
- [ ] Test implemented
- [ ] Test passes
- [ ] Test is deterministic
- [ ] Test is documented

### Before Completing
- [ ] All gaps addressed
- [ ] All tests pass
- [ ] No flaky tests
- [ ] Coverage analysis updated
- [ ] Documentation updated
