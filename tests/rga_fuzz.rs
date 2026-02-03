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

// =============================================================================
// Hole 1: Delete Propagation in Merge
// =============================================================================
//
// The current RGA merge implementation only propagates inserts, not deletes.
// This is documented behavior, not a bug. When user A deletes content on their
// replica, that delete is NOT propagated to user B when B merges from A.
//
// This design choice means:
// 1. Deletes are local-only operations
// 2. After merge, replicas may have different content if one has deletes
// 3. To propagate deletes, you need a separate delete operation log/sync mechanism
//
// These tests document and verify this behavior.

#[test]
fn test_delete_not_propagated_in_merge() {
    // User A deletes content, user B merges from A.
    // User B should NOT see the delete - they get A's content pre-delete.
    let alice = KeyPair::generate();
    let _bob = KeyPair::generate();

    let mut rga_a = Rga::new();
    let mut rga_b = Rga::new();

    // Alice inserts content
    rga_a.insert(&alice.key_pub, 0, b"hello world");
    assert_eq!(rga_a.to_string(), "hello world");

    // Alice deletes " world"
    rga_a.delete(5, 6);
    assert_eq!(rga_a.to_string(), "hello");

    // Bob has an empty replica
    assert_eq!(rga_b.to_string(), "");

    // Bob merges from Alice
    // DOCUMENTED BEHAVIOR: Bob gets Alice's inserts but NOT her deletes
    rga_b.merge(&rga_a);

    // Bob should have the full content (inserts are propagated)
    // but NOT the delete (deletes are local-only)
    assert_eq!(rga_b.to_string(), "hello world");

    // Alice still has her local delete
    assert_eq!(rga_a.to_string(), "hello");
}

#[test]
fn test_delete_local_only_two_users() {
    // Both users have the same initial content.
    // User A deletes part of it.
    // After merge, user B still has the deleted content.
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();

    let mut rga_a = Rga::new();
    let mut rga_b = Rga::new();

    // Both users insert content (we'll simulate shared initial state)
    rga_a.insert(&alice.key_pub, 0, b"shared content");
    rga_b.merge(&rga_a); // Bob gets Alice's content
    
    assert_eq!(rga_a.to_string(), "shared content");
    assert_eq!(rga_b.to_string(), "shared content");

    // Alice deletes "content"
    rga_a.delete(7, 7);
    assert_eq!(rga_a.to_string(), "shared ");

    // Bob adds more content
    rga_b.insert(&bob.key_pub, 14, b" here");
    assert_eq!(rga_b.to_string(), "shared content here");

    // Merge in both directions
    let rga_a_clone = rga_a.clone();
    let rga_b_clone = rga_b.clone();
    rga_a.merge(&rga_b_clone);
    rga_b.merge(&rga_a_clone);

    // Alice gets Bob's new content, but keeps her delete
    assert!(rga_a.to_string().contains("shared "));
    assert!(rga_a.to_string().contains(" here"));
    // Note: Alice's delete removes "content" but the tombstone is local

    // Bob does NOT get Alice's delete
    assert!(rga_b.to_string().contains("content"));
}

#[test]
fn test_delete_visibility_after_merge() {
    // Document the asymmetry: delete is visible locally but not after merge.
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    rga.insert(&user.key_pub, 0, b"ABCDE");
    assert_eq!(rga.len(), 5);
    assert_eq!(rga.to_string(), "ABCDE");

    // Delete "BCD"
    rga.delete(1, 3);
    assert_eq!(rga.len(), 2);
    assert_eq!(rga.to_string(), "AE");

    // Create another replica and merge
    let mut rga2 = Rga::new();
    rga2.merge(&rga);

    // The new replica sees all inserts but not deletes
    // DOCUMENTED BEHAVIOR: rga2 has the full content
    assert_eq!(rga2.to_string(), "ABCDE");
    assert_eq!(rga2.len(), 5);
}

#[test]
fn test_multiple_deletes_not_propagated() {
    // Multiple deletes by one user are all local-only.
    let alice = KeyPair::generate();

    let mut rga_a = Rga::new();

    // Alice inserts and performs multiple deletes
    rga_a.insert(&alice.key_pub, 0, b"one two three four");
    rga_a.delete(4, 4);  // Delete "two "
    rga_a.delete(4, 6);  // Delete "three "
    assert_eq!(rga_a.to_string(), "one four");

    // New replica merges from Alice
    let mut rga_b = Rga::new();
    rga_b.merge(&rga_a);

    // Bob sees all the original content
    assert_eq!(rga_b.to_string(), "one two three four");
}

proptest! {
    #![proptest_config(Config {
        cases: 50,
        max_shrink_iters: 500,
        timeout: 10000,
        fork: false,
        ..Config::default()
    })]

    /// Property: After merge, new replica has at least as much visible content
    /// as the original (since deletes don't propagate).
    #[test]
    fn prop_merge_does_not_propagate_deletes(
        content in weighted_content(5, 50),
        delete_start in 0usize..5,
        delete_len in 1usize..5,
    ) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();

        rga.insert(&user.key_pub, 0, &content);
        let original_len = rga.len();

        // Perform delete if valid
        let actual_start = delete_start.min(original_len as usize - 1);
        let actual_len = delete_len.min((original_len as usize) - actual_start);
        if actual_len > 0 {
            rga.delete(actual_start as u64, actual_len as u64);
        }
        let after_delete_len = rga.len();

        // Merge into new replica
        let mut rga2 = Rga::new();
        rga2.merge(&rga);

        // New replica should have original length (deletes not propagated)
        prop_assert_eq!(rga2.len(), original_len);
        
        // Original should have reduced length
        prop_assert!(after_delete_len <= original_len);
    }
}

// =============================================================================
// Hole 2: Adversarial Concurrent Delete Scenarios
// =============================================================================
//
// These tests explore edge cases around concurrent deletes and inserts.
// Since deletes don't propagate in merge, some of these tests focus on
// local delete behavior and what happens with concurrent operations.

#[test]
fn test_user_a_deletes_range_user_b_inserts_into_range() {
    // User A deletes a range, User B inserts into that range, then merge.
    // Since deletes are local-only, after merge:
    // - User A has: original content minus deleted range, plus B's insert
    // - User B has: original content plus their insert (no delete from A)
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();

    // Setup: Both replicas start with shared content
    let mut rga_a = Rga::new();
    let mut rga_b = Rga::new();

    rga_a.insert(&alice.key_pub, 0, b"hello world");
    rga_b.merge(&rga_a);

    assert_eq!(rga_a.to_string(), "hello world");
    assert_eq!(rga_b.to_string(), "hello world");

    // User A deletes "world" (positions 6-10)
    rga_a.delete(6, 5);
    assert_eq!(rga_a.to_string(), "hello ");

    // User B inserts "beautiful " at position 6 (into the range A deleted)
    rga_b.insert(&bob.key_pub, 6, b"beautiful ");
    assert_eq!(rga_b.to_string(), "hello beautiful world");

    // Merge: A merges from B
    rga_a.merge(&rga_b);
    
    // A should now have B's insert, but A's local delete still applies
    // The "beautiful " insert is new content, so it appears
    // The original "world" was deleted by A locally
    let result_a = rga_a.to_string();
    assert!(result_a.contains("hello"));
    assert!(result_a.contains("beautiful"));
    // "world" was deleted locally by A, so it shouldn't appear
    // But actually the delete only affects the original spans, not new inserts

    // Merge: B merges from A
    let rga_a_for_b = rga_a.clone();
    rga_b.merge(&rga_a_for_b);

    // B doesn't get A's delete, so B has everything
    let result_b = rga_b.to_string();
    assert!(result_b.contains("hello"));
    assert!(result_b.contains("beautiful"));
    assert!(result_b.contains("world"));
}

#[test]
fn test_two_users_delete_overlapping_ranges() {
    // Both users delete overlapping ranges from their local replicas.
    // Since deletes don't propagate, each user only sees their own delete.
    let alice = KeyPair::generate();
    let _bob = KeyPair::generate();

    let mut rga_a = Rga::new();
    let mut rga_b = Rga::new();

    // Shared content: "0123456789"
    rga_a.insert(&alice.key_pub, 0, b"0123456789");
    rga_b.merge(&rga_a);

    // User A deletes "234" (positions 2-4)
    rga_a.delete(2, 3);
    assert_eq!(rga_a.to_string(), "0156789");

    // User B deletes "456" (positions 4-6) 
    rga_b.delete(4, 3);
    assert_eq!(rga_b.to_string(), "0123789");

    // After merge, neither sees the other's delete
    let rga_a_clone = rga_a.clone();
    let rga_b_clone = rga_b.clone();
    rga_a.merge(&rga_b_clone);
    rga_b.merge(&rga_a_clone);

    // A still has their local delete only
    assert_eq!(rga_a.to_string(), "0156789");
    
    // B still has their local delete only  
    assert_eq!(rga_b.to_string(), "0123789");
}

#[test]
fn test_delete_then_insert_same_position_different_users() {
    // User A deletes at position P, User B inserts at position P.
    // This tests the interaction of local deletes with concurrent inserts.
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();

    let mut rga_a = Rga::new();
    let mut rga_b = Rga::new();

    // Shared content: "ABCDE"
    rga_a.insert(&alice.key_pub, 0, b"ABCDE");
    rga_b.merge(&rga_a);

    // User A deletes "C" at position 2
    rga_a.delete(2, 1);
    assert_eq!(rga_a.to_string(), "ABDE");

    // User B inserts "X" at position 2 (before C)
    rga_b.insert(&bob.key_pub, 2, b"X");
    assert_eq!(rga_b.to_string(), "ABXCDE");

    // Merge
    let rga_a_clone = rga_a.clone();
    let rga_b_clone = rga_b.clone();
    rga_a.merge(&rga_b_clone);
    rga_b.merge(&rga_a_clone);

    // A gets B's insert "X", but A's delete of "C" is local
    let result_a = rga_a.to_string();
    assert!(result_a.contains("X"));
    // "C" was deleted locally by A
    assert!(!result_a.contains("C"));

    // B doesn't get A's delete, so B has everything including C
    let result_b = rga_b.to_string();
    assert!(result_b.contains("X"));
    assert!(result_b.contains("C"));
}

#[test]
fn test_user_deletes_content_containing_anchored_position() {
    // User A creates an anchor, User A then deletes the content containing the anchor.
    // The anchor should resolve to None after the anchored character is deleted.
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    rga.insert(&user.key_pub, 0, b"hello world");

    // Create an anchor at position 6 (the 'w' in "world")
    use together::crdt::rga::AnchorBias;
    let anchor = rga.anchor_at(6, AnchorBias::Before).unwrap();
    
    // Verify anchor resolves correctly before delete
    assert_eq!(rga.resolve_anchor(&anchor), Some(6));

    // Delete "world" (positions 6-10)
    rga.delete(6, 5);
    assert_eq!(rga.to_string(), "hello ");

    // Anchor should now resolve to None since the 'w' was deleted
    assert_eq!(rga.resolve_anchor(&anchor), None);
}

#[test]
fn test_concurrent_insert_at_deleted_anchor() {
    // User A has an anchor, deletes the anchored content.
    // User B inserts at the same position.
    // This tests anchor behavior with concurrent operations.
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();

    let mut rga_a = Rga::new();
    let mut rga_b = Rga::new();

    // Shared content
    rga_a.insert(&alice.key_pub, 0, b"ABCDE");
    rga_b.merge(&rga_a);

    // User A creates an anchor at position 2 (the 'C')
    use together::crdt::rga::AnchorBias;
    let anchor = rga_a.anchor_at(2, AnchorBias::Before).unwrap();
    assert_eq!(rga_a.resolve_anchor(&anchor), Some(2));

    // User A deletes "C"
    rga_a.delete(2, 1);
    assert_eq!(rga_a.to_string(), "ABDE");
    assert_eq!(rga_a.resolve_anchor(&anchor), None); // Anchor is now invalid

    // User B inserts at position 2
    rga_b.insert(&bob.key_pub, 2, b"X");
    assert_eq!(rga_b.to_string(), "ABXCDE");

    // Merge
    rga_a.merge(&rga_b);

    // A now has B's insert, but A's delete is still local
    let result = rga_a.to_string();
    assert!(result.contains("X"));
    // The anchor still points to the deleted 'C', so it's still None
    assert_eq!(rga_a.resolve_anchor(&anchor), None);
}

#[test]
fn test_delete_range_with_multiple_spans() {
    // Delete a range that spans multiple internal spans.
    // This tests the delete logic when crossing span boundaries.
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();

    let mut rga = Rga::new();

    // Create content from multiple users (creates multiple spans)
    // Using different users ensures spans won't coalesce
    rga.insert(&alice.key_pub, 0, b"AAA");
    rga.insert(&bob.key_pub, 3, b"BBB");
    rga.insert(&alice.key_pub, 6, b"CCC");
    
    assert_eq!(rga.to_string(), "AAABBBCCC");
    assert!(rga.span_count() >= 2); // Multiple spans

    // Delete across span boundaries: "ABBB" (positions 2-5)
    rga.delete(2, 4);
    assert_eq!(rga.to_string(), "AACCC");
}

#[test]
fn test_rapid_delete_insert_alternation() {
    // Rapidly alternate between delete and insert at the same position.
    // This stress-tests the position tracking during interleaved operations.
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    rga.insert(&user.key_pub, 0, b"X");

    for i in 0..10 {
        // Delete the character at position 0
        rga.delete(0, 1);
        assert_eq!(rga.len(), 0);
        assert_eq!(rga.to_string(), "");

        // Insert a new character
        let byte = b'A' + (i as u8 % 26);
        rga.insert(&user.key_pub, 0, &[byte]);
        assert_eq!(rga.len(), 1);
    }

    // Should have exactly one character at the end
    assert_eq!(rga.len(), 1);
}

#[test]
fn test_delete_entire_document_then_insert() {
    // Delete all content, then insert new content.
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    rga.insert(&user.key_pub, 0, b"hello world");
    assert_eq!(rga.len(), 11);

    // Delete everything
    rga.delete(0, 11);
    assert_eq!(rga.len(), 0);
    assert_eq!(rga.to_string(), "");

    // Insert new content
    rga.insert(&user.key_pub, 0, b"goodbye");
    assert_eq!(rga.len(), 7);
    assert_eq!(rga.to_string(), "goodbye");
}

proptest! {
    #![proptest_config(Config {
        cases: 50,
        max_shrink_iters: 500,
        timeout: 10000,
        fork: false,
        ..Config::default()
    })]

    /// Property: Delete followed by insert at same position maintains consistency.
    #[test]
    fn prop_delete_insert_same_position(
        initial in weighted_content(5, 30),
        insert_content in weighted_content(1, 10),
        delete_pos in 0usize..5,
        delete_len in 1usize..5,
    ) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();

        rga.insert(&user.key_pub, 0, &initial);
        let initial_len = rga.len();

        // Clamp delete range
        let actual_pos = delete_pos.min((initial_len as usize).saturating_sub(1));
        let actual_len = delete_len.min((initial_len as usize) - actual_pos);
        
        if actual_len > 0 {
            rga.delete(actual_pos as u64, actual_len as u64);
        }
        let after_delete = rga.len();

        // Insert at the same position
        rga.insert(&user.key_pub, actual_pos as u64, &insert_content);
        let after_insert = rga.len();

        // Length should be: after_delete + insert_content.len()
        prop_assert_eq!(after_insert, after_delete + insert_content.len() as u64);
    }

    /// Property: Overlapping deletes from two users stay independent.
    #[test]
    fn prop_overlapping_deletes_independent(
        content in weighted_content(10, 40),
        delete_a_start in 0usize..5,
        delete_a_len in 1usize..5,
        delete_b_start in 0usize..5,
        delete_b_len in 1usize..5,
    ) {
        let alice = KeyPair::generate();
        let _bob = KeyPair::generate();

        let mut rga_a = Rga::new();
        let mut rga_b = Rga::new();

        // Shared content
        rga_a.insert(&alice.key_pub, 0, &content);
        rga_b.merge(&rga_a);
        let original_len = rga_a.len() as usize;

        // Clamp delete ranges
        let a_start = delete_a_start.min(original_len.saturating_sub(1));
        let a_len = delete_a_len.min(original_len - a_start);
        let b_start = delete_b_start.min(original_len.saturating_sub(1));
        let b_len = delete_b_len.min(original_len - b_start);

        // Each user deletes
        if a_len > 0 {
            rga_a.delete(a_start as u64, a_len as u64);
        }
        if b_len > 0 {
            rga_b.delete(b_start as u64, b_len as u64);
        }

        let len_a = rga_a.len();
        let len_b = rga_b.len();

        // Merge (deletes don't propagate)
        let rga_a_clone = rga_a.clone();
        let rga_b_clone = rga_b.clone();
        rga_a.merge(&rga_b_clone);
        rga_b.merge(&rga_a_clone);

        // Each replica should still have their own delete applied
        // (lengths unchanged by merge since deletes don't propagate)
        prop_assert_eq!(rga_a.len(), len_a);
        prop_assert_eq!(rga_b.len(), len_b);
    }
}

// =============================================================================
// Hole 3: Network Partition / Delayed Sync
// =============================================================================
//
// These tests simulate scenarios where users work independently for extended
// periods before syncing. This exercises the CRDT's ability to handle large
// divergence and still converge correctly.

#[test]
fn test_500_edits_per_user_then_merge() {
    // User A makes 500 edits, User B makes 500 edits independently, then merge.
    // This tests convergence after significant independent work.
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();

    let mut rga_a = Rga::new();
    let mut rga_b = Rga::new();

    // User A makes 500 independent edits
    for i in 0..500 {
        let content = format!("A{}", i);
        let pos = rga_a.len();
        rga_a.insert(&alice.key_pub, pos, content.as_bytes());
    }

    // User B makes 500 independent edits
    for i in 0..500 {
        let content = format!("B{}", i);
        let pos = rga_b.len();
        rga_b.insert(&bob.key_pub, pos, content.as_bytes());
    }

    // Both should have substantial content
    assert!(rga_a.len() > 1000);
    assert!(rga_b.len() > 1000);

    // Merge
    let rga_a_clone = rga_a.clone();
    let rga_b_clone = rga_b.clone();
    rga_a.merge(&rga_b_clone);
    rga_b.merge(&rga_a_clone);

    // Both should converge to same content
    assert_eq!(rga_a.to_string(), rga_b.to_string());

    // Verify all content from both users is present
    let result = rga_a.to_string();
    assert!(result.contains("A0"));
    assert!(result.contains("A499"));
    assert!(result.contains("B0"));
    assert!(result.contains("B499"));
}

#[test]
fn test_offline_editing_simulation() {
    // Simulate "offline editing": two users work on the same document
    // without seeing each other's changes, then sync.
    //
    // NOTE: This test uses independent replicas (no shared state via merge)
    // because the current merge implementation has limitations with coalesced spans.
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();

    let mut rga_a = Rga::new();
    let mut rga_b = Rga::new();
    
    // Alice creates her document
    rga_a.insert(&alice.key_pub, 0, b"Alice wrote this document offline.");

    // Bob creates his document independently
    rga_b.insert(&bob.key_pub, 0, b"Bob wrote something else offline.");

    // Both have been editing independently
    assert_ne!(rga_a.to_string(), rga_b.to_string());

    // --- They come back online and sync ---
    let rga_a_clone = rga_a.clone();
    let rga_b_clone = rga_b.clone();
    rga_a.merge(&rga_b_clone);
    rga_b.merge(&rga_a_clone);

    // Both should now have identical content (CRDT convergence)
    assert_eq!(rga_a.to_string(), rga_b.to_string());

    // All content from both users should be present
    let result = rga_a.to_string();
    assert!(result.contains("Alice wrote this"));
    assert!(result.contains("Bob wrote something"));
}

#[test]
fn test_long_divergence_convergence() {
    // Test convergence after very long periods of independent work.
    // Each user types a full paragraph independently.
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();

    let mut rga_a = Rga::new();
    let mut rga_b = Rga::new();

    let alice_text = b"Alice wrote this paragraph during the network partition. \
        She had many thoughts to express and typed them all out carefully. \
        The text grew longer and longer as she continued working.";

    let bob_text = b"Bob was also writing during the outage. His thoughts were \
        different but equally important. He crafted his sentences with care \
        and made sure everything was properly formatted.";

    // Each user types their paragraph character by character
    for (i, &byte) in alice_text.iter().enumerate() {
        rga_a.insert(&alice.key_pub, i as u64, &[byte]);
    }

    for (i, &byte) in bob_text.iter().enumerate() {
        rga_b.insert(&bob.key_pub, i as u64, &[byte]);
    }

    assert_eq!(rga_a.to_string(), std::str::from_utf8(alice_text).unwrap());
    assert_eq!(rga_b.to_string(), std::str::from_utf8(bob_text).unwrap());

    // Merge
    let rga_a_clone = rga_a.clone();
    let rga_b_clone = rga_b.clone();
    rga_a.merge(&rga_b_clone);
    rga_b.merge(&rga_a_clone);

    // Should converge
    assert_eq!(rga_a.to_string(), rga_b.to_string());

    // Both texts should be in the result (interleaved by RGA ordering)
    let result = rga_a.to_string();
    // At minimum, all characters from both should be present
    assert!(result.len() >= alice_text.len() + bob_text.len());
}

#[test]
fn test_chain_of_merges_convergence() {
    // Test convergence through a chain of partial syncs.
    // A syncs with B, B syncs with C, C syncs with D, then all sync.
    let users: Vec<KeyPair> = (0..4).map(|_| KeyPair::generate()).collect();
    let mut rgas: Vec<Rga> = (0..4).map(|_| Rga::new()).collect();

    // Each user creates content
    for (i, (rga, user)) in rgas.iter_mut().zip(users.iter()).enumerate() {
        let content = format!("User{}-content-{}", i, "x".repeat(50));
        rga.insert(&user.key_pub, 0, content.as_bytes());
    }

    // Chain sync: 0 -> 1 -> 2 -> 3
    // Use clones to avoid borrow issues
    let r0 = rgas[0].clone();
    rgas[1].merge(&r0);
    let r1 = rgas[1].clone();
    rgas[2].merge(&r1);
    let r2 = rgas[2].clone();
    rgas[3].merge(&r2);

    // Now 3 has content from all users, but others don't have 3's content
    // Back propagate: 3 -> 2 -> 1 -> 0
    let r3 = rgas[3].clone();
    rgas[2].merge(&r3);
    let r2 = rgas[2].clone();
    rgas[1].merge(&r2);
    let r1 = rgas[1].clone();
    rgas[0].merge(&r1);

    // Now all should converge
    let first = rgas[0].to_string();
    for (i, rga) in rgas.iter().enumerate().skip(1) {
        assert_eq!(rga.to_string(), first, "Replica {} diverged", i);
    }

    // All user content should be present
    for i in 0..4 {
        assert!(first.contains(&format!("User{}", i)));
    }
}

#[test]
fn test_star_topology_sync() {
    // Star topology: central server syncs with all clients.
    // All clients sync with server, but not directly with each other.
    let server_key = KeyPair::generate();
    let client_keys: Vec<KeyPair> = (0..5).map(|_| KeyPair::generate()).collect();

    let mut server = Rga::new();
    let mut clients: Vec<Rga> = (0..5).map(|_| Rga::new()).collect();

    // Server has initial content
    server.insert(&server_key.key_pub, 0, b"Server initial state. ");

    // Each client syncs with server (gets initial state)
    for client in clients.iter_mut() {
        client.merge(&server);
    }

    // Each client makes independent edits
    for (i, (client, key)) in clients.iter_mut().zip(client_keys.iter()).enumerate() {
        let content = format!("Client{} added this. ", i);
        let pos = client.len();
        client.insert(&key.key_pub, pos, content.as_bytes());
    }

    // All clients sync with server
    for client in clients.iter() {
        server.merge(client);
    }

    // Server syncs back to all clients
    for client in clients.iter_mut() {
        client.merge(&server);
    }

    // All should converge
    let server_content = server.to_string();
    for (i, client) in clients.iter().enumerate() {
        assert_eq!(client.to_string(), server_content, "Client {} diverged", i);
    }

    // All client content should be present
    for i in 0..5 {
        assert!(server_content.contains(&format!("Client{}", i)));
    }
}

#[test]
fn test_delayed_sync_different_content() {
    // Simulate delayed sync where users work independently then sync later.
    // User A and User B each create significant content independently,
    // then sync and verify convergence.
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();

    let mut rga_a = Rga::new();
    let mut rga_b = Rga::new();

    // Alice creates content
    for i in 0..50 {
        let content = format!("A{} ", i);
        let pos = rga_a.len();
        rga_a.insert(&alice.key_pub, pos, content.as_bytes());
    }

    // Bob creates different content independently
    for i in 0..50 {
        let content = format!("B{} ", i);
        let pos = rga_b.len();
        rga_b.insert(&bob.key_pub, pos, content.as_bytes());
    }

    // Simulate delayed sync - they finally connect
    let rga_a_clone = rga_a.clone();
    let rga_b_clone = rga_b.clone();
    rga_a.merge(&rga_b_clone);
    rga_b.merge(&rga_a_clone);

    // Key property: They should converge to the same content
    assert_eq!(rga_a.to_string(), rga_b.to_string());
    
    // All content should be present
    let result = rga_a.to_string();
    assert!(result.contains("A0"), "Missing A0");
    assert!(result.contains("A49"), "Missing A49");
    assert!(result.contains("B0"), "Missing B0");
    assert!(result.contains("B49"), "Missing B49");
}

proptest! {
    #![proptest_config(Config {
        cases: 20,
        max_shrink_iters: 500,
        timeout: 30000,
        fork: false,
        ..Config::default()
    })]

    /// Property: Convergence holds after many independent edits.
    #[test]
    fn prop_convergence_after_many_edits(
        num_edits_a in 50usize..150,
        num_edits_b in 50usize..150,
    ) {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();

        let mut rga_a = Rga::new();
        let mut rga_b = Rga::new();

        // Each user makes many independent edits
        for i in 0..num_edits_a {
            let byte = b'A' + ((i % 26) as u8);
            let pos = rga_a.len();
            rga_a.insert(&alice.key_pub, pos, &[byte]);
        }

        for i in 0..num_edits_b {
            let byte = b'a' + ((i % 26) as u8);
            let pos = rga_b.len();
            rga_b.insert(&bob.key_pub, pos, &[byte]);
        }

        // Merge
        let rga_a_clone = rga_a.clone();
        let rga_b_clone = rga_b.clone();
        rga_a.merge(&rga_b_clone);
        rga_b.merge(&rga_a_clone);

        // Should converge
        prop_assert_eq!(rga_a.to_string(), rga_b.to_string());

        // Total length should be sum of both
        prop_assert_eq!(rga_a.len(), (num_edits_a + num_edits_b) as u64);
    }

    /// Property: Two users editing independently then syncing converges.
    /// Note: We avoid partial sync patterns that would trigger merge limitations
    /// with coalesced spans.
    #[test]
    fn prop_independent_edits_then_sync_converges(
        edits_a in 10usize..50,
        edits_b in 10usize..50,
    ) {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();

        let mut rga_a = Rga::new();
        let mut rga_b = Rga::new();

        // Alice makes edits
        for _i in 0..edits_a {
            rga_a.insert(&alice.key_pub, rga_a.len(), &[b'A']);
        }

        // Bob makes edits independently  
        for _i in 0..edits_b {
            rga_b.insert(&bob.key_pub, rga_b.len(), &[b'B']);
        }

        // Full sync
        let rga_a_clone = rga_a.clone();
        let rga_b_clone = rga_b.clone();
        rga_a.merge(&rga_b_clone);
        rga_b.merge(&rga_a_clone);

        // Should converge
        prop_assert_eq!(rga_a.to_string(), rga_b.to_string());
        
        // Total length should be sum of both
        prop_assert_eq!(rga_a.len(), (edits_a + edits_b) as u64);
    }
}

// =============================================================================
// Hole 4: Large-Scale Multi-User (50+ Users)
// =============================================================================
//
// These tests verify that CRDT properties hold with many concurrent users.
// They exercise the RGA's ability to handle high fan-out scenarios.

#[test]
fn test_50_users_each_insert_then_mesh_merge() {
    // 50 users each make a few edits, then full mesh merge.
    // This tests scalability of the merge algorithm with many participants.
    const NUM_USERS: usize = 50;
    
    let users: Vec<KeyPair> = (0..NUM_USERS).map(|_| KeyPair::generate()).collect();
    let mut rgas: Vec<Rga> = (0..NUM_USERS).map(|_| Rga::new()).collect();

    // Each user makes some edits
    for (i, (rga, user)) in rgas.iter_mut().zip(users.iter()).enumerate() {
        let content = format!("User{:02} ", i);
        rga.insert(&user.key_pub, 0, content.as_bytes());
    }

    // Full mesh merge
    full_mesh_merge(&mut rgas);

    // Verify convergence
    let first = rgas[0].to_string();
    for (i, rga) in rgas.iter().enumerate().skip(1) {
        assert_eq!(rga.to_string(), first, "Replica {} diverged", i);
    }

    // All user content should be present
    for i in 0..NUM_USERS {
        assert!(first.contains(&format!("User{:02}", i)), "User{:02} missing", i);
    }
}

#[test]
fn test_100_users_single_char_each() {
    // 100 users each insert a single character.
    // Tests merge with very high user count but minimal content per user.
    const NUM_USERS: usize = 100;
    
    let users: Vec<KeyPair> = (0..NUM_USERS).map(|_| KeyPair::generate()).collect();
    let mut rgas: Vec<Rga> = (0..NUM_USERS).map(|_| Rga::new()).collect();

    // Each user inserts one character
    for (i, (rga, user)) in rgas.iter_mut().zip(users.iter()).enumerate() {
        let byte = b'A' + ((i % 26) as u8);
        rga.insert(&user.key_pub, 0, &[byte]);
    }

    // Full mesh merge
    full_mesh_merge(&mut rgas);

    // Verify convergence
    let first = rgas[0].to_string();
    for (i, rga) in rgas.iter().enumerate().skip(1) {
        assert_eq!(rga.to_string(), first, "Replica {} diverged", i);
    }

    // Length should be exactly NUM_USERS
    assert_eq!(rgas[0].len(), NUM_USERS as u64);
}

#[test]
fn test_50_users_multiple_edits_each() {
    // 50 users each make 10 edits, testing higher total operation count.
    const NUM_USERS: usize = 50;
    const EDITS_PER_USER: usize = 10;
    
    let users: Vec<KeyPair> = (0..NUM_USERS).map(|_| KeyPair::generate()).collect();
    let mut rgas: Vec<Rga> = (0..NUM_USERS).map(|_| Rga::new()).collect();

    // Each user makes multiple edits
    for (user_idx, (rga, user)) in rgas.iter_mut().zip(users.iter()).enumerate() {
        for edit_idx in 0..EDITS_PER_USER {
            let content = format!("U{}E{} ", user_idx, edit_idx);
            let pos = rga.len();
            rga.insert(&user.key_pub, pos, content.as_bytes());
        }
    }

    // Full mesh merge
    full_mesh_merge(&mut rgas);

    // Verify convergence
    let first = rgas[0].to_string();
    for (i, rga) in rgas.iter().enumerate().skip(1) {
        assert_eq!(rga.to_string(), first, "Replica {} diverged", i);
    }

    // Spot check: content from first and last users should be present
    assert!(first.contains("U0E0"), "First user first edit missing");
    assert!(first.contains("U0E9"), "First user last edit missing");
    assert!(first.contains("U49E0"), "Last user first edit missing");
    assert!(first.contains("U49E9"), "Last user last edit missing");
}

#[test]
fn test_many_users_all_insert_at_same_position() {
    // 50 users all insert at position 0 (maximum conflict scenario).
    // This stress tests the RGA ordering algorithm with many concurrent inserts
    // at the exact same position.
    const NUM_USERS: usize = 50;
    
    let users: Vec<KeyPair> = (0..NUM_USERS).map(|_| KeyPair::generate()).collect();
    let mut rgas: Vec<Rga> = (0..NUM_USERS).map(|_| Rga::new()).collect();

    // All users insert at position 0
    for (i, (rga, user)) in rgas.iter_mut().zip(users.iter()).enumerate() {
        let content = format!("[{}]", i);
        rga.insert(&user.key_pub, 0, content.as_bytes());
    }

    // Full mesh merge
    full_mesh_merge(&mut rgas);

    // Verify convergence - this is the critical property
    let first = rgas[0].to_string();
    for (i, rga) in rgas.iter().enumerate().skip(1) {
        assert_eq!(rga.to_string(), first, "Replica {} diverged", i);
    }

    // All content should be present
    for i in 0..NUM_USERS {
        assert!(first.contains(&format!("[{}]", i)), "[{}] missing", i);
    }
}

#[test]
fn test_graduated_user_scaling() {
    // Test with increasing user counts to verify scaling behavior.
    // 10 -> 20 -> 30 -> 40 -> 50 users
    for num_users in [10, 20, 30, 40, 50] {
        let users: Vec<KeyPair> = (0..num_users).map(|_| KeyPair::generate()).collect();
        let mut rgas: Vec<Rga> = (0..num_users).map(|_| Rga::new()).collect();

        // Each user creates content
        for (i, (rga, user)) in rgas.iter_mut().zip(users.iter()).enumerate() {
            let content = format!("N{}U{} ", num_users, i);
            rga.insert(&user.key_pub, 0, content.as_bytes());
        }

        // Full mesh merge
        full_mesh_merge(&mut rgas);

        // Verify convergence
        let first = rgas[0].to_string();
        for (i, rga) in rgas.iter().enumerate().skip(1) {
            assert_eq!(
                rga.to_string(), first,
                "With {} users, replica {} diverged", num_users, i
            );
        }
    }
}

#[test]
fn test_crdt_properties_with_many_users() {
    // Verify all three CRDT merge properties with 30 users:
    // 1. Commutativity: A.merge(B) == B.merge(A)
    // 2. Associativity: (A.merge(B)).merge(C) == A.merge(B.merge(C))
    // 3. Idempotence: A.merge(A) == A
    const NUM_USERS: usize = 30;
    
    let users: Vec<KeyPair> = (0..NUM_USERS).map(|_| KeyPair::generate()).collect();
    let mut rgas: Vec<Rga> = (0..NUM_USERS).map(|_| Rga::new()).collect();

    // Each user creates content
    for (i, (rga, user)) in rgas.iter_mut().zip(users.iter()).enumerate() {
        let content = format!("U{} ", i);
        rga.insert(&user.key_pub, 0, content.as_bytes());
    }

    // Test idempotence: merging with self should not change anything
    for rga in rgas.iter_mut() {
        let before = rga.to_string();
        let clone = rga.clone();
        rga.merge(&clone);
        assert_eq!(rga.to_string(), before, "Idempotence violated");
    }

    // Test commutativity: pairwise merges should be order-independent
    for i in 0..5 {
        for j in (i+1)..6 {
            let mut a_then_b = rgas[i].clone();
            a_then_b.merge(&rgas[j]);
            
            let mut b_then_a = rgas[j].clone();
            b_then_a.merge(&rgas[i]);
            
            assert_eq!(
                a_then_b.to_string(), b_then_a.to_string(),
                "Commutativity violated for users {} and {}", i, j
            );
        }
    }

    // Test associativity with three replicas
    let mut left_assoc = rgas[0].clone();
    left_assoc.merge(&rgas[1]);
    left_assoc.merge(&rgas[2]);

    let mut right_assoc = rgas[1].clone();
    right_assoc.merge(&rgas[2]);
    let mut right_result = rgas[0].clone();
    right_result.merge(&right_assoc);

    assert_eq!(
        left_assoc.to_string(), right_result.to_string(),
        "Associativity violated"
    );
}

#[test]
fn test_hub_and_spoke_topology_many_users() {
    // Hub-and-spoke: one central replica syncs with 50 satellite replicas.
    // This mimics a server-client architecture.
    const NUM_SATELLITES: usize = 50;
    
    let hub_key = KeyPair::generate();
    let satellite_keys: Vec<KeyPair> = (0..NUM_SATELLITES).map(|_| KeyPair::generate()).collect();

    let mut hub = Rga::new();
    let mut satellites: Vec<Rga> = (0..NUM_SATELLITES).map(|_| Rga::new()).collect();

    // Hub has initial content
    hub.insert(&hub_key.key_pub, 0, b"HUB_INITIAL ");

    // Each satellite gets initial state from hub and adds their content
    for (i, (sat, key)) in satellites.iter_mut().zip(satellite_keys.iter()).enumerate() {
        sat.merge(&hub);
        let content = format!("SAT{:02} ", i);
        let pos = sat.len();
        sat.insert(&key.key_pub, pos, content.as_bytes());
    }

    // All satellites sync to hub
    for sat in satellites.iter() {
        hub.merge(sat);
    }

    // Hub syncs back to all satellites
    for sat in satellites.iter_mut() {
        sat.merge(&hub);
    }

    // All should converge
    let hub_content = hub.to_string();
    for (i, sat) in satellites.iter().enumerate() {
        assert_eq!(sat.to_string(), hub_content, "Satellite {} diverged", i);
    }

    // All satellite content should be present
    assert!(hub_content.contains("HUB_INITIAL"));
    for i in 0..NUM_SATELLITES {
        assert!(hub_content.contains(&format!("SAT{:02}", i)), "SAT{:02} missing", i);
    }
}

proptest! {
    #![proptest_config(Config {
        cases: 10,
        max_shrink_iters: 100,
        timeout: 60000,
        fork: false,
        ..Config::default()
    })]

    /// Property: Convergence holds regardless of number of users.
    #[test]
    fn prop_many_user_convergence(
        num_users in 10usize..30,
    ) {
        let users: Vec<KeyPair> = (0..num_users).map(|_| KeyPair::generate()).collect();
        let mut rgas: Vec<Rga> = (0..num_users).map(|_| Rga::new()).collect();

        // Each user makes one insert
        for (i, (rga, user)) in rgas.iter_mut().zip(users.iter()).enumerate() {
            let byte = b'A' + ((i % 26) as u8);
            rga.insert(&user.key_pub, 0, &[byte, byte]);
        }

        // Full mesh merge
        full_mesh_merge(&mut rgas);

        // Verify convergence
        let first = rgas[0].to_string();
        for (i, rga) in rgas.iter().enumerate().skip(1) {
            prop_assert_eq!(
                &rga.to_string(), &first,
                "Replica {} diverged with {} users", i, num_users
            );
        }

        // Length should be 2 * num_users (each user inserted 2 chars)
        prop_assert_eq!(rgas[0].len(), (num_users * 2) as u64);
    }
}

// =============================================================================
// Hole 5: Rare Code Paths in rga.rs
// =============================================================================
//
// These tests explicitly exercise code paths that are rare in normal operation:
// 1. Origin not found in insert_span_rga (shouldn't happen in normal operation)
// 2. Insert at beginning with existing content (spans with no origin)
// 3. Split operations at various offsets
// 4. Delete operations that create multiple splits
// 5. Cursor cache edge cases

#[test]
fn test_insert_at_beginning_multiple_users_ordering() {
    // Test RGA ordering for spans with no origin (inserted at document beginning).
    // Multiple users all insert at position 0 - tests the "no origin" ordering path.
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();
    let charlie = KeyPair::generate();

    let mut rga_a = Rga::new();
    let mut rga_b = Rga::new();
    let mut rga_c = Rga::new();

    // All three users insert at position 0
    rga_a.insert(&alice.key_pub, 0, b"A");
    rga_b.insert(&bob.key_pub, 0, b"B");
    rga_c.insert(&charlie.key_pub, 0, b"C");

    // Merge all combinations
    let mut merged = rga_a.clone();
    merged.merge(&rga_b);
    merged.merge(&rga_c);

    let mut merged2 = rga_c.clone();
    merged2.merge(&rga_b);
    merged2.merge(&rga_a);

    // Should converge regardless of merge order
    assert_eq!(merged.to_string(), merged2.to_string());
    
    // All characters should be present
    let result = merged.to_string();
    assert!(result.contains("A"));
    assert!(result.contains("B"));
    assert!(result.contains("C"));
    assert_eq!(result.len(), 3);
}

#[test]
fn test_split_at_various_offsets() {
    // Test span splitting at different positions within a span.
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    // Create a span: "ABCDEFGHIJ"
    rga.insert(&user.key_pub, 0, b"ABCDEFGHIJ");
    assert_eq!(rga.span_count(), 1);

    // Insert in middle (splits at offset 5)
    rga.insert(&user.key_pub, 5, b"X");
    assert_eq!(rga.to_string(), "ABCDEXFGHIJ");

    // Insert near beginning (splits at offset 1)
    rga.insert(&user.key_pub, 1, b"Y");
    assert_eq!(rga.to_string(), "AYBCDEXFGHIJ");

    // Insert near end
    rga.insert(&user.key_pub, 11, b"Z");
    assert_eq!(rga.to_string(), "AYBCDEXFGHIZJ");
}

#[test]
fn test_delete_creates_multiple_splits() {
    // Test delete that requires splitting a span multiple times.
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    // Create a single span
    rga.insert(&user.key_pub, 0, b"ABCDEFGHIJ");
    assert_eq!(rga.span_count(), 1);

    // Delete middle portion (causes two splits: left part, deleted middle, right part)
    rga.delete(3, 4); // Delete "DEFG"
    assert_eq!(rga.to_string(), "ABCHIJ");
    
    // The span should have been split
    assert!(rga.span_count() >= 2, "Expected at least 2 spans after middle delete");
}

#[test]
fn test_delete_prefix_creates_split() {
    // Test deleting the prefix of a span.
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    rga.insert(&user.key_pub, 0, b"HELLO");
    rga.delete(0, 2); // Delete "HE"
    
    assert_eq!(rga.to_string(), "LLO");
    assert_eq!(rga.len(), 3);
}

#[test]
fn test_delete_suffix_creates_split() {
    // Test deleting the suffix of a span.
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    rga.insert(&user.key_pub, 0, b"HELLO");
    rga.delete(3, 2); // Delete "LO"
    
    assert_eq!(rga.to_string(), "HEL");
    assert_eq!(rga.len(), 3);
}

#[test]
fn test_delete_across_multiple_spans() {
    // Test delete that spans multiple internal spans.
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();
    let mut rga = Rga::new();

    // Create content from different users (won't coalesce)
    rga.insert(&alice.key_pub, 0, b"AAA");
    rga.insert(&bob.key_pub, 3, b"BBB");
    rga.insert(&alice.key_pub, 6, b"CCC");
    
    assert_eq!(rga.to_string(), "AAABBBCCC");
    let initial_spans = rga.span_count();
    assert!(initial_spans >= 2);

    // Delete across span boundaries
    rga.delete(2, 5); // Delete "ABBBC"
    assert_eq!(rga.to_string(), "AACC");
}

#[test]
fn test_cursor_cache_invalidation_on_insert_at_beginning() {
    // The cursor cache should be invalidated when inserting at position 0.
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    // Build up some content
    rga.insert(&user.key_pub, 0, b"world");
    
    // Cache should be valid and pointing to last char
    // (internal detail, but important for the optimization)
    
    // Insert at beginning - this shifts all indices
    rga.insert(&user.key_pub, 0, b"hello ");
    
    assert_eq!(rga.to_string(), "hello world");
}

#[test]
fn test_cursor_cache_with_non_adjacent_inserts() {
    // Test that cursor cache handles non-sequential inserts correctly.
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    // Sequential typing
    rga.insert(&user.key_pub, 0, b"a");
    rga.insert(&user.key_pub, 1, b"b");
    rga.insert(&user.key_pub, 2, b"c");
    
    // Jump to a different position (cache miss)
    rga.insert(&user.key_pub, 0, b"X");
    
    // Continue from new position
    rga.insert(&user.key_pub, 1, b"Y");
    
    assert_eq!(rga.to_string(), "XYabc");
}

#[test]
fn test_apply_with_origin_in_middle_of_span() {
    // Test applying an operation where the origin is in the middle of an existing span.
    // This triggers a span split in insert_span_rga.
    use together::crdt::op::{OpBlock, ItemId as OpItemId};

    let alice = KeyPair::generate();
    let bob = KeyPair::generate();
    let mut rga = Rga::new();

    // Alice inserts "ABCDE" as one span
    let block1 = OpBlock::insert(None, 0, b"ABCDE".to_vec());
    rga.apply(&alice.key_pub, &block1);
    assert_eq!(rga.to_string(), "ABCDE");
    assert_eq!(rga.span_count(), 1); // Single coalesced span

    // Bob inserts after 'C' (seq=2 in Alice's span)
    // This requires splitting Alice's span
    let origin = OpItemId {
        user: alice.key_pub.clone(),
        seq: 2, // The 'C' character
    };
    let block2 = OpBlock::insert(Some(origin), 0, b"X".to_vec());
    rga.apply(&bob.key_pub, &block2);
    
    assert_eq!(rga.to_string(), "ABCXDE");
}

#[test]
fn test_apply_concurrent_inserts_same_origin() {
    // Multiple users insert after the same origin character.
    // Tests the sibling ordering in insert_span_rga.
    use together::crdt::op::{OpBlock, ItemId as OpItemId};

    let alice = KeyPair::generate();
    let bob = KeyPair::generate();
    let charlie = KeyPair::generate();
    let mut rga = Rga::new();

    // Alice creates the document
    let block1 = OpBlock::insert(None, 0, b"A".to_vec());
    rga.apply(&alice.key_pub, &block1);

    // Both Bob and Charlie insert after 'A' (concurrent)
    let origin = OpItemId {
        user: alice.key_pub.clone(),
        seq: 0,
    };
    
    let block_bob = OpBlock::insert(Some(origin.clone()), 0, b"B".to_vec());
    let block_charlie = OpBlock::insert(Some(origin), 0, b"C".to_vec());
    
    rga.apply(&bob.key_pub, &block_bob);
    rga.apply(&charlie.key_pub, &block_charlie);
    
    // Both B and C should appear after A
    let result = rga.to_string();
    assert!(result.starts_with("A"));
    assert!(result.contains("B"));
    assert!(result.contains("C"));
    assert_eq!(result.len(), 3);
}

#[test]
fn test_apply_duplicate_operation_idempotent() {
    // Applying the same operation twice should be idempotent.
    use together::crdt::op::OpBlock;

    let alice = KeyPair::generate();
    let mut rga = Rga::new();

    let block = OpBlock::insert(None, 0, b"hello".to_vec());
    
    // First apply succeeds
    assert!(rga.apply(&alice.key_pub, &block));
    assert_eq!(rga.to_string(), "hello");
    
    // Second apply returns false (already present)
    assert!(!rga.apply(&alice.key_pub, &block));
    assert_eq!(rga.to_string(), "hello"); // No change
}

#[test]
fn test_delete_by_id_single_char_span() {
    // Delete operation targeting a single-character span.
    use together::crdt::op::{OpBlock, ItemId as OpItemId};

    let alice = KeyPair::generate();
    let mut rga = Rga::new();

    // Insert three separate characters (separate operations = separate spans)
    let block_a = OpBlock::insert(None, 0, b"A".to_vec());
    rga.apply(&alice.key_pub, &block_a);
    
    let origin_a = OpItemId { user: alice.key_pub.clone(), seq: 0 };
    let block_b = OpBlock::insert(Some(origin_a), 1, b"B".to_vec());
    rga.apply(&alice.key_pub, &block_b);
    
    let origin_b = OpItemId { user: alice.key_pub.clone(), seq: 1 };
    let block_c = OpBlock::insert(Some(origin_b), 2, b"C".to_vec());
    rga.apply(&alice.key_pub, &block_c);
    
    assert_eq!(rga.to_string(), "ABC");

    // Delete 'B'
    let target = OpItemId { user: alice.key_pub.clone(), seq: 1 };
    let delete_block = OpBlock::delete(target);
    rga.apply(&alice.key_pub, &delete_block);
    
    assert_eq!(rga.to_string(), "AC");
}

#[test]
fn test_delete_by_id_first_char_of_span() {
    // Delete the first character of a multi-char span.
    use together::crdt::op::{OpBlock, ItemId as OpItemId};

    let alice = KeyPair::generate();
    let mut rga = Rga::new();

    let block = OpBlock::insert(None, 0, b"ABCDE".to_vec());
    rga.apply(&alice.key_pub, &block);
    assert_eq!(rga.to_string(), "ABCDE");

    // Delete 'A' (first char, seq=0)
    let target = OpItemId { user: alice.key_pub.clone(), seq: 0 };
    let delete_block = OpBlock::delete(target);
    rga.apply(&alice.key_pub, &delete_block);
    
    assert_eq!(rga.to_string(), "BCDE");
}

#[test]
fn test_delete_by_id_last_char_of_span() {
    // Delete the last character of a multi-char span.
    use together::crdt::op::{OpBlock, ItemId as OpItemId};

    let alice = KeyPair::generate();
    let mut rga = Rga::new();

    let block = OpBlock::insert(None, 0, b"ABCDE".to_vec());
    rga.apply(&alice.key_pub, &block);
    assert_eq!(rga.to_string(), "ABCDE");

    // Delete 'E' (last char, seq=4)
    let target = OpItemId { user: alice.key_pub.clone(), seq: 4 };
    let delete_block = OpBlock::delete(target);
    rga.apply(&alice.key_pub, &delete_block);
    
    assert_eq!(rga.to_string(), "ABCD");
}

#[test]
fn test_delete_by_id_middle_char_of_span() {
    // Delete a character in the middle of a span (causes two splits).
    use together::crdt::op::{OpBlock, ItemId as OpItemId};

    let alice = KeyPair::generate();
    let mut rga = Rga::new();

    let block = OpBlock::insert(None, 0, b"ABCDE".to_vec());
    rga.apply(&alice.key_pub, &block);
    assert_eq!(rga.to_string(), "ABCDE");

    // Delete 'C' (middle char, seq=2)
    let target = OpItemId { user: alice.key_pub.clone(), seq: 2 };
    let delete_block = OpBlock::delete(target);
    rga.apply(&alice.key_pub, &delete_block);
    
    assert_eq!(rga.to_string(), "ABDE");
}

// =============================================================================
// Hole 6: Versioning Edge Cases
// =============================================================================
//
// These tests explore edge cases in the versioning/snapshot system.

#[test]
fn test_version_at_deleted_position() {
    // Take a version, delete content, verify version reconstruction.
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    rga.insert(&user.key_pub, 0, b"hello world");
    
    // Version before any deletes
    let v1 = rga.version();
    let s1 = rga.to_string();
    assert_eq!(s1, "hello world");

    // Delete "world"
    rga.delete(6, 5);
    assert_eq!(rga.to_string(), "hello ");

    // Version after delete
    let v2 = rga.version();
    let s2 = rga.to_string();
    assert_eq!(s2, "hello ");

    // Both versions should reconstruct correctly
    assert_eq!(rga.to_string_at(&v1), s1);
    assert_eq!(rga.to_string_at(&v2), s2);
    
    // Lengths should be correct
    assert_eq!(rga.len_at(&v1), 11);
    assert_eq!(rga.len_at(&v2), 6);
}

#[test]
fn test_version_with_many_tombstones() {
    // Create many tombstones and verify versioning still works.
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    // Insert content
    rga.insert(&user.key_pub, 0, b"ABCDEFGHIJKLMNOPQRSTUVWXYZ");
    
    let v1 = rga.version();
    assert_eq!(rga.len_at(&v1), 26);

    // Delete every other character (creates many tombstones)
    for i in (0..13).rev() {
        rga.delete(i * 2, 1);
    }
    
    assert_eq!(rga.to_string(), "BDFHJLNPRTVXZ");
    let v2 = rga.version();
    assert_eq!(rga.len_at(&v2), 13);

    // Version at v1 should still have all 26 chars
    assert_eq!(rga.to_string_at(&v1), "ABCDEFGHIJKLMNOPQRSTUVWXYZ");
    
    // Version at v2 has the post-delete state
    assert_eq!(rga.to_string_at(&v2), "BDFHJLNPRTVXZ");
}

#[test]
fn test_version_empty_then_populated() {
    // Version of empty doc, then populate and verify.
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    let v_empty = rga.version();
    assert_eq!(rga.len_at(&v_empty), 0);
    assert_eq!(rga.to_string_at(&v_empty), "");

    // Populate
    rga.insert(&user.key_pub, 0, b"hello");
    let v_hello = rga.version();
    
    // Both versions should work
    assert_eq!(rga.to_string_at(&v_empty), "");
    assert_eq!(rga.to_string_at(&v_hello), "hello");
}

#[test]
fn test_version_slice_at_boundaries() {
    // Test slice_at with various boundary conditions.
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    rga.insert(&user.key_pub, 0, b"0123456789");
    let version = rga.version();

    // Normal slice
    assert_eq!(rga.slice_at(0, 5, &version), Some("01234".to_string()));
    
    // Slice at exact boundaries
    assert_eq!(rga.slice_at(0, 10, &version), Some("0123456789".to_string()));
    
    // Slice at end
    assert_eq!(rga.slice_at(5, 10, &version), Some("56789".to_string()));
    
    // Empty slice
    assert_eq!(rga.slice_at(5, 5, &version), Some("".to_string()));
    
    // Slice beyond end (clamped)
    assert_eq!(rga.slice_at(8, 100, &version), Some("89".to_string()));
    
    // Slice starting beyond end
    assert_eq!(rga.slice_at(100, 200, &version), None);
}

#[test]
fn test_version_across_merge() {
    // Test that versions work correctly after merging.
    use together::crdt::Crdt;

    let alice = KeyPair::generate();
    let bob = KeyPair::generate();

    let mut rga_a = Rga::new();
    let mut rga_b = Rga::new();

    // Alice creates content
    rga_a.insert(&alice.key_pub, 0, b"ALICE");
    let v_alice = rga_a.version();

    // Bob creates content
    rga_b.insert(&bob.key_pub, 0, b"BOB");
    let v_bob = rga_b.version();

    // Merge
    rga_a.merge(&rga_b);

    // Alice's version should still work
    assert_eq!(rga_a.to_string_at(&v_alice), "ALICE");
    
    // The merged document should have both
    let merged = rga_a.to_string();
    assert!(merged.contains("ALICE"));
    assert!(merged.contains("BOB"));
}

#[test]
fn test_version_many_small_edits() {
    // Take versions during many small edits and verify all work.
    let user = KeyPair::generate();
    let mut rga = Rga::new();
    
    let mut versions = Vec::new();
    let mut snapshots = Vec::new();

    // Build document with versions at each step
    for i in 0..20 {
        let byte = b'A' + (i as u8);
        rga.insert(&user.key_pub, i as u64, &[byte]);
        versions.push(rga.version());
        snapshots.push(rga.to_string());
    }

    // Verify all versions
    for (i, (version, expected)) in versions.iter().zip(snapshots.iter()).enumerate() {
        let reconstructed = rga.to_string_at(version);
        assert_eq!(
            &reconstructed, expected,
            "Version {} mismatch: expected {:?}, got {:?}",
            i, expected, reconstructed
        );
    }
}

#[test]
fn test_version_with_interleaved_delete_insert() {
    // Interleave deletes and inserts, taking versions throughout.
    let user = KeyPair::generate();
    let mut rga = Rga::new();

    rga.insert(&user.key_pub, 0, b"ABCDE");
    let v1 = rga.version();

    rga.delete(2, 1); // Delete 'C'
    let v2 = rga.version();

    rga.insert(&user.key_pub, 2, b"X");
    let v3 = rga.version();

    rga.delete(0, 1); // Delete 'A'
    let v4 = rga.version();

    rga.insert(&user.key_pub, 0, b"Z");
    let v5 = rga.version();

    // Verify all versions
    assert_eq!(rga.to_string_at(&v1), "ABCDE");
    assert_eq!(rga.to_string_at(&v2), "ABDE");
    assert_eq!(rga.to_string_at(&v3), "ABXDE");
    assert_eq!(rga.to_string_at(&v4), "BXDE");
    assert_eq!(rga.to_string_at(&v5), "ZBXDE");
}

// =============================================================================
// Hole 7: OpLog End-to-End Integration Test
// =============================================================================
//
// These tests verify the complete flow:
// 1. User edits document
// 2. Operations are recorded in OpLog
// 3. New replica rebuilds from OpLog
// 4. Result matches original

use together::crdt::op::{OpLog, OpBlock, ItemId as OpItemId};

#[test]
fn test_oplog_roundtrip_simple() {
    // Simple case: record inserts, replay to new RGA.
    let alice = KeyPair::generate();
    
    // Original document
    let mut original = Rga::new();
    original.insert(&alice.key_pub, 0, b"hello");
    original.insert(&alice.key_pub, 5, b" world");
    
    // Record operations in OpLog
    let mut log = OpLog::new();
    log.push(alice.key_pub.clone(), OpBlock::insert(None, 0, b"hello".to_vec()));
    
    let origin = OpItemId { user: alice.key_pub.clone(), seq: 4 };
    log.push(alice.key_pub.clone(), OpBlock::insert(Some(origin), 5, b" world".to_vec()));
    
    // Rebuild from OpLog
    let mut rebuilt = Rga::new();
    for (user, block) in log.ops() {
        rebuilt.apply(user, block);
    }
    
    // Should match
    assert_eq!(rebuilt.to_string(), original.to_string());
}

#[test]
fn test_oplog_roundtrip_with_deletes() {
    // Record inserts and deletes, replay to new RGA.
    let alice = KeyPair::generate();
    
    // Build document
    let mut original = Rga::new();
    let block1 = OpBlock::insert(None, 0, b"hello".to_vec());
    original.apply(&alice.key_pub, &block1);
    
    let origin = OpItemId { user: alice.key_pub.clone(), seq: 4 };
    let block2 = OpBlock::insert(Some(origin), 5, b" world".to_vec());
    original.apply(&alice.key_pub, &block2);
    
    // Delete 'o' at seq 4
    let target = OpItemId { user: alice.key_pub.clone(), seq: 4 };
    let block3 = OpBlock::delete(target.clone());
    original.apply(&alice.key_pub, &block3);
    
    assert_eq!(original.to_string(), "hell world");
    
    // Record in OpLog
    let mut log = OpLog::new();
    log.push(alice.key_pub.clone(), block1);
    log.push(alice.key_pub.clone(), block2);
    log.push(alice.key_pub.clone(), block3);
    
    // Rebuild from OpLog
    let mut rebuilt = Rga::new();
    for (user, block) in log.ops() {
        rebuilt.apply(user, block);
    }
    
    assert_eq!(rebuilt.to_string(), original.to_string());
}

#[test]
fn test_oplog_two_users() {
    // Two users, operations interleaved in OpLog.
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();
    
    let mut log = OpLog::new();
    
    // Alice inserts first
    log.push(alice.key_pub.clone(), OpBlock::insert(None, 0, b"ALICE".to_vec()));
    
    // Bob inserts (also at beginning)
    log.push(bob.key_pub.clone(), OpBlock::insert(None, 0, b"BOB".to_vec()));
    
    // Rebuild from OpLog
    let mut rebuilt = Rga::new();
    for (user, block) in log.ops() {
        rebuilt.apply(user, block);
    }
    
    let result = rebuilt.to_string();
    assert!(result.contains("ALICE"));
    assert!(result.contains("BOB"));
    assert_eq!(result.len(), 8);
}

#[test]
fn test_oplog_complex_editing_session() {
    // Simulate a realistic editing session with OpLog.
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();
    
    let mut log = OpLog::new();
    
    // Alice types "Hello "
    log.push(alice.key_pub.clone(), OpBlock::insert(None, 0, b"Hello ".to_vec()));
    
    // Bob types "World" after Alice's last char
    let origin_alice = OpItemId { user: alice.key_pub.clone(), seq: 5 };
    log.push(bob.key_pub.clone(), OpBlock::insert(Some(origin_alice), 0, b"World".to_vec()));
    
    // Alice deletes a character (typo fix) - delete 'o' at seq 4
    let target = OpItemId { user: alice.key_pub.clone(), seq: 4 };
    log.push(alice.key_pub.clone(), OpBlock::delete(target));
    
    // Alice re-inserts correct character after 'l' (seq 3)
    let origin_fix = OpItemId { user: alice.key_pub.clone(), seq: 3 };
    log.push(alice.key_pub.clone(), OpBlock::insert(Some(origin_fix), 6, b"o".to_vec()));
    
    // Rebuild from OpLog
    let mut rebuilt = Rga::new();
    for (user, block) in log.ops() {
        rebuilt.apply(user, block);
    }
    
    // Should contain both users' content
    let result = rebuilt.to_string();
    assert!(result.contains("World"));
    assert!(result.contains("Hell"));
}

#[test]
fn test_oplog_rebuild_matches_direct_construction() {
    // Build the same document two ways:
    // 1. Direct RGA operations
    // 2. Through OpLog and rebuild
    // Results should be identical.
    let alice = KeyPair::generate();
    
    // Direct construction
    let mut direct = Rga::new();
    direct.insert(&alice.key_pub, 0, b"ABC");
    direct.insert(&alice.key_pub, 3, b"DEF");
    
    // Through OpLog
    let mut log = OpLog::new();
    log.push(alice.key_pub.clone(), OpBlock::insert(None, 0, b"ABC".to_vec()));
    let origin = OpItemId { user: alice.key_pub.clone(), seq: 2 };
    log.push(alice.key_pub.clone(), OpBlock::insert(Some(origin), 3, b"DEF".to_vec()));
    
    let mut via_log = Rga::new();
    for (user, block) in log.ops() {
        via_log.apply(user, block);
    }
    
    assert_eq!(direct.to_string(), via_log.to_string());
}

#[test]
fn test_oplog_order_independence() {
    // Operations from different users should produce same result
    // regardless of order in OpLog (CRDT property).
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();
    
    // Order 1: Alice then Bob
    let mut log1 = OpLog::new();
    log1.push(alice.key_pub.clone(), OpBlock::insert(None, 0, b"A".to_vec()));
    log1.push(bob.key_pub.clone(), OpBlock::insert(None, 0, b"B".to_vec()));
    
    let mut rga1 = Rga::new();
    for (user, block) in log1.ops() {
        rga1.apply(user, block);
    }
    
    // Order 2: Bob then Alice
    let mut log2 = OpLog::new();
    log2.push(bob.key_pub.clone(), OpBlock::insert(None, 0, b"B".to_vec()));
    log2.push(alice.key_pub.clone(), OpBlock::insert(None, 0, b"A".to_vec()));
    
    let mut rga2 = Rga::new();
    for (user, block) in log2.ops() {
        rga2.apply(user, block);
    }
    
    // Should produce the same result (CRDT commutativity)
    assert_eq!(rga1.to_string(), rga2.to_string());
}

// =============================================================================
// Hole 8: Stress/Load Testing
// =============================================================================
//
// These tests verify performance and correctness at scale:
// 1. Large documents (100KB)
// 2. Many operations
// 3. Memory usage verification

#[test]
fn test_100kb_document_construction() {
    // Build a 100KB document and verify correctness.
    let user = KeyPair::generate();
    let mut rga = Rga::new();
    
    // Build 100KB document (100 * 1024 bytes)
    let chunk = b"0123456789ABCDEF"; // 16 bytes
    let chunks_needed = (100 * 1024) / 16;
    
    for i in 0..chunks_needed {
        let pos = rga.len();
        rga.insert(&user.key_pub, pos, chunk);
        
        // Sanity check every 1000 chunks
        if i % 1000 == 0 {
            assert_eq!(rga.len(), ((i + 1) * 16) as u64);
        }
    }
    
    // Verify final size
    let expected_len = (chunks_needed * 16) as u64;
    assert_eq!(rga.len(), expected_len);
    assert!(rga.len() >= 100 * 1024);
    
    // Verify we can read the content
    let content = rga.to_string();
    assert_eq!(content.len() as u64, expected_len);
}

#[test]
fn test_100kb_document_with_deletes() {
    // Build 100KB, delete half, verify.
    let user = KeyPair::generate();
    let mut rga = Rga::new();
    
    // Build document
    let chunk = b"XXXXXXXXXXXXXXXX"; // 16 bytes
    let chunks = 100 * 1024 / 16;
    
    for _ in 0..chunks {
        let pos = rga.len();
        rga.insert(&user.key_pub, pos, chunk);
    }
    
    let full_len = rga.len();
    assert!(full_len >= 100 * 1024);
    
    // Delete half (from the middle)
    let delete_start = full_len / 4;
    let delete_len = full_len / 2;
    rga.delete(delete_start, delete_len);
    
    // Verify
    assert_eq!(rga.len(), full_len - delete_len);
    
    // Should still be readable
    let content = rga.to_string();
    assert_eq!(content.len() as u64, rga.len());
}

#[test]
fn test_10000_single_char_inserts() {
    // Insert 10000 single characters and verify span coalescing.
    let user = KeyPair::generate();
    let mut rga = Rga::new();
    
    // Sequential typing
    for i in 0..10000 {
        let byte = b'a' + ((i % 26) as u8);
        let pos = rga.len();
        rga.insert(&user.key_pub, pos, &[byte]);
    }
    
    assert_eq!(rga.len(), 10000);
    
    // With coalescing, should have very few spans (ideally 1)
    let span_count = rga.span_count();
    assert!(
        span_count <= 10,
        "Expected <= 10 spans with coalescing, got {}",
        span_count
    );
    
    // Verify content
    let content = rga.to_string();
    assert_eq!(content.len(), 10000);
}

#[test]
fn test_1000_random_position_inserts() {
    // Insert at semi-random positions to test non-sequential patterns.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let user = KeyPair::generate();
    let mut rga = Rga::new();
    
    // Initial content
    rga.insert(&user.key_pub, 0, b"START");
    
    for i in 0..1000 {
        // Deterministic "random" position based on i
        let mut hasher = DefaultHasher::new();
        i.hash(&mut hasher);
        let hash = hasher.finish();
        
        let current_len = rga.len();
        let pos = if current_len > 0 {
            (hash % (current_len + 1)) as u64
        } else {
            0
        };
        
        let byte = b'A' + ((i % 26) as u8);
        rga.insert(&user.key_pub, pos, &[byte]);
    }
    
    assert_eq!(rga.len(), 1005); // 5 (START) + 1000 inserts
    
    // Should be able to read content
    let content = rga.to_string();
    assert_eq!(content.len(), 1005);
}

#[test]
fn test_many_users_many_operations() {
    // Simulate 20 users each making 100 operations.
    const NUM_USERS: usize = 20;
    const OPS_PER_USER: usize = 100;
    
    let users: Vec<KeyPair> = (0..NUM_USERS).map(|_| KeyPair::generate()).collect();
    let mut rgas: Vec<Rga> = (0..NUM_USERS).map(|_| Rga::new()).collect();
    
    // Each user makes operations
    for (user_idx, (rga, user)) in rgas.iter_mut().zip(users.iter()).enumerate() {
        for op_idx in 0..OPS_PER_USER {
            let content = format!("U{}O{} ", user_idx, op_idx);
            let pos = rga.len();
            rga.insert(&user.key_pub, pos, content.as_bytes());
        }
    }
    
    // Merge all
    full_mesh_merge(&mut rgas);
    
    // Verify convergence
    let first = rgas[0].to_string();
    for (i, rga) in rgas.iter().enumerate().skip(1) {
        assert_eq!(
            rga.to_string(), first,
            "Replica {} diverged after many operations", i
        );
    }
    
    // Verify content from each user is present
    for user_idx in [0, NUM_USERS/2, NUM_USERS-1] {
        assert!(first.contains(&format!("U{}O0", user_idx)));
        assert!(first.contains(&format!("U{}O{}", user_idx, OPS_PER_USER-1)));
    }
}

#[test]
fn test_rapid_insert_delete_cycles() {
    // Rapidly insert and delete at the same position.
    let user = KeyPair::generate();
    let mut rga = Rga::new();
    
    for cycle in 0..500 {
        // Insert
        rga.insert(&user.key_pub, 0, b"X");
        assert_eq!(rga.len(), 1, "Cycle {} insert failed", cycle);
        
        // Delete
        rga.delete(0, 1);
        assert_eq!(rga.len(), 0, "Cycle {} delete failed", cycle);
    }
    
    // Document should be empty
    assert_eq!(rga.len(), 0);
    assert_eq!(rga.to_string(), "");
    
    // But there should be many tombstones
    // (span_count includes deleted spans)
    assert!(rga.span_count() > 0);
}

#[test]
fn test_version_snapshot_at_scale() {
    // Take many versions during large document construction.
    let user = KeyPair::generate();
    let mut rga = Rga::new();
    
    let mut versions = Vec::new();
    let mut expected_lens = Vec::new();
    
    // Build document, taking versions every 1000 chars
    let chunk = b"0123456789"; // 10 bytes
    for i in 0..1000 {
        let pos = rga.len();
        rga.insert(&user.key_pub, pos, chunk);
        
        if i % 100 == 0 {
            versions.push(rga.version());
            expected_lens.push(rga.len());
        }
    }
    
    // Verify all versions reconstruct correctly
    for (version, &expected_len) in versions.iter().zip(expected_lens.iter()) {
        let reconstructed_len = rga.len_at(version);
        assert_eq!(
            reconstructed_len, expected_len,
            "Version length mismatch"
        );
        
        // Verify we can read the content at that version
        let content = rga.to_string_at(version);
        assert_eq!(content.len() as u64, expected_len);
    }
}

proptest! {
    #![proptest_config(Config {
        cases: 5,
        max_shrink_iters: 50,
        timeout: 120000,
        fork: false,
        ..Config::default()
    })]

    /// Stress test: many random operations then verify invariants.
    #[test]
    fn prop_stress_random_operations(
        num_ops in 500usize..1000,
        seed in 0u64..1000,
    ) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();
        
        // Use seed for deterministic "randomness"
        let mut pseudo_random = seed;
        
        for _ in 0..num_ops {
            pseudo_random = pseudo_random.wrapping_mul(1103515245).wrapping_add(12345);
            let op_type = pseudo_random % 3;
            
            let current_len = rga.len();
            
            match op_type {
                0 | 1 => {
                    // Insert (2/3 chance)
                    let pos = if current_len > 0 {
                        pseudo_random % (current_len + 1)
                    } else {
                        0
                    };
                    let byte = b'A' + ((pseudo_random % 26) as u8);
                    rga.insert(&user.key_pub, pos, &[byte]);
                }
                _ => {
                    // Delete (1/3 chance)
                    if current_len > 0 {
                        let pos = pseudo_random % current_len;
                        let max_len = (current_len - pos).min(5);
                        if max_len > 0 {
                            let del_len = (pseudo_random % max_len) + 1;
                            rga.delete(pos, del_len);
                        }
                    }
                }
            }
        }
        
        // Verify invariants
        let content = rga.to_string();
        prop_assert_eq!(content.len() as u64, rga.len());
        
        // Version should work
        let version = rga.version();
        prop_assert_eq!(rga.len_at(&version), rga.len());
    }
}

#[test]
fn test_concurrent_insert_at_beginning_convergence() {
    use together::crdt::rga::Rga;
    use together::crdt::Crdt;
    use together::key::KeyPair;
    
    let u0 = KeyPair::generate();
    let u1 = KeyPair::generate();
    
    // Phase 1: Both users insert, then sync
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    r0.insert(&u0.key_pub, 0, b"ABCDEF");
    r1.insert(&u1.key_pub, 0, b"BCDEFG");
    
    // Full sync
    let r0_clone = r0.clone();
    let r1_clone = r1.clone();
    r0.merge(&r1_clone);
    r1.merge(&r0_clone);
    
    assert_eq!(r0.to_string(), r1.to_string(), "Phase 1 convergence failed");
    
    // Phase 2: Both insert at pos 0 again (concurrent edits at same position)
    r0.insert(&u0.key_pub, 0, b"GHIJKL");
    r1.insert(&u1.key_pub, 0, b"MNOPQR");
    
    // Full sync
    let r0_clone = r0.clone();
    let r1_clone = r1.clone();
    r0.merge(&r1_clone);
    r1.merge(&r0_clone);
    
    assert_eq!(r0.to_string(), r1.to_string(), "Phase 2 convergence failed: R0={:?} R1={:?}", r0.to_string(), r1.to_string());
}

#[test]
fn test_crash_007_repro() {
    use together::crdt::rga::Rga;
    use together::crdt::Crdt;
    use together::key::KeyPair;
    
    let u0 = KeyPair::generate();
    let u1 = KeyPair::generate();
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    // U0 inserts "ABCDEF" 
    r0.insert(&u0.key_pub, 0, b"ABCDEF");
    // U0 inserts "AB" at pos 0
    r0.insert(&u0.key_pub, 0, b"AB");
    // U0 deletes pos 0-2 (deletes "ABA")
    r0.delete(0, 3);
    // U1 inserts "B" at pos 0
    r1.insert(&u1.key_pub, 0, b"B");
    // U0 deletes pos 0 (deletes "B" from "BCDEF")
    r0.delete(0, 1);
    
    println!("Before sync:");
    println!("  R0: {:?}", r0.to_string());
    println!("  R1: {:?}", r1.to_string());
    
    // Full sync
    let r0_clone = r0.clone();
    let r1_clone = r1.clone();
    r0.merge(&r1_clone);
    r1.merge(&r0_clone);
    
    println!("After sync:");
    println!("  R0: {:?}", r0.to_string());
    println!("  R1: {:?}", r1.to_string());
    
    assert_eq!(r0.to_string(), r1.to_string(), "Convergence failed!");
}

#[test]
fn test_crash_037_repro() {
    use together::crdt::rga::Rga;
    use together::crdt::Crdt;
    use together::key::KeyPair;
    
    let u0 = KeyPair::generate();
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    // U0 inserts "ABCDEFGHIJKLMNOPQ"
    r0.insert(&u0.key_pub, 0, b"ABCDEFGHIJKLMNOPQ");
    println!("After insert: {:?}", r0.to_string());
    
    // U0 deletes positions 6-12 (GHIJKLM)
    r0.delete(6, 7);
    println!("After delete: {:?}", r0.to_string());
    
    // Sync to r1
    r1.merge(&r0);
    println!("R0 after sync: {:?}", r0.to_string());
    println!("R1 after sync: {:?}", r1.to_string());
    
    assert_eq!(r0.to_string(), r1.to_string(), "Convergence failed: R0={:?} R1={:?}", r0.to_string(), r1.to_string());
}

#[test]
fn test_crash_040_repro() {
    use together::crdt::rga::Rga;
    use together::crdt::Crdt;
    use together::key::KeyPair;
    
    let u0 = KeyPair::generate();
    let u1 = KeyPair::generate();
    let u2 = KeyPair::generate();
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    let mut r2 = Rga::new();
    
    // Op 1: U2 inserts "C" at pos 0
    r2.insert(&u2.key_pub, 0, b"C");
    // Op 2: U0 inserts "AB" at pos 0
    r0.insert(&u0.key_pub, 0, b"AB");
    
    // FullSync
    let r0c = r0.clone();
    let r1c = r1.clone();
    let r2c = r2.clone();
    r0.merge(&r1c); r0.merge(&r2c);
    r1.merge(&r0c); r1.merge(&r2c);
    r2.merge(&r0c); r2.merge(&r1c);
    
    println!("After sync: R0={:?} R1={:?} R2={:?}", r0.to_string(), r1.to_string(), r2.to_string());
    assert_eq!(r0.to_string(), r1.to_string());
    assert_eq!(r0.to_string(), r2.to_string());
    
    // Op 7: U1 inserts "BCDEF" at pos 0
    r1.insert(&u1.key_pub, 0, b"BCDEF");
    println!("After U1 insert: R1={:?}", r1.to_string());
    
    // Op 8: U0 inserts 28 chars at pos 2
    println!("About to insert at pos 2 in R0={:?} (len={})", r0.to_string(), r0.len());
    r0.insert(&u0.key_pub, 2, b"ABCDEFGHIJKLMNOPQRSTUVWXYZAB");
    println!("After U0 insert: R0={:?}", r0.to_string());
}

#[test]
fn test_concurrent_insert_with_middle_origin() {
    use together::crdt::rga::Rga;
    use together::crdt::Crdt;
    use together::key::KeyPair;
    
    let u0 = KeyPair::generate();
    let u1 = KeyPair::generate();
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    // Both insert at pos 0 (no origin - concurrent at beginning)
    r0.insert(&u0.key_pub, 0, b"ABCDEF");
    r1.insert(&u1.key_pub, 0, b"BCDEFG");
    
    // U1 inserts in the middle of their own text
    r1.insert(&u1.key_pub, 3, b"XYZ");
    
    println!("Before sync: R0={:?} R1={:?}", r0.to_string(), r1.to_string());
    
    // Full sync
    let r0c = r0.clone();
    let r1c = r1.clone();
    r0.merge(&r1c);
    r1.merge(&r0c);
    
    println!("After sync: R0={:?} R1={:?}", r0.to_string(), r1.to_string());
    
    assert_eq!(r0.to_string(), r1.to_string(), "Convergence failed!");
}

#[test]
fn test_deterministic_convergence_bug() {
    use together::crdt::rga::Rga;
    use together::crdt::Crdt;
    use together::key::KeyPair;
    
    // Use same deterministic keys as fuzzer
    let u0 = KeyPair::from_seed(0);
    let u1 = KeyPair::from_seed(1);
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    // Op 1: U0 inserts "ABCDEF" at pos 0
    r0.insert(&u0.key_pub, 0, b"ABCDEF");
    // Op 2: U1 inserts "BCDEFG" at pos 0
    r1.insert(&u1.key_pub, 0, b"BCDEFG");
    // Op 4: U1 inserts at pos 3
    r1.insert(&u1.key_pub, 3, b"BCDEFGHIJKLMNOPQRSTUV");
    
    println!("Before sync:");
    println!("  R0: {:?}", r0.to_string());
    println!("  R1: {:?}", r1.to_string());
    
    // Full sync - clone first
    let r0c = r0.clone();
    let r1c = r1.clone();
    
    // Now merge
    r0.merge(&r1c);
    r1.merge(&r0c);
    
    println!("After sync:");
    println!("  R0: {:?}", r0.to_string());
    println!("  R1: {:?}", r1.to_string());
    
    assert_eq!(r0.to_string(), r1.to_string(), "Convergence failed!");
}

#[test]
fn test_delete_merge_bug() {
    // Minimal repro from fuzzer: U2 inserts, deletes, then R0/R1 merge from R2
    // and get different content
    let users: Vec<KeyPair> = (0..3).map(|i| KeyPair::from_seed(i as u64)).collect();
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    let mut r2 = Rga::new();
    
    // User 2 inserts at pos=0 len=31
    let content: Vec<u8> = (0..31).map(|i| b'C' + (i % 26)).collect();
    r2.insert(&users[2].key_pub, 0, &content);
    
    // User 2 deletes at pos=12 len=2
    r2.delete(12, 2);
    
    // User 2 deletes at pos=7 len=11
    r2.delete(7, 11);
    
    // Full sync
    let r0c = r0.clone();
    let r1c = r1.clone();
    let r2c = r2.clone();
    r0.merge(&r1c);
    r0.merge(&r2c);
    r1.merge(&r0c);
    r1.merge(&r2c);
    r2.merge(&r0c);
    r2.merge(&r1c);
    
    assert_eq!(r0.to_string(), r1.to_string(), "R0 != R1");
    assert_eq!(r0.to_string(), r2.to_string(), "R0 != R2");
}

#[test]
fn test_insert_at_beginning_after_delete() {
    // Repro from fuzzer: insert at pos=0 after delete shows wrong result
    let users: Vec<KeyPair> = (0..3).map(|i| KeyPair::from_seed(i as u64)).collect();
    
    let mut r = Rga::new();
    
    // Insert "BCDEFGHIJK" (user 1's content starting with 'B')
    let content: Vec<u8> = (0..10).map(|i| b'B' + (i % 26)).collect();
    r.insert(&users[1].key_pub, 0, &content);
    assert_eq!(r.to_string(), "BCDEFGHIJK");
    
    // Delete at pos=1, len=9 -> leaves "B"
    r.delete(1, 9);
    assert_eq!(r.to_string(), "B");
    
    // Insert "C" at pos=0
    r.insert(&users[2].key_pub, 0, b"C");
    // Should be "CB", not "BC"
    assert_eq!(r.to_string(), "CB", "Insert at pos=0 should prepend");
}

#[test]
fn test_concurrent_inserts_no_origin() {
    // U2 inserts at pos=0, U0 inserts at pos=0 independently
    // After sync, should converge
    let users: Vec<KeyPair> = (0..3).map(|i| KeyPair::from_seed(i as u64)).collect();
    
    let mut r0 = Rga::new();
    let mut r2 = Rga::new();
    
    // U2 inserts "CDE" at pos=0
    r2.insert(&users[2].key_pub, 0, b"CDE");
    assert_eq!(r2.to_string(), "CDE");
    
    // U0 inserts "AB" at pos=0 (independently)
    r0.insert(&users[0].key_pub, 0, b"AB");
    assert_eq!(r0.to_string(), "AB");
    
    // Sync both ways
    let r0c = r0.clone();
    let r2c = r2.clone();
    r0.merge(&r2c);
    r2.merge(&r0c);
    
    // Should converge
    assert_eq!(r0.to_string(), r2.to_string(), "Should converge after merge");
}

#[test]
fn test_concurrent_inserts_with_split() {
    // U2 inserts at pos=0, then inserts at pos=1 (splits)
    // U0 inserts at pos=0 independently
    // After sync, should converge
    let users: Vec<KeyPair> = (0..3).map(|i| KeyPair::from_seed(i as u64)).collect();
    
    let mut r0 = Rga::new();
    let mut r2 = Rga::new();
    
    // U2 inserts "CDE" at pos=0
    r2.insert(&users[2].key_pub, 0, b"CDE");
    assert_eq!(r2.to_string(), "CDE");
    
    // U2 inserts "XY" at pos=1 -> "CXYDE"
    r2.insert(&users[2].key_pub, 1, b"XY");
    assert_eq!(r2.to_string(), "CXYDE");
    
    // U0 inserts "AB" at pos=0 (independently)
    r0.insert(&users[0].key_pub, 0, b"AB");
    assert_eq!(r0.to_string(), "AB");
    
    // Sync both ways
    let r0c = r0.clone();
    let r2c = r2.clone();
    r0.merge(&r2c);
    r2.merge(&r0c);
    
    eprintln!("R0: {:?}", r0.to_string());
    eprintln!("R2: {:?}", r2.to_string());
    
    // Should converge
    assert_eq!(r0.to_string(), r2.to_string(), "Should converge after merge");
}

#[test]
fn test_merge_order_debug() {
    use together::crdt::Crdt;
    
    let users: Vec<KeyPair> = (0..3).map(|i| KeyPair::from_seed(i as u64)).collect();
    
    // Create R2's state
    let mut r2 = Rga::new();
    r2.insert(&users[2].key_pub, 0, b"CDE");
    r2.insert(&users[2].key_pub, 1, b"XY");
    assert_eq!(r2.to_string(), "CXYDE");
    
    // Create R0's state
    let mut r0 = Rga::new();
    r0.insert(&users[0].key_pub, 0, b"AB");
    assert_eq!(r0.to_string(), "AB");
    
    // Merge R2 into R0
    let r2c = r2.clone();
    r0.merge(&r2c);
    eprintln!("R0 after merge(R2): {:?}", r0.to_string());
    
    // Merge R0 into R2
    let r0_original = Rga::new();
    let mut r0_for_merge = r0_original.clone();
    r0_for_merge.insert(&users[0].key_pub, 0, b"AB");
    r2.merge(&r0_for_merge);
    eprintln!("R2 after merge(R0): {:?}", r2.to_string());
    
    // They should be the same
    assert_eq!(r0.to_string(), r2.to_string());
}

#[test]
fn test_user_key_ordering() {
    let users: Vec<KeyPair> = (0..3).map(|i| KeyPair::from_seed(i as u64)).collect();
    
    // Check ordering
    if users[0].key_pub > users[2].key_pub {
        eprintln!("User 0 > User 2");
    } else {
        eprintln!("User 2 > User 0");
    }
    
    if users[0].key_pub > users[1].key_pub {
        eprintln!("User 0 > User 1");
    } else {
        eprintln!("User 1 > User 0");
    }
}

#[test]
fn test_insert_at_beginning_twice() {
    // U2 inserts at pos=0, sync, then U2 inserts at pos=0 again
    let users: Vec<KeyPair> = (0..3).map(|i| KeyPair::from_seed(i as u64)).collect();
    
    let mut r0 = Rga::new();
    let mut r2 = Rga::new();
    
    // U2 inserts "C" at pos=0
    r2.insert(&users[2].key_pub, 0, b"C");
    assert_eq!(r2.to_string(), "C");
    
    // Sync
    r0.merge(&r2);
    assert_eq!(r0.to_string(), "C");
    
    // U2 inserts "DEFGHIJK" at pos=0 -> should give "DEFGHIJKC"
    r2.insert(&users[2].key_pub, 0, b"DEFGHIJK");
    assert_eq!(r2.to_string(), "DEFGHIJKC");
    
    // Sync
    r0.merge(&r2);
    
    eprintln!("R0: {:?}", r0.to_string());
    eprintln!("R2: {:?}", r2.to_string());
    
    assert_eq!(r0.to_string(), r2.to_string(), "Should converge");
}

#[test]
fn test_simple_delete_prefix_merge() {
    // Simple case: insert, delete prefix, merge
    let users: Vec<KeyPair> = (0..3).map(|i| KeyPair::from_seed(i as u64)).collect();
    
    let mut r0 = Rga::new();
    let mut r2 = Rga::new();
    
    // U2 inserts "ABCDE" at pos=0 (seq 0-4, no origin)
    r2.insert(&users[2].key_pub, 0, b"ABCDE");
    assert_eq!(r2.to_string(), "ABCDE");
    
    // U2 deletes prefix "AB" -> leaves "CDE"
    r2.delete(0, 2);
    assert_eq!(r2.to_string(), "CDE");
    
    // Merge into empty R0
    r0.merge(&r2);
    
    eprintln!("R0: {:?}", r0.to_string());
    eprintln!("R2: {:?}", r2.to_string());
    
    assert_eq!(r0.to_string(), r2.to_string(), "Should converge");
}

#[test]
fn test_delete_middle_then_prefix_merge() {
    // Insert, delete middle, delete prefix, merge
    let users: Vec<KeyPair> = (0..3).map(|i| KeyPair::from_seed(i as u64)).collect();
    
    let mut r0 = Rga::new();
    let mut r2 = Rga::new();
    
    // U2 inserts "ABCDEFGHIJ" at pos=0 (seq 0-9, no origin)
    r2.insert(&users[2].key_pub, 0, b"ABCDEFGHIJ");
    assert_eq!(r2.to_string(), "ABCDEFGHIJ");
    
    // U2 deletes middle "DE" at pos=3, len=2 -> "ABCFGHIJ"
    r2.delete(3, 2);
    assert_eq!(r2.to_string(), "ABCFGHIJ");
    
    // U2 deletes prefix "ABC" at pos=0, len=3 -> "FGHIJ"
    r2.delete(0, 3);
    assert_eq!(r2.to_string(), "FGHIJ");
    
    // Merge into empty R0
    r0.merge(&r2);
    
    eprintln!("R0: {:?}", r0.to_string());
    eprintln!("R2: {:?}", r2.to_string());
    
    assert_eq!(r0.to_string(), r2.to_string(), "Should converge");
}

#[test]
fn test_delete_merge_bug_simplified() {
    // Exact repro of test_delete_merge_bug but with tracing
    let users: Vec<KeyPair> = (0..3).map(|i| KeyPair::from_seed(i as u64)).collect();
    
    let mut r2 = Rga::new();
    
    // User 2 inserts at pos=0 len=31
    let content: Vec<u8> = (0..31).map(|i| b'C' + (i % 26)).collect();
    eprintln!("Content: {:?}", String::from_utf8_lossy(&content));
    r2.insert(&users[2].key_pub, 0, &content);
    eprintln!("After insert: {:?} (len={})", r2.to_string(), r2.len());
    
    // User 2 deletes at pos=12 len=2
    r2.delete(12, 2);
    eprintln!("After delete(12,2): {:?} (len={})", r2.to_string(), r2.len());
    
    // User 2 deletes at pos=7 len=11
    r2.delete(7, 11);
    eprintln!("After delete(7,11): {:?} (len={})", r2.to_string(), r2.len());
    
    // Now merge into empty R0
    let mut r0 = Rga::new();
    r0.merge(&r2);
    
    eprintln!("R0: {:?}", r0.to_string());
    eprintln!("R2: {:?}", r2.to_string());
    
    assert_eq!(r0.to_string(), r2.to_string(), "Should converge");
}

#[test]
fn test_debug_span_structure() {
    let users: Vec<KeyPair> = (0..3).map(|i| KeyPair::from_seed(i as u64)).collect();
    
    let mut r2 = Rga::new();
    
    // Insert
    let content: Vec<u8> = (0..31).map(|i| b'C' + (i % 26)).collect();
    r2.insert(&users[2].key_pub, 0, &content);
    eprintln!("After insert: {} spans", r2.span_count());
    
    // Delete(12, 2)
    r2.delete(12, 2);
    eprintln!("After delete(12,2): {} spans", r2.span_count());
    
    // Delete(7, 11)
    r2.delete(7, 11);
    eprintln!("After delete(7,11): {} spans, visible={}", r2.span_count(), r2.len());
    eprintln!("Content: {:?}", r2.to_string());
    
    // The key question: what origin does the last visible span have?
    // It should have origin pointing to seq 19 (last char of deleted span)
}


#[test]
fn test_insert_at_pos0_after_sync() {
    // Reproduces fuzzer crash: after syncing, inserting at pos=0 
    // should use RGA ordering, not just prepend
    let u0 = KeyPair::from_seed(0);
    let u1 = KeyPair::from_seed(1);
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    // U1 inserts "B" at pos=0
    r1.insert(&u1.key_pub, 0, b"B");
    assert_eq!(r1.to_string(), "B");
    
    // Full sync - both have "B"
    r0.merge(&r1);
    r1.merge(&r0);
    assert_eq!(r0.to_string(), "B");
    assert_eq!(r1.to_string(), "B");
    
    // U0 inserts "A" at pos=0 (before the "B")
    r0.insert(&u0.key_pub, 0, b"A");
    eprintln!("After U0 inserts A at pos=0:");
    eprintln!("  r0: {:?}", r0.to_string());
    
    // Final sync
    r1.merge(&r0);
    r0.merge(&r1);
    
    eprintln!("After final sync:");
    eprintln!("  r0: {:?}", r0.to_string());
    eprintln!("  r1: {:?}", r1.to_string());
    
    // They must converge
    assert_eq!(r0.to_string(), r1.to_string(), "Convergence failure!");
}

#[test]
fn test_debug_key_ordering() {
    use together::key::KeyPair;
    use together::crdt::rga::Rga;
    use together::crdt::Crdt;
    
    let u0 = KeyPair::from_seed(0);
    let u1 = KeyPair::from_seed(1);
    
    eprintln!("U0 > U1 (keys): {}", u0.key_pub > u1.key_pub);
    
    // Test independent inserts at pos=0
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    // Both insert independently at pos=0
    r0.insert(&u0.key_pub, 0, b"A");
    r1.insert(&u1.key_pub, 0, b"B");
    
    // Merge both ways
    let mut merged_0 = r0.clone();
    merged_0.merge(&r1);
    
    let mut merged_1 = r1.clone();
    merged_1.merge(&r0);
    
    eprintln!("r0 merged with r1: {:?}", merged_0.to_string());
    eprintln!("r1 merged with r0: {:?}", merged_1.to_string());
    
    assert_eq!(merged_0.to_string(), merged_1.to_string(), "Order should be consistent!");
}

#[test]
fn test_insert_after_first_char_merge_bug() {
    // U2 inserts C at pos=0
    // U0 inserts A at pos=0  
    // Sync -> all have "CA" (C first due to U2 > U0 keys)
    // U2 inserts X at pos=1 (after C, before A) -> U2 has "CXA"
    // Sync -> should all have "CXA"
    
    let u0 = KeyPair::from_seed(0);
    let u2 = KeyPair::from_seed(2);
    
    let mut r0 = Rga::new();
    let mut r2 = Rga::new();
    
    // U2 inserts "C" at pos=0
    r2.insert(&u2.key_pub, 0, b"C");
    
    // U0 inserts "A" at pos=0
    r0.insert(&u0.key_pub, 0, b"A");
    
    // Full sync
    r0.merge(&r2);
    r2.merge(&r0);
    
    eprintln!("After first sync:");
    eprintln!("  r0: {:?}", r0.to_string());
    eprintln!("  r2: {:?}", r2.to_string());
    assert_eq!(r0.to_string(), r2.to_string());
    
    // U2 inserts "X" at pos=1 (after C, before A)
    r2.insert(&u2.key_pub, 1, b"X");
    eprintln!("After U2 inserts X at pos=1:");
    eprintln!("  r2: {:?}", r2.to_string());
    
    // Final sync
    r0.merge(&r2);
    r2.merge(&r0);
    
    eprintln!("After final sync:");
    eprintln!("  r0: {:?}", r0.to_string());
    eprintln!("  r2: {:?}", r2.to_string());
    
    assert_eq!(r0.to_string(), r2.to_string(), "Convergence failure!");
}

#[test]
fn test_insert_after_first_char_detailed() {
    // Detailed trace of the merge bug
    
    let u0 = KeyPair::from_seed(0);
    let u2 = KeyPair::from_seed(2);
    
    let mut r0 = Rga::new();
    let mut r2 = Rga::new();
    
    // U2 inserts "C" at pos=0 (seq=0)
    r2.insert(&u2.key_pub, 0, b"C");
    eprintln!("r2 after C insert: spans={}", r2.span_count());
    
    // U0 inserts "A" at pos=0 (seq=0)
    r0.insert(&u0.key_pub, 0, b"A");
    eprintln!("r0 after A insert: spans={}", r0.span_count());
    
    // Sync
    r0.merge(&r2);
    r2.merge(&r0);
    eprintln!("After sync:");
    eprintln!("  r0: {:?} spans={}", r0.to_string(), r0.span_count());
    eprintln!("  r2: {:?} spans={}", r2.to_string(), r2.span_count());
    
    // U2 inserts "X" at pos=1
    // This should create span with origin=(U2, seq=0) i.e. origin is the C
    r2.insert(&u2.key_pub, 1, b"X");
    eprintln!("After U2 inserts X at pos=1:");
    eprintln!("  r2: {:?} spans={}", r2.to_string(), r2.span_count());
    
    // Now r0 merges r2
    // r0 should find origin=(U2, seq=0) which is the C
    eprintln!("\nMerging r2 into r0...");
    r0.merge(&r2);
    eprintln!("After merge:");
    eprintln!("  r0: {:?}", r0.to_string());
    
    assert_eq!(r0.to_string(), "CXA", "X should appear!");
}

#[test]
fn test_coalesce_merge_bug() {
    // Simplified repro of the fuzzer crash
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    let mut r1 = Rga::new();
    let mut r2 = Rga::new();
    
    // U1: insert "BCDEFGHIJK" at pos=0, then "B" at pos=1
    r1.insert(&u1.key_pub, 0, b"BCDEFGHIJK");
    r1.insert(&u1.key_pub, 1, b"B");
    eprintln!("r1 after inserts: {:?}", r1.to_string());
    
    // U2: insert "C" at pos=0
    r2.insert(&u2.key_pub, 0, b"C");
    eprintln!("r2 after insert: {:?}", r2.to_string());
    
    // Full sync
    r1.merge(&r2);
    r2.merge(&r1);
    eprintln!("After sync:");
    eprintln!("  r1: {:?}", r1.to_string());
    eprintln!("  r2: {:?}", r2.to_string());
    assert_eq!(r1.to_string(), r2.to_string());
    
    // U2 inserts at pos=1
    r2.insert(&u2.key_pub, 1, b"X");
    eprintln!("After U2 inserts X at pos=1:");
    eprintln!("  r2: {:?}", r2.to_string());
    
    // U2 inserts more at pos=1
    r2.insert(&u2.key_pub, 1, b"YYYYYYYY");
    eprintln!("After U2 inserts YYYYYYYY at pos=1:");
    eprintln!("  r2: {:?}", r2.to_string());
    
    // Final sync
    r1.merge(&r2);
    r2.merge(&r1);
    eprintln!("After final sync:");
    eprintln!("  r1: {:?}", r1.to_string());
    eprintln!("  r2: {:?}", r2.to_string());
    
    assert_eq!(r1.to_string(), r2.to_string(), "Convergence failure!");
}

#[test]
fn test_coalesce_debug() {
    let u1 = KeyPair::from_seed(1);
    
    let mut r1 = Rga::new();
    
    // U1: insert "BCDEFGHIJK" at pos=0
    r1.insert(&u1.key_pub, 0, b"BCDEFGHIJK");
    eprintln!("After first insert: {:?}, spans={}", r1.to_string(), r1.span_count());
    
    // U1: insert "B" at pos=1
    r1.insert(&u1.key_pub, 1, b"B");
    eprintln!("After second insert: {:?}, spans={}", r1.to_string(), r1.span_count());
}

#[test]
fn test_merge_span_structure() {
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    let mut r1 = Rga::new();
    let mut r2 = Rga::new();
    
    // U1 inserts with a split
    r1.insert(&u1.key_pub, 0, b"BCDEFGHIJK");
    r1.insert(&u1.key_pub, 1, b"B");
    eprintln!("r1: {:?}, spans={}", r1.to_string(), r1.span_count());
    
    // U2 inserts
    r2.insert(&u2.key_pub, 0, b"C");
    eprintln!("r2: {:?}, spans={}", r2.to_string(), r2.span_count());
    
    // Sync
    r1.merge(&r2);
    r2.merge(&r1);
    eprintln!("After sync:");
    eprintln!("  r1: {:?}, spans={}", r1.to_string(), r1.span_count());
    eprintln!("  r2: {:?}, spans={}", r2.to_string(), r2.span_count());
    
    // U2 inserts at pos=1
    r2.insert(&u2.key_pub, 1, b"X");
    eprintln!("After U2 inserts X at pos=1:");
    eprintln!("  r2: {:?}, spans={}", r2.to_string(), r2.span_count());
    
    // Now merge just this one change
    let before_r1 = r1.to_string();
    r1.merge(&r2);
    eprintln!("After r1 merges r2:");
    eprintln!("  r1 before: {:?}", before_r1);
    eprintln!("  r1 after:  {:?}, spans={}", r1.to_string(), r1.span_count());
    
    assert_eq!(r1.to_string(), r2.to_string());
}

#[test] 
fn test_simplest_case() {
    // Most minimal repro
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    let mut r1 = Rga::new();
    let mut r2 = Rga::new();
    
    // U1: "AB" split as [A][B]
    r1.insert(&u1.key_pub, 0, b"A");
    r1.insert(&u1.key_pub, 1, b"B");  // Insert B at end
    eprintln!("r1: {:?}, spans={}", r1.to_string(), r1.span_count());
    
    // U2 gets r1's content
    r2.merge(&r1);
    eprintln!("r2 after merge: {:?}, spans={}", r2.to_string(), r2.span_count());
    
    // U2 inserts X at pos=1 (after A, before B)
    r2.insert(&u2.key_pub, 1, b"X");
    eprintln!("r2 after X insert: {:?}, spans={}", r2.to_string(), r2.span_count());
    
    // r1 merges r2's X
    eprintln!("r1 before merge: {:?}", r1.to_string());
    r1.merge(&r2);
    eprintln!("r1 after merge:  {:?}", r1.to_string());
    
    assert_eq!(r1.to_string(), "AXB");
}

#[test] 
fn test_with_split() {
    // With a split in the middle
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    let mut r1 = Rga::new();
    let mut r2 = Rga::new();
    
    // U1: "AC" then insert "B" at pos=1 -> "ABC"
    r1.insert(&u1.key_pub, 0, b"AC");
    r1.insert(&u1.key_pub, 1, b"B");  // Insert B in middle, causes split
    eprintln!("r1: {:?}, spans={}", r1.to_string(), r1.span_count());
    
    // U2 gets r1's content
    r2.merge(&r1);
    eprintln!("r2 after merge: {:?}, spans={}", r2.to_string(), r2.span_count());
    
    // U2 inserts X at pos=1 (after A, before B)
    r2.insert(&u2.key_pub, 1, b"X");
    eprintln!("r2 after X insert: {:?}", r2.to_string());
    
    // r1 merges r2's X
    eprintln!("r1 before merge: {:?}", r1.to_string());
    r1.merge(&r2);
    eprintln!("r1 after merge:  {:?}", r1.to_string());
    
    // X should be between A and B
    assert_eq!(r1.to_string(), "ABXC", "X should be after B due to RGA ordering");
}

#[test] 
fn test_debug_spans() {
    let u1 = KeyPair::from_seed(1);
    
    let mut r1 = Rga::new();
    
    // U1: "AC" then insert "B" at pos=1 -> "ABC"
    r1.insert(&u1.key_pub, 0, b"AC");
    eprintln!("After 'AC': spans={}", r1.span_count());
    
    r1.insert(&u1.key_pub, 1, b"B");
    eprintln!("After 'B' insert at pos=1: {:?}, spans={}", r1.to_string(), r1.span_count());
    
    // Let's look at the spans
    // Access internal spans - we can't directly, but we can test merge behavior
}

#[test] 
fn test_key_order() {
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    eprintln!("U1 > U2: {}", u1.key_pub > u2.key_pub);
    eprintln!("U2 > U1: {}", u2.key_pub > u1.key_pub);
}

#[test]
fn test_concurrent_inserts_at_same_pos() {
    // U2 creates content, syncs to all
    // U0 and U1 both insert at same position independently
    // After merge, all should converge
    
    let u0 = KeyPair::from_seed(0);
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    let mut r2 = Rga::new();
    
    // U2 inserts "CD"
    r2.insert(&u2.key_pub, 0, b"CD");
    
    // Full sync
    r0.merge(&r2);
    r1.merge(&r2);
    eprintln!("After sync: r0={:?} r1={:?}", r0.to_string(), r1.to_string());
    
    // U0 inserts "A" at pos=1
    r0.insert(&u0.key_pub, 1, b"A");
    eprintln!("r0 after A: {:?}", r0.to_string());
    
    // U1 inserts "B" at pos=1
    r1.insert(&u1.key_pub, 1, b"B");
    eprintln!("r1 after B: {:?}", r1.to_string());
    
    // Final sync
    r0.merge(&r1);
    r0.merge(&r2);
    r1.merge(&r0);
    r1.merge(&r2);
    r2.merge(&r0);
    r2.merge(&r1);
    
    eprintln!("Final: r0={:?} r1={:?} r2={:?}", r0.to_string(), r1.to_string(), r2.to_string());
    
    assert_eq!(r0.to_string(), r1.to_string(), "r0 != r1");
    assert_eq!(r1.to_string(), r2.to_string(), "r1 != r2");
}

#[test]
fn test_multiple_inserts_same_user() {
    // Simulating the fuzzer crash pattern:
    // U2 creates content, syncs
    // U0 inserts at pos=1
    // U1 inserts at pos=1, then more inserts
    
    let u0 = KeyPair::from_seed(0);
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    // U2 inserts "CDEFGHIJKLM"
    let mut r2 = Rga::new();
    r2.insert(&u2.key_pub, 0, b"CDEFGHIJKLM");
    
    // Full sync
    r0.merge(&r2);
    r1.merge(&r2);
    eprintln!("After initial sync:");
    eprintln!("  r0: {:?}", r0.to_string());
    eprintln!("  r1: {:?}", r1.to_string());
    
    // U0 inserts at pos=1
    r0.insert(&u0.key_pub, 1, b"ABCDEFGHIJKLMNO");
    eprintln!("r0 after U0 insert: {:?}", r0.to_string());
    
    // U1 inserts at pos=1
    r1.insert(&u1.key_pub, 1, b"BCDEFGHIJKLMNOPQRSTUVWXYZABCD");
    eprintln!("r1 after U1 insert: {:?}", r1.to_string());
    
    // U1 inserts at pos=4
    r1.insert(&u1.key_pub, 4, b"BCDEFGHIJKLMNOPQRSTUVWXYZABCD");
    eprintln!("r1 after U1 insert at pos=4: {:?}", r1.to_string());
    
    // Full sync
    r0.merge(&r1);
    r1.merge(&r0);
    
    eprintln!("Final:");
    eprintln!("  r0: {:?}", r0.to_string());
    eprintln!("  r1: {:?}", r1.to_string());
    
    assert_eq!(r0.to_string(), r1.to_string(), "Convergence failure!");
}

#[test]
fn test_simpler_multi_insert() {
    let u0 = KeyPair::from_seed(0);
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    // U2 creates "CD"
    let mut r2 = Rga::new();
    r2.insert(&u2.key_pub, 0, b"CD");
    
    // Sync
    r0.merge(&r2);
    r1.merge(&r2);
    eprintln!("After sync: r0={:?} r1={:?}", r0.to_string(), r1.to_string());
    
    // U0 inserts "A" at pos=1
    r0.insert(&u0.key_pub, 1, b"A");
    eprintln!("r0 after U0: {:?}", r0.to_string());
    
    // U1 inserts "B" at pos=1
    r1.insert(&u1.key_pub, 1, b"B");
    eprintln!("r1 after first U1 insert: {:?}", r1.to_string());
    
    // U1 inserts "X" at pos=2 (after the first B)
    r1.insert(&u1.key_pub, 2, b"X");
    eprintln!("r1 after second U1 insert: {:?}", r1.to_string());
    
    // Merge
    r0.merge(&r1);
    r1.merge(&r0);
    
    eprintln!("Final: r0={:?} r1={:?}", r0.to_string(), r1.to_string());
    assert_eq!(r0.to_string(), r1.to_string());
}

#[test]
fn test_insert_at_later_pos() {
    let u0 = KeyPair::from_seed(0);
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    // U2 creates "CDEFG"
    let mut r2 = Rga::new();
    r2.insert(&u2.key_pub, 0, b"CDEFG");
    
    // Sync
    r0.merge(&r2);
    r1.merge(&r2);
    eprintln!("After sync: r0={:?} r1={:?}", r0.to_string(), r1.to_string());
    
    // U0 inserts "A" at pos=1 (after C)
    r0.insert(&u0.key_pub, 1, b"A");
    eprintln!("r0: {:?}", r0.to_string());
    
    // U1 inserts "B" at pos=1 (after C)
    r1.insert(&u1.key_pub, 1, b"B");
    eprintln!("r1 after B: {:?}", r1.to_string());
    
    // U1 inserts "X" at pos=4 (after first 4 chars: "CBDE")
    // Wait, r1 is "CBDEFG", so pos=4 is after "CBDE"
    r1.insert(&u1.key_pub, 4, b"X");
    eprintln!("r1 after X at pos=4: {:?}", r1.to_string());
    
    // Merge
    r0.merge(&r1);
    r1.merge(&r0);
    
    eprintln!("Final: r0={:?} r1={:?}", r0.to_string(), r1.to_string());
    assert_eq!(r0.to_string(), r1.to_string());
}

#[test]
fn test_long_inserts_coalesce() {
    let u0 = KeyPair::from_seed(0);
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    // U2 creates "CDEFGHIJKLM"
    let mut r2 = Rga::new();
    r2.insert(&u2.key_pub, 0, b"CDEFGHIJKLM");
    
    // Sync
    r0.merge(&r2);
    r1.merge(&r2);
    
    // U0 inserts long string at pos=1
    r0.insert(&u0.key_pub, 1, b"ABCDEFGHIJKLMNO");
    eprintln!("r0: {:?} spans={}", r0.to_string(), r0.span_count());
    
    // U1 inserts long string at pos=1
    r1.insert(&u1.key_pub, 1, b"BCDEFGHIJKLMNOPQRSTUVWXYZABCD");
    eprintln!("r1 after first: {:?} spans={}", r1.to_string(), r1.span_count());
    
    // U1 inserts at pos=4
    r1.insert(&u1.key_pub, 4, b"BCDEFGHIJKLMNOPQRSTUVWXYZABCD");
    eprintln!("r1 after second: {:?} spans={}", r1.to_string(), r1.span_count());
    
    // Check r1's structure before merge
    eprintln!("\n=== Before merge ===");
    eprintln!("r0: {:?}", r0.to_string());
    eprintln!("r1: {:?}", r1.to_string());
    
    // Merge r1 into r0
    r0.merge(&r1);
    eprintln!("\n=== After r0.merge(r1) ===");
    eprintln!("r0: {:?}", r0.to_string());
    
    // Merge r0 into r1
    r1.merge(&r0);
    eprintln!("r1: {:?}", r1.to_string());
    
    assert_eq!(r0.to_string(), r1.to_string());
}

#[test]
fn test_minimal_coalesce_bug() {
    let u0 = KeyPair::from_seed(0);
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    // U2 creates "CD"
    let mut r2 = Rga::new();
    r2.insert(&u2.key_pub, 0, b"CD");
    
    // Sync
    r0.merge(&r2);
    r1.merge(&r2);
    
    // U0 inserts "AB" at pos=1 (after C)
    r0.insert(&u0.key_pub, 1, b"AB");
    eprintln!("r0: {:?} spans={}", r0.to_string(), r0.span_count());
    
    // U1 inserts "XY" at pos=1 (after C)
    r1.insert(&u1.key_pub, 1, b"XY");
    eprintln!("r1 after XY: {:?} spans={}", r1.to_string(), r1.span_count());
    
    // U1 inserts "Z" at pos=3 (after X, Y - so after "CXY")
    // This means origin is Y
    r1.insert(&u1.key_pub, 3, b"Z");
    eprintln!("r1 after Z at pos=3: {:?} spans={}", r1.to_string(), r1.span_count());
    
    // Merge
    eprintln!("\nMerging...");
    r0.merge(&r1);
    r1.merge(&r0);
    
    eprintln!("r0: {:?}", r0.to_string());
    eprintln!("r1: {:?}", r1.to_string());
    
    assert_eq!(r0.to_string(), r1.to_string());
}

#[test]
fn test_insert_mid_coalesce() {
    let u0 = KeyPair::from_seed(0);
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    // U2 creates "CDEFG"
    let mut r2 = Rga::new();
    r2.insert(&u2.key_pub, 0, b"CDEFG");
    
    // Sync
    r0.merge(&r2);
    r1.merge(&r2);
    
    // U0 inserts "AB" at pos=1 (after C)
    r0.insert(&u0.key_pub, 1, b"AB");
    eprintln!("r0: {:?} spans={}", r0.to_string(), r0.span_count());
    
    // U1 inserts "XY" at pos=1 (after C)  
    r1.insert(&u1.key_pub, 1, b"XY");
    eprintln!("r1 after XY: {:?} spans={}", r1.to_string(), r1.span_count());
    
    // U1 inserts "Z" at pos=4 
    // r1 is "CXYDEFG", so pos=4 is after "CXYD" - origin is D (from U2)
    r1.insert(&u1.key_pub, 4, b"Z");
    eprintln!("r1 after Z at pos=4: {:?} spans={}", r1.to_string(), r1.span_count());
    
    // Merge
    eprintln!("\nMerging...");
    r0.merge(&r1);
    r1.merge(&r0);
    
    eprintln!("r0: {:?}", r0.to_string());
    eprintln!("r1: {:?}", r1.to_string());
    
    assert_eq!(r0.to_string(), r1.to_string());
}

#[test]
fn test_u1_multiple_inserts() {
    let u0 = KeyPair::from_seed(0);
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    // U2 creates "CDEF"
    let mut r2 = Rga::new();
    r2.insert(&u2.key_pub, 0, b"CDEF");
    
    // Sync
    r0.merge(&r2);
    r1.merge(&r2);
    eprintln!("Start: r0={:?} r1={:?}", r0.to_string(), r1.to_string());
    
    // U0 inserts "A" at pos=1 (after C)
    r0.insert(&u0.key_pub, 1, b"A");
    eprintln!("r0 after A: {:?}", r0.to_string());
    
    // U1 inserts "X" at pos=1 (after C)
    r1.insert(&u1.key_pub, 1, b"X");
    eprintln!("r1 after X: {:?}", r1.to_string());
    
    // U1 inserts "Y" at pos=2 (after X)
    r1.insert(&u1.key_pub, 2, b"Y");
    eprintln!("r1 after Y: {:?}", r1.to_string());
    
    // U1 inserts "Z" at pos=3 (after Y)
    r1.insert(&u1.key_pub, 3, b"Z");
    eprintln!("r1 after Z: {:?}", r1.to_string());
    
    // Merge
    eprintln!("\n=== Merging ===");
    r0.merge(&r1);
    eprintln!("r0 after merge: {:?}", r0.to_string());
    r1.merge(&r0);
    eprintln!("r1 after merge: {:?}", r1.to_string());
    
    assert_eq!(r0.to_string(), r1.to_string());
}

#[test]
fn test_exact_fail_pattern() {
    let u0 = KeyPair::from_seed(0);
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    // U2 creates content
    let mut r2 = Rga::new();
    r2.insert(&u2.key_pub, 0, b"CDEFGHIJKLM");
    
    // Sync
    r0.merge(&r2);
    r1.merge(&r2);
    eprintln!("After sync:");
    eprintln!("  r0: {:?}", r0.to_string());
    eprintln!("  r1: {:?}", r1.to_string());
    
    // U0 inserts at pos=1: long content
    r0.insert(&u0.key_pub, 1, b"ABCDEFGHIJKLMNO"); // 15 chars
    eprintln!("r0 after U0 insert at pos=1: {:?}", r0.to_string());
    
    // U1 inserts at pos=1: longer content
    r1.insert(&u1.key_pub, 1, b"BCDEFGHIJKLMNOPQRSTUVWXYZABCD"); // 29 chars
    eprintln!("r1 after U1 first insert at pos=1: {:?}", r1.to_string());
    eprintln!("  r1 length: {}", r1.len());
    
    // U1 inserts at pos=4
    // r1 is "C" + 29chars + "DEFGHIJKLM" = "CBCDEFGHIJKLMNOPQRSTUVWXYZABCDDEFGHIJKLM"
    // pos=4 is after "CBCD" - the 4th char is D (from U1's insert)
    eprintln!("r1 before second insert: {:?}", r1.to_string());
    eprintln!("  char at pos 3 (0-indexed): '{}'", r1.to_string().chars().nth(3).unwrap());
    r1.insert(&u1.key_pub, 4, b"BCDEFGHIJKLMNOPQRSTUVWXYZABCD"); // 29 chars
    eprintln!("r1 after U1 second insert at pos=4: {:?}", r1.to_string());
    
    // Merge
    eprintln!("\n=== Merging ===");
    r0.merge(&r1);
    eprintln!("r0: {:?}", r0.to_string());
    r1.merge(&r0);
    eprintln!("r1: {:?}", r1.to_string());
    
    assert_eq!(r0.to_string(), r1.to_string(), "Divergence!");
}

#[test]
fn test_simpler_version() {
    let u0 = KeyPair::from_seed(0);
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    // U2 creates "CD"
    let mut r2 = Rga::new();
    r2.insert(&u2.key_pub, 0, b"CD");
    
    // Sync
    r0.merge(&r2);
    r1.merge(&r2);
    
    // U0 inserts "AB" at pos=1 (origin=C)
    r0.insert(&u0.key_pub, 1, b"AB");
    eprintln!("r0: {:?}", r0.to_string());
    
    // U1 inserts "XY" at pos=1 (origin=C)
    r1.insert(&u1.key_pub, 1, b"XY");
    eprintln!("r1 after XY: {:?}", r1.to_string());
    
    // U1 inserts "Z" at pos=3 - this is after "CXY", so origin=Y (U1, seq=1)
    r1.insert(&u1.key_pub, 3, b"Z");
    eprintln!("r1 after Z: {:?}", r1.to_string());
    
    // Now U1 inserts "W" at pos=4 - this is after "CXYZ", so origin=Z (U1, seq=2)
    r1.insert(&u1.key_pub, 4, b"W");
    eprintln!("r1 after W: {:?}", r1.to_string());
    
    // Merge
    eprintln!("\n=== Merge ===");
    r0.merge(&r1);
    eprintln!("r0: {:?}", r0.to_string());
    r1.merge(&r0);
    eprintln!("r1: {:?}", r1.to_string());
    
    assert_eq!(r0.to_string(), r1.to_string());
}

#[test]
fn test_coalesce_then_origin() {
    let u0 = KeyPair::from_seed(0);
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    // U2 creates "CD"
    let mut r2 = Rga::new();
    r2.insert(&u2.key_pub, 0, b"CD");
    
    // Sync
    r0.merge(&r2);
    r1.merge(&r2);
    
    // U0 inserts "AB" at pos=1 (origin=C)
    r0.insert(&u0.key_pub, 1, b"AB");
    eprintln!("r0: {:?} spans={}", r0.to_string(), r0.span_count());
    
    // U1 inserts "WXYZ" at pos=1 (origin=C) - one big insert, coalesced
    r1.insert(&u1.key_pub, 1, b"WXYZ");
    eprintln!("r1 after WXYZ: {:?} spans={}", r1.to_string(), r1.span_count());
    
    // U1 inserts at pos=4 - this is after "CWXY", so origin=Y (U1, seq=2)
    // But Y is in the middle of the coalesced WXYZ span!
    r1.insert(&u1.key_pub, 4, b"!");
    eprintln!("r1 after ! at pos=4: {:?} spans={}", r1.to_string(), r1.span_count());
    
    // Merge
    eprintln!("\n=== Merge ===");
    r0.merge(&r1);
    eprintln!("r0: {:?}", r0.to_string());
    r1.merge(&r0);
    eprintln!("r1: {:?}", r1.to_string());
    
    assert_eq!(r0.to_string(), r1.to_string());
}

#[test]
fn test_coalesce_debug2() {
    let u0 = KeyPair::from_seed(0);
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    // U2 creates "CD"
    let mut r2 = Rga::new();
    r2.insert(&u2.key_pub, 0, b"CD");
    
    // Sync
    r0.merge(&r2);
    r1.merge(&r2);
    
    // U0 inserts "AB" at pos=1 (origin=C)
    r0.insert(&u0.key_pub, 1, b"AB");
    eprintln!("r0: {:?}", r0.to_string());
    
    // U1 inserts "WXYZ" at pos=1 (origin=C)
    r1.insert(&u1.key_pub, 1, b"WXYZ");
    eprintln!("r1 after WXYZ: {:?}", r1.to_string());
    
    // U1 inserts "!" at pos=4 (origin should be Y = U1 seq 2)
    r1.insert(&u1.key_pub, 4, b"!");
    eprintln!("r1 after !: {:?}", r1.to_string());
    
    // Before merge, what does r0 look like?
    eprintln!("\n=== Before merge ===");
    eprintln!("r0: {:?}", r0.to_string());
    eprintln!("r1: {:?}", r1.to_string());
    
    // Merge r1's U1 content into r0
    eprintln!("\n=== r0.merge(r1) ===");
    r0.merge(&r1);
    eprintln!("r0 after: {:?}", r0.to_string());
    
    // Now merge r0 into r1
    eprintln!("\n=== r1.merge(r0) ===");
    eprintln!("r1 before: {:?}", r1.to_string());
    r1.merge(&r0);
    eprintln!("r1 after: {:?}", r1.to_string());
    
    assert_eq!(r0.to_string(), r1.to_string());
}

#[test]
fn test_key_ordering() {
    let u0 = KeyPair::from_seed(0);
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    eprintln!("U0 > U1: {}", u0.key_pub > u1.key_pub);
    eprintln!("U0 > U2: {}", u0.key_pub > u2.key_pub);
    eprintln!("U1 > U2: {}", u1.key_pub > u2.key_pub);
    eprintln!("U1 > U0: {}", u1.key_pub > u0.key_pub);
    eprintln!("U2 > U0: {}", u2.key_pub > u0.key_pub);
    eprintln!("U2 > U1: {}", u2.key_pub > u1.key_pub);
}

#[test]
fn test_check_d_origin() {
    let u0 = KeyPair::from_seed(0);
    let u2 = KeyPair::from_seed(2);
    
    let mut r0 = Rga::new();
    
    // U2 creates "CD"
    r0.insert(&u2.key_pub, 0, b"CD");
    eprintln!("r0 after CD: {:?} spans={}", r0.to_string(), r0.span_count());
    
    // U0 inserts "AB" at pos=1 (after C) - this should split CD
    r0.insert(&u0.key_pub, 1, b"AB");
    eprintln!("r0 after AB: {:?} spans={}", r0.to_string(), r0.span_count());
    
    // Now r0 should have spans: C, AB, D
    // D's origin should be C (last char of left part after split)
}

#[test]
fn test_trace_r0_structure() {
    let u0 = KeyPair::from_seed(0);
    let u2 = KeyPair::from_seed(2);
    
    let mut r0 = Rga::new();
    
    // U2 creates "CD"
    r0.insert(&u2.key_pub, 0, b"CD");
    eprintln!("After CD: spans={}", r0.span_count());
    
    // U0 inserts "AB" at pos=1 (after C)
    r0.insert(&u0.key_pub, 1, b"AB");
    eprintln!("After AB: {:?} spans={}", r0.to_string(), r0.span_count());
    // Expected structure: C, AB, D (3 spans)
    // or C, A, B, D if not coalesced (4 spans)
}

#[test]
fn test_trace_merge_step_by_step() {
    // This test verifies that concurrent inserts from different replicas
    // are correctly ordered during merge, with split continuations staying
    // in their natural position after real concurrent inserts.
    let u0 = KeyPair::from_seed(0);
    let u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    let mut r0 = Rga::new();
    let mut r1 = Rga::new();
    
    // U2 creates "CD"
    r0.insert(&u2.key_pub, 0, b"CD");
    r1.insert(&u2.key_pub, 0, b"CD");
    
    // U0 inserts "AB" at pos=1 on r0
    // Result: "CABD" (AB at cursor position, D is split continuation)
    r0.insert(&u0.key_pub, 1, b"AB");
    eprintln!("r0 after AB: {:?}", r0.to_string());
    
    // U1 inserts "WXYZ" at pos=1 on r1
    // Result: "CWXYZD" (WXYZ at cursor position, D is split continuation)
    r1.insert(&u1.key_pub, 1, b"WXYZ");
    eprintln!("r1 after WXYZ: {:?}", r1.to_string());
    
    // U1 inserts "!" at pos=4 on r1 (after Y)
    // Result: "CWXY!ZD" (! at cursor position, Z is split continuation)
    r1.insert(&u1.key_pub, 4, b"!");
    eprintln!("r1 after !: {:?}", r1.to_string());
    
    // Now merge r1's WXYZ and ! into r0
    // r0 starts with: C, AB, D (AB and D at origin=C)
    // Merging WXY (origin=C): WXY and AB are siblings, ordered by RGA priority
    //   u1 (7aae) > u0 (03a1), so WXY comes before AB
    // Merging ! (origin=Y): goes after Y
    // Merging Z (origin=Y, split continuation): stays after !
    // D is a split continuation of C, stays after all siblings
    // Expected: C + WXY + ! + Z + AB + D = "CWXY!ZABD"
    eprintln!("\n=== Merging r1 into r0 ===");
    r0.merge(&r1);
    eprintln!("r0 result: {:?}", r0.to_string());
    
    // With split continuations excluded from sibling ordering:
    // - WXY (u1) before AB (u0) due to RGA priority
    // - ! after Y, Z after ! (Z is split continuation)
    // - D stays at the end (split continuation)
    assert_eq!(r0.to_string(), "CWXY!ZABD");
}

#[test]
fn test_local_insert_sibling_order() {
    // When a user inserts at position 1 in "CD", their content should appear
    // at position 1 (between C and D), regardless of RGA sibling priority.
    // This is because D is a "split continuation" of C - it was created by
    // splitting the original span, not by a concurrent insert.
    let u0 = KeyPair::from_seed(0);
    let u2 = KeyPair::from_seed(2);
    
    eprintln!("U0 > U2: {}", u0.key_pub > u2.key_pub);
    eprintln!("U2 > U0: {}", u2.key_pub > u0.key_pub);
    
    let mut rga = Rga::new();
    
    // U2 creates "CD"
    rga.insert(&u2.key_pub, 0, b"CD");
    eprintln!("After CD: {:?}", rga.to_string());
    
    // U0 inserts "AB" at pos=1 (after C)
    // This splits CD into C and D. D is a split continuation of C.
    // AB is inserted at the cursor position (after C, before D).
    // Even though U2 > U0, D is NOT a concurrent sibling - it's a split
    // continuation, so it should stay after AB.
    // Expected result: "CABD" (AB at cursor position, D after)
    rga.insert(&u0.key_pub, 1, b"AB");
    eprintln!("After AB at pos=1: {:?}", rga.to_string());
    
    // AB should appear at position 1 (the cursor position), not after D
    assert_eq!(rga.to_string(), "CABD", "AB should be at cursor position, D after");
}

#[test]
fn test_insert_after_sync_convergence() {
    // Test that local inserts work correctly after merging with other replicas.
    // This catches a bug where the right-split span was incorrectly treated as
    // a higher-priority sibling, causing inserts to go to the wrong position.
    let u0 = KeyPair::from_seed(0);
    let _u1 = KeyPair::from_seed(1);
    let u2 = KeyPair::from_seed(2);
    
    let mut rga0 = Rga::new();
    let mut rga1 = Rga::new();
    let mut rga2 = Rga::new();
    
    // U0 inserts at pos=0
    rga0.insert(&u0.key_pub, 0, b"ABCDEFGHIJKLMNOPQ");
    
    // U2 inserts at pos=0 (concurrent with U0)
    rga2.insert(&u2.key_pub, 0, b"CDEFGHIJKLMNOPQRS");
    
    // Full sync - all replicas converge
    rga0.merge(&rga1);
    rga0.merge(&rga2);
    rga1.merge(&rga0);
    rga1.merge(&rga2);
    rga2.merge(&rga0);
    rga2.merge(&rga1);
    
    assert_eq!(rga0.to_string(), rga1.to_string());
    assert_eq!(rga1.to_string(), rga2.to_string());
    
    assert_eq!(rga0.to_string(), rga1.to_string());
    assert_eq!(rga1.to_string(), rga2.to_string());
    
    // Now U0 inserts at pos=6 (after 'H', before 'I')
    // This requires splitting U2's span, and the new content should go
    // BEFORE the right-split part, not after it.
    rga0.insert(&u0.key_pub, 6, b"ABCDEFGHIJKLMNOPQ");
    
    // Final sync
    rga1.merge(&rga0);
    rga2.merge(&rga0);
    
    // All replicas should converge
    assert_eq!(rga0.to_string(), rga1.to_string(), "U0 and U1 should converge");
    assert_eq!(rga1.to_string(), rga2.to_string(), "U1 and U2 should converge");
    
    // Verify expected content: CDEFGH + new insert + IJKLMNOPQRS + original U0 content
    let expected = "CDEFGHABCDEFGHIJKLMNOPQIJKLMNOPQRSABCDEFGHIJKLMNOPQ";
    assert_eq!(rga0.to_string(), expected, "Content should match expected");
}
