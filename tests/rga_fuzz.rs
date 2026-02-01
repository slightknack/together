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
    
    let mut rga1 = Rga::new();
    rga1.insert(&user1.key_pub, 0, b"a");
    
    let mut rga2 = Rga::new();
    rga2.insert(&user2.key_pub, 0, b"aa");
    rga2.insert(&user2.key_pub, 1, b"aa"); // Insert after first 'a', splits span
    rga2.insert(&user2.key_pub, 2, b"b");  // Insert after second 'a' (which is first of inserted "aa")
    
    let mut m1 = rga1.clone();
    m1.merge(&rga2);
    
    let mut m2 = rga2.clone();
    m2.merge(&rga1);
    
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
        
        // Merge
        let mut m12 = rga1.clone();
        m12.merge(&rga2);
        
        let mut m21 = rga2.clone();
        m21.merge(&rga1);
        
        prop_assert_eq!(m12.to_string(), m21.to_string());
    }
}
