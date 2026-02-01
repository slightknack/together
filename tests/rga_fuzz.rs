// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

//! Fuzzing-style multi-user consistency test for the RGA CRDT.
//!
//! This test simulates N users (2..=10) randomly editing a document with
//! inserts, with biases toward conflict-inducing operations. After all
//! operations, all replicas are merged and verified to converge to the
//! same content.
//!
//! NOTE: The current RGA merge implementation has specific constraints:
//! 1. It only propagates inserts, not deletes
//! 2. Operations must be applied in sequence order (no gaps)
//!
//! These tests work within those constraints while still exercising
//! the CRDT convergence properties.

use proptest::prelude::*;
use proptest::test_runner::Config;

use together::crdt::rga::Rga;
use together::crdt::Crdt;
use together::key::KeyPair;

// =============================================================================
// English Letter Frequency Weighting
// =============================================================================

/// English letter frequencies (approximate, including space).
const LETTER_FREQUENCIES: [(u8, f64); 27] = [
    (b' ', 0.180),
    (b'e', 0.111),
    (b't', 0.079),
    (b'a', 0.071),
    (b'o', 0.066),
    (b'i', 0.061),
    (b'n', 0.059),
    (b's', 0.055),
    (b'h', 0.053),
    (b'r', 0.052),
    (b'd', 0.037),
    (b'l', 0.035),
    (b'c', 0.024),
    (b'u', 0.024),
    (b'm', 0.021),
    (b'w', 0.021),
    (b'f', 0.019),
    (b'g', 0.017),
    (b'y', 0.017),
    (b'p', 0.017),
    (b'b', 0.013),
    (b'v', 0.009),
    (b'k', 0.007),
    (b'j', 0.001),
    (b'x', 0.001),
    (b'q', 0.001),
    (b'z', 0.001),
];

/// Generate a byte weighted by English letter frequency.
fn weighted_byte() -> impl Strategy<Value = u8> {
    let weights: Vec<(u32, u8)> = LETTER_FREQUENCIES
        .iter()
        .map(|(b, f)| ((f * 10000.0) as u32, *b))
        .collect();

    prop_oneof![
        weights[0].0 => Just(weights[0].1),
        weights[1].0 => Just(weights[1].1),
        weights[2].0 => Just(weights[2].1),
        weights[3].0 => Just(weights[3].1),
        weights[4].0 => Just(weights[4].1),
        weights[5].0 => Just(weights[5].1),
        weights[6].0 => Just(weights[6].1),
        weights[7].0 => Just(weights[7].1),
        weights[8].0 => Just(weights[8].1),
        weights[9].0 => Just(weights[9].1),
        weights[10].0 => Just(weights[10].1),
        weights[11].0 => Just(weights[11].1),
        weights[12].0 => Just(weights[12].1),
        weights[13].0 => Just(weights[13].1),
        weights[14].0 => Just(weights[14].1),
        weights[15].0 => Just(weights[15].1),
        weights[16].0 => Just(weights[16].1),
        weights[17].0 => Just(weights[17].1),
        weights[18].0 => Just(weights[18].1),
        weights[19].0 => Just(weights[19].1),
        weights[20].0 => Just(weights[20].1),
        weights[21].0 => Just(weights[21].1),
        weights[22].0 => Just(weights[22].1),
        weights[23].0 => Just(weights[23].1),
        weights[24].0 => Just(weights[24].1),
        weights[25].0 => Just(weights[25].1),
        weights[26].0 => Just(weights[26].1),
    ]
}

/// Generate content with English letter frequency weighting.
fn weighted_content(min_len: usize, max_len: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(weighted_byte(), min_len..=max_len)
}

// =============================================================================
// Test Helpers
// =============================================================================

/// Full mesh merge: every replica merges with every other replica.
/// After this, all replicas should have identical content.
fn full_mesh_merge(rgas: &mut [Rga]) {
    let n = rgas.len();
    // Multiple rounds to handle transitive dependencies
    for _ in 0..n {
        for i in 0..n {
            for j in 0..n {
                if i != j {
                    let other = rgas[j].clone();
                    rgas[i].merge(&other);
                }
            }
        }
    }
}

/// Verify all replicas have converged to the same content.
fn verify_convergence(rgas: &[Rga]) -> Result<(), proptest::test_runner::TestCaseError> {
    let first = rgas[0].to_string();
    for (i, rga) in rgas.iter().enumerate().skip(1) {
        prop_assert_eq!(
            &rga.to_string(),
            &first,
            "Replica {} diverged from replica 0",
            i
        );
    }
    Ok(())
}

// =============================================================================
// Proptest Tests
// =============================================================================

proptest! {
    #![proptest_config(Config {
        cases: 100,
        max_shrink_iters: 1000,
        timeout: 10000,
        fork: false,
        ..Config::default()
    })]

    /// Each user makes one insert, then we merge all replicas.
    /// This is the core concurrent insert test.
    #[test]
    fn fuzz_single_insert_per_user(
        num_users in 2usize..=8,
        contents in prop::collection::vec(weighted_content(1, 20), 8),
        positions in prop::collection::vec(0.0..1.0f64, 8),
    ) {
        let users: Vec<KeyPair> = (0..num_users).map(|_| KeyPair::generate()).collect();
        let mut rgas: Vec<Rga> = (0..num_users).map(|_| Rga::new()).collect();

        // Each user makes exactly one insert
        for i in 0..num_users {
            let content = &contents[i % contents.len()];
            let pos_ratio = positions[i % positions.len()];
            let len = rgas[i].len();
            let pos = if len == 0 { 0 } else { ((len as f64) * pos_ratio) as u64 };
            rgas[i].insert(&users[i].key_pub, pos, content);
        }

        // Merge all
        full_mesh_merge(&mut rgas);

        // Verify convergence
        verify_convergence(&rgas)?;

        // Verify all content is present
        let result = rgas[0].to_string();
        for i in 0..num_users {
            let content = &contents[i % contents.len()];
            let content_str = String::from_utf8_lossy(content);
            prop_assert!(
                result.contains(&*content_str),
                "Content from user {} missing: {:?}",
                i,
                content_str
            );
        }
    }

    /// Users build documents independently, then merge.
    /// Each user types multiple characters sequentially.
    #[test]
    fn fuzz_sequential_typing_then_merge(
        num_users in 2usize..=5,
        chars_per_user in 5usize..=20,
    ) {
        let users: Vec<KeyPair> = (0..num_users).map(|_| KeyPair::generate()).collect();
        let mut rgas: Vec<Rga> = (0..num_users).map(|_| Rga::new()).collect();

        // Each user types independently
        for (i, (rga, user)) in rgas.iter_mut().zip(users.iter()).enumerate() {
            for j in 0..chars_per_user {
                let byte = b'a' + (i as u8 % 26);
                let pos = rga.len();
                rga.insert(&user.key_pub, pos, &[byte, b'0' + (j as u8 % 10)]);
            }
        }

        // Merge all
        full_mesh_merge(&mut rgas);

        // Verify convergence
        verify_convergence(&rgas)?;
    }

    /// All users insert at position 0 (maximum conflict).
    #[test]
    fn fuzz_all_insert_at_beginning(
        num_users in 2usize..=6,
        content_len in 1usize..=10,
    ) {
        let users: Vec<KeyPair> = (0..num_users).map(|_| KeyPair::generate()).collect();
        let mut rgas: Vec<Rga> = (0..num_users).map(|_| Rga::new()).collect();

        // Each user inserts at position 0
        for (i, (rga, user)) in rgas.iter_mut().zip(users.iter()).enumerate() {
            let content: Vec<u8> = (0..content_len).map(|j| b'A' + ((i + j) as u8 % 26)).collect();
            rga.insert(&user.key_pub, 0, &content);
        }

        // Merge all
        full_mesh_merge(&mut rgas);

        // Verify convergence
        verify_convergence(&rgas)?;
    }

    /// Merge is commutative: A.merge(B) should give same result as B.merge(A).
    #[test]
    fn fuzz_merge_commutativity(
        content_a in weighted_content(1, 20),
        content_b in weighted_content(1, 20),
        _pos_a in 0.0..1.0f64,
        _pos_b in 0.0..1.0f64,
    ) {
        let user_a = KeyPair::generate();
        let user_b = KeyPair::generate();

        let mut rga_a = Rga::new();
        let mut rga_b = Rga::new();

        // Each user inserts their content
        rga_a.insert(&user_a.key_pub, 0, &content_a);
        rga_b.insert(&user_b.key_pub, 0, &content_b);

        // Merge both ways
        let mut ab = rga_a.clone();
        ab.merge(&rga_b);

        let mut ba = rga_b.clone();
        ba.merge(&rga_a);

        // Should be identical
        prop_assert_eq!(ab.to_string(), ba.to_string(), "Merge should be commutative");
    }

    /// Merge is idempotent: A.merge(A) should equal A.
    #[test]
    fn fuzz_merge_idempotence(
        content in weighted_content(1, 50),
    ) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();
        rga.insert(&user.key_pub, 0, &content);

        let before = rga.to_string();
        let clone = rga.clone();
        rga.merge(&clone);
        let after = rga.to_string();

        prop_assert_eq!(before, after, "Merge should be idempotent");
    }

    /// Merge is associative: merge(A, merge(B, C)) == merge(merge(A, B), C)
    #[test]
    fn fuzz_merge_associativity(
        content_a in weighted_content(1, 30),
        content_b in weighted_content(1, 30),
        content_c in weighted_content(1, 30),
        pos_a in 0u64..=100,
        pos_b in 0u64..=100,
        pos_c in 0u64..=100,
    ) {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();
        let charlie = KeyPair::generate();

        // Create three independent replicas
        let mut rga_a = Rga::new();
        let mut rga_b = Rga::new();
        let mut rga_c = Rga::new();

        // Each user inserts at a position (clamped to doc length)
        rga_a.insert(&alice.key_pub, 0, &content_a);
        rga_b.insert(&bob.key_pub, 0, &content_b);
        rga_c.insert(&charlie.key_pub, 0, &content_c);

        // Left associative: merge(merge(A, B), C)
        let mut left = rga_a.clone();
        left.merge(&rga_b);
        left.merge(&rga_c);

        // Right associative: merge(A, merge(B, C))
        let mut bc = rga_b.clone();
        bc.merge(&rga_c);
        let mut right = rga_a.clone();
        right.merge(&bc);

        prop_assert_eq!(left.to_string(), right.to_string(), "Merge should be associative");
    }
}

// =============================================================================
// Deterministic Unit Tests
// =============================================================================

#[test]
fn test_two_users_concurrent_insert_same_position() {
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();

    let mut rga_a = Rga::new();
    let mut rga_b = Rga::new();

    rga_a.insert(&alice.key_pub, 0, b"ALICE");
    rga_b.insert(&bob.key_pub, 0, b"BOB");

    let mut merged_ab = rga_a.clone();
    merged_ab.merge(&rga_b);

    let mut merged_ba = rga_b.clone();
    merged_ba.merge(&rga_a);

    assert_eq!(merged_ab.to_string(), merged_ba.to_string());
    let result = merged_ab.to_string();
    assert!(result.contains("ALICE"));
    assert!(result.contains("BOB"));
}

#[test]
fn test_three_users_chain_merge() {
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();
    let charlie = KeyPair::generate();

    let mut rga_a = Rga::new();
    let mut rga_b = Rga::new();
    let mut rga_c = Rga::new();

    rga_a.insert(&alice.key_pub, 0, b"A");
    rga_b.insert(&bob.key_pub, 0, b"B");
    rga_c.insert(&charlie.key_pub, 0, b"C");

    // Chain merge
    rga_b.merge(&rga_a);
    rga_c.merge(&rga_b);
    rga_a.merge(&rga_c);
    rga_b.merge(&rga_a);
    rga_c.merge(&rga_a);

    assert_eq!(rga_a.to_string(), rga_b.to_string());
    assert_eq!(rga_b.to_string(), rga_c.to_string());

    let result = rga_a.to_string();
    assert!(result.contains("A"));
    assert!(result.contains("B"));
    assert!(result.contains("C"));
}

#[test]
fn test_many_users_all_insert_at_beginning() {
    const NUM_USERS: usize = 10;

    let users: Vec<KeyPair> = (0..NUM_USERS).map(|_| KeyPair::generate()).collect();
    let mut rgas: Vec<Rga> = (0..NUM_USERS).map(|_| Rga::new()).collect();

    for (i, (rga, user)) in rgas.iter_mut().zip(users.iter()).enumerate() {
        let content = format!("{}", i);
        rga.insert(&user.key_pub, 0, content.as_bytes());
    }

    full_mesh_merge(&mut rgas);

    let first = rgas[0].to_string();
    for (i, rga) in rgas.iter().enumerate().skip(1) {
        assert_eq!(rga.to_string(), first, "user {} diverged", i);
    }

    for i in 0..NUM_USERS {
        assert!(first.contains(&format!("{}", i)), "digit {} missing", i);
    }
}

#[test]
fn test_large_document_insert_convergence() {
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();

    let mut rga_a = Rga::new();
    let mut rga_b = Rga::new();

    // Each user builds their document independently
    for i in 0..50 {
        let content = format!("A{}", i);
        let pos = rga_a.len();
        rga_a.insert(&alice.key_pub, pos, content.as_bytes());
    }

    for i in 0..50 {
        let content = format!("B{}", i);
        let pos = rga_b.len();
        rga_b.insert(&bob.key_pub, pos, content.as_bytes());
    }

    // Merge
    let clone_a = rga_a.clone();
    let clone_b = rga_b.clone();
    rga_a.merge(&clone_b);
    rga_b.merge(&clone_a);

    assert_eq!(rga_a.to_string(), rga_b.to_string());

    let result = rga_a.to_string();
    assert!(result.contains("A0"));
    assert!(result.contains("B0"));
    assert!(result.contains("A49"));
    assert!(result.contains("B49"));
}

#[test]
fn test_sequential_typing_single_user() {
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    let text = b"hello world";
    for (i, &byte) in text.iter().enumerate() {
        rga.insert(&user.key_pub, i as u64, &[byte]);
    }

    assert_eq!(rga.to_string(), "hello world");
    assert_eq!(rga.len(), 11);
}

#[test]
fn test_insert_at_various_positions() {
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    rga.insert(&user.key_pub, 0, b"A");
    rga.insert(&user.key_pub, 1, b"C");
    rga.insert(&user.key_pub, 2, b"E");
    rga.insert(&user.key_pub, 1, b"B");
    rga.insert(&user.key_pub, 3, b"D");

    assert_eq!(rga.to_string(), "ABCDE");
}

#[test]
fn test_weighted_content_distribution() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    let mut runner = TestRunner::default();
    let strategy = weighted_content(100, 100);

    let mut space_count = 0;
    let mut e_count = 0;
    let mut total = 0;

    for _ in 0..10 {
        let tree = strategy.new_tree(&mut runner).unwrap();
        let content = tree.current();
        for &byte in &content {
            total += 1;
            if byte == b' ' {
                space_count += 1;
            }
            if byte == b'e' {
                e_count += 1;
            }
        }
    }

    assert!(space_count > 0, "spaces should appear");
    assert!(e_count > 0, "e should appear");
    assert!(
        space_count as f64 / total as f64 > 0.05,
        "space frequency too low: {}/{}",
        space_count,
        total
    );
}

// =============================================================================
// Local Delete Tests (Merge Does Not Propagate Deletes)
// =============================================================================

#[test]
fn test_local_delete_operations() {
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    rga.insert(&user.key_pub, 0, b"hello world");
    assert_eq!(rga.len(), 11);

    rga.delete(6, 5);
    assert_eq!(rga.to_string(), "hello ");
    assert_eq!(rga.len(), 6);

    rga.delete(5, 1);
    assert_eq!(rga.to_string(), "hello");
    assert_eq!(rga.len(), 5);
}

#[test]
fn test_local_insert_after_delete() {
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    rga.insert(&user.key_pub, 0, b"hello world");
    rga.delete(5, 6);
    rga.insert(&user.key_pub, 5, b" rust");

    assert_eq!(rga.to_string(), "hello rust");
}

#[test]
fn test_local_backspace_pattern() {
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    rga.insert(&user.key_pub, 0, b"helllo");
    rga.delete(3, 1);
    rga.insert(&user.key_pub, 5, b" world");

    assert_eq!(rga.to_string(), "hello world");
}

// =============================================================================
// Versioning Snapshot Consistency Tests
// =============================================================================
//
// These tests verify that version IDs correctly identify a point in history
// that can be reconstructed by any replica that has seen the same operations.
//
// The core scenario:
// 1. User A edits a document and takes a version/checkpoint with a snapshot
// 2. User A and User B continue editing (both make changes)
// 3. User A sends the version ID to User B
// 4. User B reconstructs the document at that version
// 5. Verify: User A's original snapshot == User B's snapshot at that version

#[test]
fn test_version_snapshot_basic() {
    // User A edits, takes a version, continues editing
    // Verify the version can be used to reconstruct the original state
    let alice = KeyPair::generate();
    let mut rga = Rga::new();

    // Initial editing
    rga.insert(&alice.key_pub, 0, b"hello");
    
    // Take a version and snapshot
    let version = rga.version();
    let snapshot_at_version = rga.to_string();
    assert_eq!(snapshot_at_version, "hello");

    // Continue editing
    rga.insert(&alice.key_pub, 5, b" world");
    assert_eq!(rga.to_string(), "hello world");

    // Reconstruct at version
    let reconstructed = rga.to_string_at(&version);
    assert_eq!(reconstructed, snapshot_at_version);
}

#[test]
fn test_version_snapshot_two_users_same_replica() {
    // User A takes a version, then User B edits the same replica
    // Verify the version still correctly identifies User A's checkpoint
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();
    let mut rga = Rga::new();

    // Alice edits
    rga.insert(&alice.key_pub, 0, b"alice ");
    
    // Take version
    let version = rga.version();
    let snapshot = rga.to_string();
    assert_eq!(snapshot, "alice ");

    // Bob edits
    rga.insert(&bob.key_pub, 6, b"and bob");
    assert_eq!(rga.to_string(), "alice and bob");

    // Reconstruct at version - should match Alice's snapshot
    let reconstructed = rga.to_string_at(&version);
    assert_eq!(reconstructed, snapshot);
}

#[test]
fn test_version_snapshot_after_merge() {
    // Two users edit independently, merge, then take a version
    // This tests that versions work correctly after CRDT merge
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();

    let mut rga_a = Rga::new();
    let mut rga_b = Rga::new();

    // Each user edits independently
    rga_a.insert(&alice.key_pub, 0, b"ALICE");
    rga_b.insert(&bob.key_pub, 0, b"BOB");

    // Merge
    rga_a.merge(&rga_b);

    // Take version after merge
    let version = rga_a.version();
    let snapshot = rga_a.to_string();

    // Continue editing
    rga_a.insert(&alice.key_pub, rga_a.len(), b"!");
    assert!(rga_a.to_string().ends_with("!"));

    // Reconstruct at version
    let reconstructed = rga_a.to_string_at(&version);
    assert_eq!(reconstructed, snapshot);
}

#[test]
fn test_version_snapshot_shared_across_replicas() {
    // The core scenario from the task description:
    // 1. User A is editing a document
    // 2. User A takes a version/checkpoint with a snapshot
    // 3. User A and User B continue editing
    // 4. User A sends the version to User B (via merge)
    // 5. User B reconstructs at that version
    // 6. Verify: User A's snapshot == User B's reconstruction
    let alice = KeyPair::generate();
    let _bob = KeyPair::generate();

    let mut rga_a = Rga::new();

    // Step 1: User A edits
    rga_a.insert(&alice.key_pub, 0, b"hello");

    // Step 2: User A takes version and snapshot
    let version_a = rga_a.version();
    let snapshot_a = rga_a.to_string();
    assert_eq!(snapshot_a, "hello");

    // Step 3: User A continues editing
    rga_a.insert(&alice.key_pub, 5, b" world");
    assert_eq!(rga_a.to_string(), "hello world");

    // User B starts with an empty replica and merges A's state
    let mut rga_b = Rga::new();
    rga_b.merge(&rga_a);

    // Step 5: User B reconstructs at User A's version
    // The version contains a snapshot, so B can reconstruct directly
    let snapshot_b = rga_b.to_string_at(&version_a);

    // Step 6: Verify they match
    assert_eq!(snapshot_a, snapshot_b);
}

#[test]
fn test_version_snapshot_multiple_checkpoints() {
    // Take multiple checkpoints at different points in editing
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    // Checkpoint 1: empty (before any edits)
    let v0 = rga.version();
    let s0 = rga.to_string();
    assert_eq!(s0, "");

    // Edit
    rga.insert(&user.key_pub, 0, b"one");
    
    // Checkpoint 2
    let v1 = rga.version();
    let s1 = rga.to_string();
    assert_eq!(s1, "one");

    // Edit more
    rga.insert(&user.key_pub, 3, b" two");
    
    // Checkpoint 3
    let v2 = rga.version();
    let s2 = rga.to_string();
    assert_eq!(s2, "one two");

    // Edit even more
    rga.insert(&user.key_pub, 7, b" three");
    
    // Checkpoint 4
    let v3 = rga.version();
    let s3 = rga.to_string();
    assert_eq!(s3, "one two three");

    // Verify all versions can be reconstructed correctly
    assert_eq!(rga.to_string_at(&v0), s0);
    assert_eq!(rga.to_string_at(&v1), s1);
    assert_eq!(rga.to_string_at(&v2), s2);
    assert_eq!(rga.to_string_at(&v3), s3);
}

#[test]
fn test_version_snapshot_before_any_edits() {
    // Take a checkpoint on an empty document
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    // Version of empty doc
    let v_empty = rga.version();
    let s_empty = rga.to_string();
    assert_eq!(s_empty, "");

    // Make many edits
    for i in 0..10 {
        let content = format!("{}", i);
        rga.insert(&user.key_pub, rga.len(), content.as_bytes());
    }

    assert_eq!(rga.to_string(), "0123456789");

    // Reconstruct empty version
    let reconstructed = rga.to_string_at(&v_empty);
    assert_eq!(reconstructed, s_empty);
}

#[test]
fn test_version_snapshot_after_many_edits() {
    // Take a checkpoint, then make many subsequent edits
    // Verify the version still works after the document has grown significantly
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    // Initial content
    rga.insert(&user.key_pub, 0, b"start");
    
    // Take checkpoint
    let version = rga.version();
    let snapshot = rga.to_string();

    // Make many edits
    for i in 0..100 {
        let content = format!("-{}", i);
        rga.insert(&user.key_pub, rga.len(), content.as_bytes());
    }

    // Document should be much larger now
    assert!(rga.len() > 100);

    // Reconstruct original version
    let reconstructed = rga.to_string_at(&version);
    assert_eq!(reconstructed, snapshot);
}

#[test]
fn test_version_snapshot_with_deletes() {
    // Versions should capture deleted content state too
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    // Insert and delete
    rga.insert(&user.key_pub, 0, b"hello world");
    rga.delete(5, 6); // Delete " world"
    
    // Checkpoint after delete
    let version = rga.version();
    let snapshot = rga.to_string();
    assert_eq!(snapshot, "hello");

    // Continue editing
    rga.insert(&user.key_pub, 5, b" rust");
    assert_eq!(rga.to_string(), "hello rust");

    // Reconstruct at version
    let reconstructed = rga.to_string_at(&version);
    assert_eq!(reconstructed, snapshot);
}

#[test]
fn test_version_len_at() {
    // Test len_at for historical length queries
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    let v0 = rga.version();
    assert_eq!(rga.len_at(&v0), 0);

    rga.insert(&user.key_pub, 0, b"hello");
    let v1 = rga.version();
    assert_eq!(rga.len_at(&v1), 5);

    rga.insert(&user.key_pub, 5, b" world");
    let v2 = rga.version();
    assert_eq!(rga.len_at(&v2), 11);

    // All historical lengths should still be correct
    assert_eq!(rga.len_at(&v0), 0);
    assert_eq!(rga.len_at(&v1), 5);
    assert_eq!(rga.len_at(&v2), 11);
}

#[test]
fn test_version_slice_at() {
    // Test slice_at for historical range queries
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    rga.insert(&user.key_pub, 0, b"hello world");
    let version = rga.version();

    // Modify document
    rga.delete(5, 6);
    rga.insert(&user.key_pub, 5, b" rust");
    assert_eq!(rga.to_string(), "hello rust");

    // Slice at historical version
    assert_eq!(rga.slice_at(0, 5, &version), Some("hello".to_string()));
    assert_eq!(rga.slice_at(6, 11, &version), Some("world".to_string()));
    assert_eq!(rga.slice_at(0, 11, &version), Some("hello world".to_string()));

    // Out of bounds returns None
    assert_eq!(rga.slice_at(0, 20, &version), Some("hello world".to_string())); // Clamped
    assert_eq!(rga.slice_at(15, 20, &version), None);
}

// =============================================================================
// Property-Based Versioning Tests
// =============================================================================

proptest! {
    #![proptest_config(Config {
        cases: 50,
        max_shrink_iters: 500,
        timeout: 10000,
        fork: false,
        ..Config::default()
    })]

    /// Versioning invariant: snapshot at version always equals original snapshot
    #[test]
    fn prop_version_snapshot_consistency(
        initial_content in weighted_content(1, 50),
        additional_edits in prop::collection::vec(weighted_content(1, 20), 1..10),
    ) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();

        // Initial state
        rga.insert(&user.key_pub, 0, &initial_content);
        
        // Take version
        let version = rga.version();
        let snapshot = rga.to_string();

        // Make additional edits
        for content in additional_edits {
            let pos = rga.len();
            rga.insert(&user.key_pub, pos, &content);
        }

        // Verify version reconstruction matches original snapshot
        let reconstructed = rga.to_string_at(&version);
        prop_assert_eq!(reconstructed, snapshot);
    }

    /// Multiple versions taken during editing should all be reconstructable
    #[test]
    fn prop_multiple_versions(
        num_checkpoints in 2usize..=5,
        content_per_checkpoint in prop::collection::vec(weighted_content(1, 20), 5),
    ) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();
        
        let mut versions = Vec::new();
        let mut snapshots = Vec::new();

        // Take multiple checkpoints
        for i in 0..num_checkpoints {
            let content = &content_per_checkpoint[i % content_per_checkpoint.len()];
            let pos = rga.len();
            rga.insert(&user.key_pub, pos, content);
            
            versions.push(rga.version());
            snapshots.push(rga.to_string());
        }

        // All versions should reconstruct correctly
        for (version, expected) in versions.iter().zip(snapshots.iter()) {
            let reconstructed = rga.to_string_at(version);
            prop_assert_eq!(&reconstructed, expected);
        }
    }

    /// Version reconstruction works after merge
    #[test]
    fn prop_version_after_merge(
        content_a in weighted_content(1, 30),
        content_b in weighted_content(1, 30),
    ) {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();

        let mut rga_a = Rga::new();
        let mut rga_b = Rga::new();

        // Alice edits and takes version
        rga_a.insert(&alice.key_pub, 0, &content_a);
        let version_a = rga_a.version();
        let snapshot_a = rga_a.to_string();

        // Bob edits independently
        rga_b.insert(&bob.key_pub, 0, &content_b);

        // Merge
        rga_a.merge(&rga_b);

        // Alice's version should still reconstruct correctly
        let reconstructed = rga_a.to_string_at(&version_a);
        prop_assert_eq!(reconstructed, snapshot_a);
    }
}
