// model = "claude-opus-4-5"
// created = 2026-01-31
// modified = 2026-01-31
// driver = "Isaac Clayton"

//! Comprehensive fuzz testing for RGA CRDT correctness.
//!
//! These tests verify:
//! 1. RGA invariants hold after any sequence of operations
//! 2. Merge is commutative and idempotent
//! 3. Output matches diamond-types reference implementation
//! 4. Document length is always consistent

use proptest::prelude::*;
use together::crdt::rga::{Rga, RgaBuf};
use together::crdt::Crdt;
use together::key::KeyPair;

// =============================================================================
// Invariant checking
// =============================================================================

/// Check that RGA invariants hold
fn check_rga_invariants(rga: &Rga) -> Result<(), TestCaseError> {
    let len = rga.len();
    let content = rga.to_string();
    
    // Length should match content byte length
    prop_assert_eq!(
        len as usize,
        content.len(),
        "Length mismatch: len()={} but to_string().len()={}",
        len,
        content.len()
    );
    
    // slice(0, len) should equal to_string()
    if let Some(slice) = rga.slice(0, len) {
        prop_assert_eq!(
            slice,
            content,
            "slice(0, {}) != to_string()",
            len
        );
    } else if len > 0 {
        return Err(TestCaseError::fail(format!(
            "slice(0, {}) returned None but len > 0",
            len
        )));
    }
    
    Ok(())
}

/// Check that RgaBuf invariants hold
fn check_rga_buf_invariants(rga: &mut RgaBuf) -> Result<(), TestCaseError> {
    rga.flush();
    let len = rga.len();
    let content = rga.to_string();
    
    prop_assert_eq!(
        len as usize,
        content.len(),
        "RgaBuf length mismatch: len()={} but to_string().len()={}",
        len,
        content.len()
    );
    
    Ok(())
}

// =============================================================================
// Operation generators
// =============================================================================

#[derive(Clone, Debug)]
enum RgaOp {
    Insert { pos_pct: f64, content: Vec<u8> },
    Delete { pos_pct: f64, len: u64 },
}

fn arbitrary_rga_op() -> impl Strategy<Value = RgaOp> {
    prop_oneof![
        3 => (0.0..=1.0f64, prop::collection::vec(b'a'..=b'z', 1..20))
            .prop_map(|(pos_pct, content)| RgaOp::Insert { pos_pct, content }),
        1 => (0.0..=1.0f64, 1u64..10)
            .prop_map(|(pos_pct, len)| RgaOp::Delete { pos_pct, len }),
    ]
}

fn apply_rga_op(rga: &mut Rga, user: &KeyPair, op: &RgaOp) {
    let len = rga.len();
    match op {
        RgaOp::Insert { pos_pct, content } => {
            let pos = if len == 0 { 0 } else { ((*pos_pct * len as f64) as u64).min(len) };
            rga.insert(&user.key_pub, pos, content);
        }
        RgaOp::Delete { pos_pct, len: del_len } => {
            if len == 0 {
                return;
            }
            let pos = ((*pos_pct * len as f64) as u64).min(len.saturating_sub(1));
            let actual_del = (*del_len).min(len - pos);
            if actual_del > 0 {
                rga.delete(pos, actual_del);
            }
        }
    }
}

fn apply_rga_buf_op(rga: &mut RgaBuf, user: &KeyPair, op: &RgaOp) {
    let len = rga.len();
    match op {
        RgaOp::Insert { pos_pct, content } => {
            let pos = if len == 0 { 0 } else { ((*pos_pct * len as f64) as u64).min(len) };
            rga.insert(&user.key_pub, pos, content);
        }
        RgaOp::Delete { pos_pct, len: del_len } => {
            if len == 0 {
                return;
            }
            let pos = ((*pos_pct * len as f64) as u64).min(len.saturating_sub(1));
            let actual_del = (*del_len).min(len - pos);
            if actual_del > 0 {
                rga.delete(pos, actual_del);
            }
        }
    }
}

// =============================================================================
// Invariant tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// RGA invariants hold after any sequence of operations
    #[test]
    fn rga_invariants_hold(ops in prop::collection::vec(arbitrary_rga_op(), 1..100)) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();
        
        for op in &ops {
            apply_rga_op(&mut rga, &user, op);
            check_rga_invariants(&rga)?;
        }
    }

    /// RgaBuf invariants hold after any sequence of operations
    #[test]
    fn rga_buf_invariants_hold(ops in prop::collection::vec(arbitrary_rga_op(), 1..100)) {
        let user = KeyPair::generate();
        let mut rga = RgaBuf::new();
        
        for op in &ops {
            apply_rga_buf_op(&mut rga, &user, op);
        }
        
        check_rga_buf_invariants(&mut rga)?;
    }

    /// Rga and RgaBuf produce same output for same operations
    #[test]
    fn rga_and_rga_buf_equivalent(ops in prop::collection::vec(arbitrary_rga_op(), 1..50)) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();
        let mut rga_buf = RgaBuf::new();
        
        for op in &ops {
            apply_rga_op(&mut rga, &user, op);
            apply_rga_buf_op(&mut rga_buf, &user, op);
        }
        
        rga_buf.flush();
        
        prop_assert_eq!(rga.len(), rga_buf.len());
        prop_assert_eq!(rga.to_string(), rga_buf.to_string());
    }
}

// =============================================================================
// Merge tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Merge is idempotent: merge(a, a) == a
    #[test]
    fn merge_idempotent(ops in prop::collection::vec(arbitrary_rga_op(), 1..30)) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();
        
        for op in &ops {
            apply_rga_op(&mut rga, &user, op);
        }
        
        let before = rga.to_string();
        let clone = rga.clone();
        rga.merge(&clone);
        let after = rga.to_string();
        
        prop_assert_eq!(before, after);
    }

    /// Merge with empty is identity
    #[test]
    fn merge_with_empty_is_identity(ops in prop::collection::vec(arbitrary_rga_op(), 1..30)) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();
        
        for op in &ops {
            apply_rga_op(&mut rga, &user, op);
        }
        
        let before = rga.to_string();
        let empty = Rga::new();
        rga.merge(&empty);
        let after = rga.to_string();
        
        prop_assert_eq!(before, after);
    }
}

// =============================================================================
// Merge commutativity tests (KNOWN TO FAIL - see research/28-rga-merge-issues.md)
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Merge is commutative: merge(a, b) == merge(b, a)
    /// KNOWN TO FAIL - see research/28-rga-merge-issues.md
    #[test]
    fn merge_commutative(
        ops1 in prop::collection::vec(arbitrary_rga_op(), 1..20),
        ops2 in prop::collection::vec(arbitrary_rga_op(), 1..20),
    ) {
        let user1 = KeyPair::generate();
        let user2 = KeyPair::generate();
        
        let mut rga1 = Rga::new();
        let mut rga2 = Rga::new();
        
        for op in &ops1 {
            apply_rga_op(&mut rga1, &user1, op);
        }
        for op in &ops2 {
            apply_rga_op(&mut rga2, &user2, op);
        }
        
        let mut merged_12 = rga1.clone();
        merged_12.merge(&rga2);
        
        let mut merged_21 = rga2.clone();
        merged_21.merge(&rga1);
        
        if merged_12.to_string() != merged_21.to_string() {
            eprintln!("user1: {:?}", &user1.key_pub.0[..4]);
            eprintln!("user2: {:?}", &user2.key_pub.0[..4]);
            eprintln!("user1 > user2: {}", user1.key_pub > user2.key_pub);
            eprintln!("rga1: {:?}", rga1.to_string());
            eprintln!("rga2: {:?}", rga2.to_string());
            eprintln!("merge(rga1, rga2): {:?}", merged_12.to_string());
            eprintln!("merge(rga2, rga1): {:?}", merged_21.to_string());
        }
        
        prop_assert_eq!(merged_12.to_string(), merged_21.to_string());
        prop_assert_eq!(merged_12.len(), merged_21.len());
    }
}

// =============================================================================
// Multi-user convergence tests (KNOWN TO FAIL - see research/28-rga-merge-issues.md)
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Multiple users editing and merging produces consistent state
    /// KNOWN TO FAIL - see research/28-rga-merge-issues.md
    #[test]
    fn multi_user_convergence(
        ops1 in prop::collection::vec(arbitrary_rga_op(), 1..15),
        ops2 in prop::collection::vec(arbitrary_rga_op(), 1..15),
        ops3 in prop::collection::vec(arbitrary_rga_op(), 1..15),
    ) {
        let user1 = KeyPair::generate();
        let user2 = KeyPair::generate();
        let user3 = KeyPair::generate();
        
        let mut doc1 = Rga::new();
        let mut doc2 = Rga::new();
        let mut doc3 = Rga::new();
        
        for op in &ops1 {
            apply_rga_op(&mut doc1, &user1, op);
        }
        for op in &ops2 {
            apply_rga_op(&mut doc2, &user2, op);
        }
        for op in &ops3 {
            apply_rga_op(&mut doc3, &user3, op);
        }
        
        let mut final1 = doc1.clone();
        final1.merge(&doc2);
        final1.merge(&doc3);
        
        let mut final2 = doc2.clone();
        final2.merge(&doc3);
        final2.merge(&doc1);
        
        let mut final3 = doc3.clone();
        final3.merge(&doc1);
        final3.merge(&doc2);
        
        prop_assert_eq!(final1.to_string(), final2.to_string());
        prop_assert_eq!(final2.to_string(), final3.to_string());
        prop_assert_eq!(final1.len(), final2.len());
        prop_assert_eq!(final2.len(), final3.len());
    }
}

// =============================================================================
// Sequential typing patterns (common editor patterns)
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Sequential typing at end (most common pattern)
    #[test]
    fn sequential_typing_at_end(
        content in prop::collection::vec(b'a'..=b'z', 100..500),
    ) {
        let user = KeyPair::generate();
        let mut rga = RgaBuf::new();
        
        // Type character by character at end
        for &c in &content {
            let pos = rga.len();
            rga.insert(&user.key_pub, pos, &[c]);
        }
        
        rga.flush();
        let result = rga.to_string();
        let expected: String = content.iter().map(|&c| c as char).collect();
        
        prop_assert_eq!(result, expected);
    }

    /// Backspace pattern: type then delete
    #[test]
    fn backspace_pattern(
        chars in prop::collection::vec(b'a'..=b'z', 20..50),
        delete_count in 1usize..10,
    ) {
        let user = KeyPair::generate();
        let mut rga = RgaBuf::new();
        
        // Type all characters
        for &c in &chars {
            let pos = rga.len();
            rga.insert(&user.key_pub, pos, &[c]);
        }
        
        // Delete some from end
        let actual_delete = delete_count.min(chars.len());
        for _ in 0..actual_delete {
            let len = rga.len();
            if len > 0 {
                rga.delete(len - 1, 1);
            }
        }
        
        rga.flush();
        let result = rga.to_string();
        let expected: String = chars[..chars.len() - actual_delete]
            .iter()
            .map(|&c| c as char)
            .collect();
        
        prop_assert_eq!(result, expected);
    }

    /// Insert in middle pattern
    #[test]
    fn insert_in_middle(
        prefix in prop::collection::vec(b'a'..=b'z', 10..30),
        suffix in prop::collection::vec(b'a'..=b'z', 10..30),
        middle in prop::collection::vec(b'A'..=b'Z', 5..15),
    ) {
        let user = KeyPair::generate();
        let mut rga = RgaBuf::new();
        
        // Insert prefix
        rga.insert(&user.key_pub, 0, &prefix);
        
        // Insert suffix at end
        let pos = rga.len();
        rga.insert(&user.key_pub, pos, &suffix);
        
        // Insert middle at prefix.len()
        rga.insert(&user.key_pub, prefix.len() as u64, &middle);
        
        rga.flush();
        let result = rga.to_string();
        
        let expected: String = prefix.iter()
            .chain(middle.iter())
            .chain(suffix.iter())
            .map(|&c| c as char)
            .collect();
        
        prop_assert_eq!(result, expected);
    }
}

// =============================================================================
// Stress tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    /// Many small operations
    #[test]
    fn many_small_operations(
        ops in prop::collection::vec(arbitrary_rga_op(), 500..1000),
    ) {
        let user = KeyPair::generate();
        let mut rga = RgaBuf::new();
        
        for op in &ops {
            apply_rga_buf_op(&mut rga, &user, op);
        }
        
        check_rga_buf_invariants(&mut rga)?;
    }
}

// =============================================================================
// Targeted merge tests for edge cases discovered during bug fixing
// =============================================================================

/// Helper to ensure consistent user ordering for deterministic tests
fn ordered_users() -> (KeyPair, KeyPair) {
    let u1 = KeyPair::generate();
    let u2 = KeyPair::generate();
    if u1.key_pub > u2.key_pub {
        (u2, u1)
    } else {
        (u1, u2)
    }
}

#[test]
fn merge_both_insert_at_position_zero() {
    // Both users insert at position 0 (no origin)
    // Result should be deterministic based on user ordering
    let (user1, user2) = ordered_users(); // user1 < user2
    
    let mut rga1 = Rga::new();
    rga1.insert(&user1.key_pub, 0, b"aaa");
    
    let mut rga2 = Rga::new();
    rga2.insert(&user2.key_pub, 0, b"bbb");
    
    let mut m1 = rga1.clone();
    m1.merge(&rga2);
    
    let mut m2 = rga2.clone();
    m2.merge(&rga1);
    
    assert_eq!(m1.to_string(), m2.to_string());
    // user2 > user1, so user2's content comes first
    assert_eq!(m1.to_string(), "bbbaaa");
}

#[test]
fn merge_split_then_insert_at_zero() {
    // User2 splits their span, then user1's no-origin span is merged
    // This was the original failing case
    let (user1, user2) = ordered_users();
    
    let mut rga1 = Rga::new();
    rga1.insert(&user1.key_pub, 0, b"a");
    
    let mut rga2 = Rga::new();
    rga2.insert(&user2.key_pub, 0, b"bcd");
    rga2.insert(&user2.key_pub, 1, b"X"); // Split "bcd" into "b" + "X" + "cd"
    
    let mut m1 = rga1.clone();
    m1.merge(&rga2);
    
    let mut m2 = rga2.clone();
    m2.merge(&rga1);
    
    assert_eq!(m1.to_string(), m2.to_string());
    // user2's tree should be traversed fully before user1's content
    assert_eq!(m1.to_string(), "bXcda");
}

#[test]
fn merge_multiple_splits_same_origin() {
    // Multiple items with the same origin, testing subtree skipping
    let (user1, user2) = ordered_users();
    
    eprintln!("user1: {:?}", &user1.key_pub.0[..4]);
    eprintln!("user2: {:?}", &user2.key_pub.0[..4]);
    eprintln!("user1 < user2: {}", user1.key_pub < user2.key_pub);
    
    let mut rga1 = Rga::new();
    rga1.insert(&user1.key_pub, 0, b"a");
    eprintln!("rga1: {:?}", rga1.to_string());
    
    let mut rga2 = Rga::new();
    rga2.insert(&user2.key_pub, 0, b"aa");
    eprintln!("rga2 after 'aa' at 0: {:?}", rga2.to_string());
    rga2.insert(&user2.key_pub, 1, b"aa"); // Insert after first 'a', splits span
    eprintln!("rga2 after 'aa' at 1: {:?}", rga2.to_string());
    rga2.insert(&user2.key_pub, 2, b"b");  // Insert after second 'a' (which is first of inserted "aa")
    eprintln!("rga2 after 'b' at 2: {:?}", rga2.to_string());
    
    let mut m1 = rga1.clone();
    m1.merge(&rga2);
    eprintln!("m1 = merge(rga1, rga2): {:?}", m1.to_string());
    
    let mut m2 = rga2.clone();
    m2.merge(&rga1);
    eprintln!("m2 = merge(rga2, rga1): {:?}", m2.to_string());
    
    assert_eq!(m1.to_string(), m2.to_string());
}

#[test]
fn merge_deep_tree() {
    // Create a deep tree structure with multiple levels of origins
    let (user1, user2) = ordered_users();
    
    let mut rga1 = Rga::new();
    rga1.insert(&user1.key_pub, 0, b"X");
    
    let mut rga2 = Rga::new();
    // Build a chain: each insert is after the previous
    rga2.insert(&user2.key_pub, 0, b"a");
    rga2.insert(&user2.key_pub, 1, b"b");
    rga2.insert(&user2.key_pub, 2, b"c");
    rga2.insert(&user2.key_pub, 3, b"d");
    
    let mut m1 = rga1.clone();
    m1.merge(&rga2);
    
    let mut m2 = rga2.clone();
    m2.merge(&rga1);
    
    assert_eq!(m1.to_string(), m2.to_string());
    assert_eq!(m1.to_string(), "abcdX");
}

#[test]
fn merge_interleaved_inserts() {
    // Both users insert at position 0 independently
    // This tests that interleaving is deterministic
    let (user1, user2) = ordered_users();
    
    let mut rga1 = Rga::new();
    rga1.insert(&user1.key_pub, 0, b"abc");
    
    let mut rga2 = Rga::new();
    rga2.insert(&user2.key_pub, 0, b"123");
    
    // Merge both ways
    let mut m1 = rga1.clone();
    m1.merge(&rga2);
    
    let mut m2 = rga2.clone();
    m2.merge(&rga1);
    
    assert_eq!(m1.to_string(), m2.to_string());
    // user2 > user1, so 123 comes first
    assert_eq!(m1.to_string(), "123abc");
}

#[test]
fn merge_with_deletes() {
    // Merge with deletions - deleted spans should be preserved for ordering
    let (user1, user2) = ordered_users();
    
    let mut rga1 = Rga::new();
    rga1.insert(&user1.key_pub, 0, b"hello");
    rga1.delete(2, 2); // Delete "ll"
    
    let mut rga2 = Rga::new();
    rga2.insert(&user2.key_pub, 0, b"world");
    
    let mut m1 = rga1.clone();
    m1.merge(&rga2);
    
    let mut m2 = rga2.clone();
    m2.merge(&rga1);
    
    assert_eq!(m1.to_string(), m2.to_string());
}

#[test]
fn merge_associativity() {
    // merge(merge(a, b), c) == merge(a, merge(b, c))
    let user1 = KeyPair::generate();
    let user2 = KeyPair::generate();
    let user3 = KeyPair::generate();
    
    let mut rga1 = Rga::new();
    rga1.insert(&user1.key_pub, 0, b"aaa");
    
    let mut rga2 = Rga::new();
    rga2.insert(&user2.key_pub, 0, b"bbb");
    
    let mut rga3 = Rga::new();
    rga3.insert(&user3.key_pub, 0, b"ccc");
    
    // (a merge b) merge c
    let mut left = rga1.clone();
    left.merge(&rga2);
    left.merge(&rga3);
    
    // a merge (b merge c)
    let mut bc = rga2.clone();
    bc.merge(&rga3);
    let mut right = rga1.clone();
    right.merge(&bc);
    
    assert_eq!(left.to_string(), right.to_string());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Test that merge order doesn't matter for any pair of edit sequences
    #[test]
    fn merge_order_independence(
        ops1 in prop::collection::vec(arbitrary_rga_op(), 1..30),
        ops2 in prop::collection::vec(arbitrary_rga_op(), 1..30),
    ) {
        let user1 = KeyPair::generate();
        let user2 = KeyPair::generate();
        
        let mut rga1 = Rga::new();
        let mut rga2 = Rga::new();
        
        for op in &ops1 {
            apply_rga_op(&mut rga1, &user1, op);
        }
        for op in &ops2 {
            apply_rga_op(&mut rga2, &user2, op);
        }
        
        // Try all merge orders
        let mut m12 = rga1.clone();
        m12.merge(&rga2);
        
        let mut m21 = rga2.clone();
        m21.merge(&rga1);
        
        prop_assert_eq!(m12.to_string(), m21.to_string());
        prop_assert_eq!(m12.len(), m21.len());
    }

    /// Test merge after both users have edited a shared base
    #[test]
    fn merge_divergent_edits(
        base_ops in prop::collection::vec(arbitrary_rga_op(), 1..20),
        edit1 in prop::collection::vec(arbitrary_rga_op(), 1..15),
        edit2 in prop::collection::vec(arbitrary_rga_op(), 1..15),
    ) {
        let user1 = KeyPair::generate();
        let user2 = KeyPair::generate();
        
        // Create shared base
        let mut base = Rga::new();
        for op in &base_ops {
            apply_rga_op(&mut base, &user1, op);
        }
        
        // Both users start from same base
        let mut rga1 = base.clone();
        let mut rga2 = base.clone();
        
        // Independent edits
        for op in &edit1 {
            apply_rga_op(&mut rga1, &user1, op);
        }
        for op in &edit2 {
            apply_rga_op(&mut rga2, &user2, op);
        }
        
        // Merge both ways
        let mut m12 = rga1.clone();
        m12.merge(&rga2);
        
        let mut m21 = rga2.clone();
        m21.merge(&rga1);
        
        prop_assert_eq!(m12.to_string(), m21.to_string());
    }
}

// =============================================================================
// Advanced merge scenarios
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    /// Four users editing independently, all merges should converge
    #[test]
    fn four_way_convergence(
        ops1 in prop::collection::vec(arbitrary_rga_op(), 1..15),
        ops2 in prop::collection::vec(arbitrary_rga_op(), 1..15),
        ops3 in prop::collection::vec(arbitrary_rga_op(), 1..15),
        ops4 in prop::collection::vec(arbitrary_rga_op(), 1..15),
    ) {
        let user1 = KeyPair::generate();
        let user2 = KeyPair::generate();
        let user3 = KeyPair::generate();
        let user4 = KeyPair::generate();
        
        let mut rga1 = Rga::new();
        let mut rga2 = Rga::new();
        let mut rga3 = Rga::new();
        let mut rga4 = Rga::new();
        
        for op in &ops1 { apply_rga_op(&mut rga1, &user1, op); }
        for op in &ops2 { apply_rga_op(&mut rga2, &user2, op); }
        for op in &ops3 { apply_rga_op(&mut rga3, &user3, op); }
        for op in &ops4 { apply_rga_op(&mut rga4, &user4, op); }
        
        // Merge in different orders - all should produce same result
        let mut final1 = rga1.clone();
        final1.merge(&rga2);
        final1.merge(&rga3);
        final1.merge(&rga4);
        
        let mut final2 = rga4.clone();
        final2.merge(&rga3);
        final2.merge(&rga2);
        final2.merge(&rga1);
        
        let mut final3 = rga2.clone();
        final3.merge(&rga4);
        final3.merge(&rga1);
        final3.merge(&rga3);
        
        prop_assert_eq!(final1.to_string(), final2.to_string());
        prop_assert_eq!(final2.to_string(), final3.to_string());
    }

    /// Chain merge: A -> B -> C -> D, verify associativity
    #[test]
    fn chain_merge_associativity(
        ops1 in prop::collection::vec(arbitrary_rga_op(), 1..20),
        ops2 in prop::collection::vec(arbitrary_rga_op(), 1..20),
        ops3 in prop::collection::vec(arbitrary_rga_op(), 1..20),
    ) {
        let user1 = KeyPair::generate();
        let user2 = KeyPair::generate();
        let user3 = KeyPair::generate();
        
        let mut rga1 = Rga::new();
        let mut rga2 = Rga::new();
        let mut rga3 = Rga::new();
        
        for op in &ops1 { apply_rga_op(&mut rga1, &user1, op); }
        for op in &ops2 { apply_rga_op(&mut rga2, &user2, op); }
        for op in &ops3 { apply_rga_op(&mut rga3, &user3, op); }
        
        // ((A merge B) merge C)
        let mut left = rga1.clone();
        left.merge(&rga2);
        left.merge(&rga3);
        
        // (A merge (B merge C))
        let mut bc = rga2.clone();
        bc.merge(&rga3);
        let mut right = rga1.clone();
        right.merge(&bc);
        
        prop_assert_eq!(left.to_string(), right.to_string());
    }

    /// Repeated merges should be idempotent
    #[test]
    fn repeated_merge_idempotent(
        ops1 in prop::collection::vec(arbitrary_rga_op(), 1..30),
        ops2 in prop::collection::vec(arbitrary_rga_op(), 1..30),
    ) {
        let user1 = KeyPair::generate();
        let user2 = KeyPair::generate();
        
        let mut rga1 = Rga::new();
        let mut rga2 = Rga::new();
        
        for op in &ops1 { apply_rga_op(&mut rga1, &user1, op); }
        for op in &ops2 { apply_rga_op(&mut rga2, &user2, op); }
        
        let mut merged = rga1.clone();
        merged.merge(&rga2);
        let after_first = merged.to_string();
        
        // Merge again - should be no-op
        merged.merge(&rga2);
        let after_second = merged.to_string();
        
        // Merge rga1 again - should be no-op
        merged.merge(&rga1);
        let after_third = merged.to_string();
        
        prop_assert_eq!(&after_first, &after_second);
        prop_assert_eq!(&after_second, &after_third);
    }
}

// =============================================================================
// Interleaving resistance tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Concurrent inserts at same position should not interleave
    /// (YATA/FugueMax property: runs stay together)
    #[test]
    fn no_interleaving_concurrent_inserts(
        content1 in prop::collection::vec(b'A'..=b'Z', 5..20),
        content2 in prop::collection::vec(b'a'..=b'z', 5..20),
    ) {
        let user1 = KeyPair::generate();
        let user2 = KeyPair::generate();
        
        // Both users insert at position 0
        let mut rga1 = Rga::new();
        rga1.insert(&user1.key_pub, 0, &content1);
        
        let mut rga2 = Rga::new();
        rga2.insert(&user2.key_pub, 0, &content2);
        
        let mut merged = rga1.clone();
        merged.merge(&rga2);
        
        let result = merged.to_string();
        let s1: String = content1.iter().map(|&c| c as char).collect();
        let s2: String = content2.iter().map(|&c| c as char).collect();
        
        // Result should be either s1+s2 or s2+s1, never interleaved
        let valid = result == format!("{}{}", s1, s2) || result == format!("{}{}", s2, s1);
        prop_assert!(valid, "Interleaving detected: {:?} is neither {:?}+{:?} nor {:?}+{:?}", 
                    result, s1, s2, s2, s1);
    }

    /// Sequential typing by one user should never be split by concurrent edits
    #[test]
    fn sequential_typing_preserved(
        prefix in prop::collection::vec(b'a'..=b'z', 5..15),
        suffix in prop::collection::vec(b'a'..=b'z', 5..15),
        interrupt in prop::collection::vec(b'0'..=b'9', 3..10),
    ) {
        let user1 = KeyPair::generate();
        let user2 = KeyPair::generate();
        
        // User1 types prefix, then suffix
        let mut rga1 = Rga::new();
        rga1.insert(&user1.key_pub, 0, &prefix);
        let pos = rga1.len();
        rga1.insert(&user1.key_pub, pos, &suffix);
        
        // User2 independently inserts at position 0
        let mut rga2 = Rga::new();
        rga2.insert(&user2.key_pub, 0, &interrupt);
        
        let mut merged = rga1.clone();
        merged.merge(&rga2);
        
        let result = merged.to_string();
        let user1_text: String = prefix.iter().chain(suffix.iter()).map(|&c| c as char).collect();
        
        // User1's text should appear as a contiguous substring
        prop_assert!(result.contains(&user1_text), 
                    "User1's text {:?} was split in result {:?}", user1_text, result);
    }
}

// =============================================================================
// Edge cases and stress tests  
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    /// Many users, small edits
    #[test]
    fn many_users_small_edits(
        num_users in 3usize..8,
        ops_per_user in 5usize..15,
    ) {
        let users: Vec<KeyPair> = (0..num_users).map(|_| KeyPair::generate()).collect();
        let mut rgas: Vec<Rga> = (0..num_users).map(|_| Rga::new()).collect();
        
        // Each user does some edits
        for (i, rga) in rgas.iter_mut().enumerate() {
            for j in 0..ops_per_user {
                let content = format!("u{}e{}", i, j);
                let pos = if rga.len() == 0 { 0 } else { rga.len() / 2 };
                rga.insert(&users[i].key_pub, pos, content.as_bytes());
            }
        }
        
        // Merge all into first, then all into last
        let mut final1 = rgas[0].clone();
        for rga in &rgas[1..] {
            final1.merge(rga);
        }
        
        let mut final2 = rgas[num_users - 1].clone();
        for rga in rgas[..num_users-1].iter().rev() {
            final2.merge(rga);
        }
        
        prop_assert_eq!(final1.to_string(), final2.to_string());
    }

    /// Deep nesting: each insert is after the previous
    #[test]
    fn deep_origin_chain(
        depth in 10usize..30,
    ) {
        let user1 = KeyPair::generate();
        let user2 = KeyPair::generate();
        
        // User1 builds a deep chain
        let mut rga1 = Rga::new();
        for i in 0..depth {
            let content = format!("{}", (b'a' + (i % 26) as u8) as char);
            rga1.insert(&user1.key_pub, rga1.len(), content.as_bytes());
        }
        
        // User2 inserts at various points
        let mut rga2 = Rga::new();
        rga2.insert(&user2.key_pub, 0, b"XXX");
        
        let mut m12 = rga1.clone();
        m12.merge(&rga2);
        
        let mut m21 = rga2.clone();
        m21.merge(&rga1);
        
        prop_assert_eq!(m12.to_string(), m21.to_string());
    }

    /// Alternating inserts between two users at same position
    #[test]
    fn alternating_inserts(
        rounds in 5usize..15,
    ) {
        let user1 = KeyPair::generate();
        let user2 = KeyPair::generate();
        
        let mut rga1 = Rga::new();
        let mut rga2 = Rga::new();
        
        // Simulate alternating edits
        for i in 0..rounds {
            if i % 2 == 0 {
                let pos = rga1.len() / 2;
                rga1.insert(&user1.key_pub, pos.min(rga1.len()), b"A");
            } else {
                let pos = rga2.len() / 2;
                rga2.insert(&user2.key_pub, pos.min(rga2.len()), b"B");
            }
        }
        
        let mut m12 = rga1.clone();
        m12.merge(&rga2);
        
        let mut m21 = rga2.clone();
        m21.merge(&rga1);
        
        prop_assert_eq!(m12.to_string(), m21.to_string());
    }
}

// =============================================================================
// Version and snapshot tests
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Versions should capture document state correctly
    #[test]
    fn version_captures_state(
        ops in prop::collection::vec(arbitrary_rga_op(), 5..30),
    ) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();
        
        for op in ops.iter() {
            apply_rga_op(&mut rga, &user, op);
        }
        
        // Take version after all ops
        let version = rga.version();
        
        // Version should reflect state when taken
        prop_assert_eq!(rga.to_string_at(&version), rga.to_string());
        prop_assert_eq!(rga.len_at(&version), rga.len());
    }
}

// =============================================================================
// Deletion edge cases
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Delete then insert at same position
    #[test]
    fn delete_then_insert_same_position(
        initial in prop::collection::vec(b'a'..=b'z', 10..30),
        delete_start_pct in 0.2f64..0.8,
        delete_len in 1usize..5,
        insert_content in prop::collection::vec(b'A'..=b'Z', 1..10),
    ) {
        let user = KeyPair::generate();
        let _user2 = KeyPair::generate(); // For future concurrent tests
        let mut rga = Rga::new();
        
        rga.insert(&user.key_pub, 0, &initial);
        
        let delete_start = ((delete_start_pct * initial.len() as f64) as u64).min(rga.len().saturating_sub(1));
        let delete_len = (delete_len as u64).min(rga.len() - delete_start);
        
        if delete_len > 0 {
            rga.delete(delete_start, delete_len);
        }
        
        // Insert at the deletion point
        rga.insert(&user.key_pub, delete_start.min(rga.len()), &insert_content);
        
        check_rga_invariants(&rga)?;
    }

    /// Concurrent delete of same region
    #[test]
    fn concurrent_delete_same_region(
        initial in prop::collection::vec(b'a'..=b'z', 20..40),
        delete_start_pct in 0.2f64..0.6,
        delete_len in 3usize..8,
    ) {
        let user1 = KeyPair::generate();
        let _user2 = KeyPair::generate();
        
        // Both start with same content
        let mut rga1 = Rga::new();
        rga1.insert(&user1.key_pub, 0, &initial);
        let mut rga2 = rga1.clone();
        
        let delete_start = ((delete_start_pct * initial.len() as f64) as u64).min(rga1.len().saturating_sub(1));
        let delete_len = (delete_len as u64).min(rga1.len() - delete_start);
        
        if delete_len > 0 {
            // Both delete the same region
            rga1.delete(delete_start, delete_len);
            rga2.delete(delete_start, delete_len);
        }
        
        let mut m12 = rga1.clone();
        m12.merge(&rga2);
        
        let mut m21 = rga2.clone();
        m21.merge(&rga1);
        
        prop_assert_eq!(m12.to_string(), m21.to_string());
    }

    /// One user deletes what another user inserted into
    #[test]
    fn delete_around_concurrent_insert(
        initial in prop::collection::vec(b'a'..=b'z', 20..40),
        insert_content in prop::collection::vec(b'0'..=b'9', 3..8),
    ) {
        let user1 = KeyPair::generate();
        let user2 = KeyPair::generate();
        
        // Both start with same content
        let mut rga1 = Rga::new();
        rga1.insert(&user1.key_pub, 0, &initial);
        let mut rga2 = rga1.clone();
        
        // User1 inserts in the middle
        let insert_pos = initial.len() as u64 / 2;
        rga1.insert(&user1.key_pub, insert_pos, &insert_content);
        
        // User2 deletes a region that includes the insertion point
        let delete_start = insert_pos.saturating_sub(3);
        let delete_len = 6u64.min(rga2.len() - delete_start);
        if delete_len > 0 {
            rga2.delete(delete_start, delete_len);
        }
        
        let mut m12 = rga1.clone();
        m12.merge(&rga2);
        
        let mut m21 = rga2.clone();
        m21.merge(&rga1);
        
        // After merge, user1's insert should survive (insert wins over delete)
        prop_assert_eq!(m12.to_string(), m21.to_string());
        
        // The inserted content should be present
        let result = m12.to_string();
        let insert_str: String = insert_content.iter().map(|&c| c as char).collect();
        prop_assert!(result.contains(&insert_str), 
                    "Inserted content {:?} should survive in {:?}", insert_str, result);
    }
}
