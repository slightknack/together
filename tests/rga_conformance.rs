// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

//! Conformance test suite for RGA implementations.
//!
//! All implementations of the `Rga` trait must pass these tests.
//! The tests verify:
//!
//! 1. Basic operations: insert, delete, to_string
//! 2. CRDT properties: commutativity, associativity, idempotence
//! 3. Concurrent edit resolution
//! 4. Edge cases: empty docs, unicode, large documents
//!
//! # Usage
//!
//! To test a new implementation, add it to the `test_all_implementations!`
//! macro at the bottom of this file.

use std::hash::Hash;
use together::crdt::rga_trait::Rga;

/// Test context providing user IDs and helper methods.
pub struct TestContext<U> {
    pub user1: U,
    pub user2: U,
    pub user3: U,
}

// =============================================================================
// Basic Operation Tests
// =============================================================================

/// Test that insert at position 0 works.
pub fn test_insert_at_beginning<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId) {
    let mut rga = make_empty();
    rga.insert(&user, 0, b"hello");
    assert_eq!(rga.to_string(), "hello");
    assert_eq!(rga.len(), 5);
}

/// Test that insert at end works.
pub fn test_insert_at_end<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId) {
    let mut rga = make_empty();
    rga.insert(&user, 0, b"hello");
    rga.insert(&user, 5, b" world");
    assert_eq!(rga.to_string(), "hello world");
    assert_eq!(rga.len(), 11);
}

/// Test that insert in middle works.
pub fn test_insert_in_middle<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId) {
    let mut rga = make_empty();
    rga.insert(&user, 0, b"hd");
    rga.insert(&user, 1, b"ello worl");
    assert_eq!(rga.to_string(), "hello world");
}

/// Test multiple sequential inserts.
pub fn test_sequential_inserts<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId) {
    let mut rga = make_empty();
    rga.insert(&user, 0, b"a");
    rga.insert(&user, 1, b"b");
    rga.insert(&user, 2, b"c");
    rga.insert(&user, 3, b"d");
    rga.insert(&user, 4, b"e");
    assert_eq!(rga.to_string(), "abcde");
}

/// Test delete single character.
pub fn test_delete_single<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId) {
    let mut rga = make_empty();
    rga.insert(&user, 0, b"hello");
    rga.delete(2, 1); // Delete 'l'
    assert_eq!(rga.to_string(), "helo");
}

/// Test delete range.
pub fn test_delete_range<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId) {
    let mut rga = make_empty();
    rga.insert(&user, 0, b"hello world");
    rga.delete(5, 6); // Delete " world"
    assert_eq!(rga.to_string(), "hello");
}

/// Test delete at beginning.
pub fn test_delete_at_beginning<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId) {
    let mut rga = make_empty();
    rga.insert(&user, 0, b"hello");
    rga.delete(0, 2); // Delete "he"
    assert_eq!(rga.to_string(), "llo");
}

/// Test delete at end.
pub fn test_delete_at_end<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId) {
    let mut rga = make_empty();
    rga.insert(&user, 0, b"hello");
    rga.delete(3, 2); // Delete "lo"
    assert_eq!(rga.to_string(), "hel");
}

/// Test delete entire content.
pub fn test_delete_all<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId) {
    let mut rga = make_empty();
    rga.insert(&user, 0, b"hello");
    rga.delete(0, 5);
    assert_eq!(rga.to_string(), "");
    assert_eq!(rga.len(), 0);
}

/// Test insert after delete.
pub fn test_insert_after_delete<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId) {
    let mut rga = make_empty();
    rga.insert(&user, 0, b"hello world");
    rga.delete(5, 6); // Delete " world"
    rga.insert(&user, 5, b" rust");
    assert_eq!(rga.to_string(), "hello rust");
}

// =============================================================================
// CRDT Property Tests
// =============================================================================

/// Test merge commutativity: merge(A, B) == merge(B, A)
pub fn test_merge_commutativity<R: Rga>(
    make_empty: impl Fn() -> R,
    user1: R::UserId,
    user2: R::UserId,
) {
    let mut a = make_empty();
    let mut b = make_empty();

    a.insert(&user1, 0, b"hello");
    b.insert(&user2, 0, b"world");

    let mut ab = a.clone();
    ab.merge(&b);

    let mut ba = b.clone();
    ba.merge(&a);

    assert_eq!(ab.to_string(), ba.to_string(), "merge should be commutative");
}

/// Test merge associativity: merge(A, merge(B, C)) == merge(merge(A, B), C)
pub fn test_merge_associativity<R: Rga>(
    make_empty: impl Fn() -> R,
    user1: R::UserId,
    user2: R::UserId,
    user3: R::UserId,
) {
    let mut a = make_empty();
    let mut b = make_empty();
    let mut c = make_empty();

    a.insert(&user1, 0, b"A");
    b.insert(&user2, 0, b"B");
    c.insert(&user3, 0, b"C");

    // (A merge (B merge C))
    let mut bc = b.clone();
    bc.merge(&c);
    let mut a_bc = a.clone();
    a_bc.merge(&bc);

    // ((A merge B) merge C)
    let mut ab = a.clone();
    ab.merge(&b);
    let mut ab_c = ab;
    ab_c.merge(&c);

    assert_eq!(
        a_bc.to_string(),
        ab_c.to_string(),
        "merge should be associative"
    );
}

/// Test merge idempotence: merge(A, A) == A
pub fn test_merge_idempotence<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId) {
    let mut a = make_empty();
    a.insert(&user, 0, b"hello");

    let before = a.to_string();
    let a_clone = a.clone();
    a.merge(&a_clone);
    let after = a.to_string();

    assert_eq!(before, after, "merge should be idempotent");
}

/// Test merging empty documents.
pub fn test_merge_empty<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId) {
    let mut a = make_empty();
    let b = make_empty();

    a.insert(&user, 0, b"hello");
    let before = a.to_string();

    a.merge(&b);
    assert_eq!(a.to_string(), before, "merging empty should not change content");
}

/// Test merging into empty document.
pub fn test_merge_into_empty<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId) {
    let mut a = make_empty();
    let mut b = make_empty();

    b.insert(&user, 0, b"hello");

    a.merge(&b);
    assert_eq!(a.to_string(), "hello");
}

// =============================================================================
// Concurrent Edit Tests
// =============================================================================

/// Test concurrent inserts at same position.
pub fn test_concurrent_insert_same_position<R: Rga>(
    make_empty: impl Fn() -> R,
    user1: R::UserId,
    user2: R::UserId,
) {
    let mut a = make_empty();
    let mut b = make_empty();

    // Both users insert at position 0
    a.insert(&user1, 0, b"A");
    b.insert(&user2, 0, b"B");

    // After merge, one should come before the other consistently
    let mut ab = a.clone();
    ab.merge(&b);

    let mut ba = b.clone();
    ba.merge(&a);

    let result = ab.to_string();
    assert!(
        result == "AB" || result == "BA",
        "concurrent inserts should produce valid interleaving"
    );
    assert_eq!(
        ab.to_string(),
        ba.to_string(),
        "merge order should not affect result"
    );
}

/// Test concurrent inserts at adjacent positions.
pub fn test_concurrent_insert_adjacent<R: Rga>(
    make_empty: impl Fn() -> R,
    user1: R::UserId,
    user2: R::UserId,
) {
    // Start with shared base
    let mut base = make_empty();
    base.insert(&user1, 0, b"ac");

    let mut a = base.clone();
    let mut b = base.clone();

    // User1 inserts 'b' after 'a' (position 1)
    a.insert(&user1, 1, b"b");
    // User2 inserts 'x' after 'a' (position 1)
    b.insert(&user2, 1, b"x");

    let mut merged = a.clone();
    merged.merge(&b);

    let result = merged.to_string();
    // Both 'b' and 'x' should be between 'a' and 'c'
    assert!(
        result == "abxc" || result == "axbc",
        "concurrent inserts should interleave correctly, got: {}",
        result
    );
}

/// Test concurrent delete of same character.
pub fn test_concurrent_delete_same<R: Rga>(
    make_empty: impl Fn() -> R,
    user1: R::UserId,
    user2: R::UserId,
) {
    let mut base = make_empty();
    base.insert(&user1, 0, b"hello");

    let mut a = base.clone();
    let mut b = base.clone();

    // Both delete 'e'
    a.delete(1, 1);
    b.delete(1, 1);

    let mut merged = a.clone();
    merged.merge(&b);

    assert_eq!(merged.to_string(), "hllo", "concurrent deletes should be idempotent");
}

/// Test concurrent insert and delete.
pub fn test_concurrent_insert_delete<R: Rga>(
    make_empty: impl Fn() -> R,
    user1: R::UserId,
    user2: R::UserId,
) {
    let mut base = make_empty();
    base.insert(&user1, 0, b"hello");

    let mut a = base.clone();
    let mut b = base.clone();

    // User1 inserts 'X' at position 2
    a.insert(&user1, 2, b"X");
    // User2 deletes 'll' (positions 2-3)
    b.delete(2, 2);

    let mut merged = a.clone();
    merged.merge(&b);

    // The inserted 'X' should survive, but the deleted chars should be gone
    // Result depends on implementation, but should be deterministic
    let result = merged.to_string();
    assert!(
        result.contains('X'),
        "inserted character should survive, got: {}",
        result
    );
}

// =============================================================================
// Edge Case Tests
// =============================================================================

/// Test empty document operations.
pub fn test_empty_document<R: Rga>(make_empty: impl Fn() -> R) {
    let rga = make_empty();
    assert_eq!(rga.len(), 0);
    assert!(rga.is_empty());
    assert_eq!(rga.to_string(), "");
}

/// Test single character document.
pub fn test_single_character<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId) {
    let mut rga = make_empty();
    rga.insert(&user, 0, b"x");
    assert_eq!(rga.len(), 1);
    assert_eq!(rga.to_string(), "x");

    rga.delete(0, 1);
    assert_eq!(rga.len(), 0);
    assert_eq!(rga.to_string(), "");
}

/// Test Unicode handling.
pub fn test_unicode<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId) {
    let mut rga = make_empty();
    
    // Note: Rga works with bytes, not characters
    // UTF-8 encoding of emoji: 🎉 = 4 bytes
    let emoji = "🎉".as_bytes();
    rga.insert(&user, 0, emoji);
    
    assert_eq!(rga.len(), 4); // 4 bytes
    assert_eq!(rga.to_string(), "🎉");
}

/// Test slice operation.
pub fn test_slice<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId)
where
    R: Rga,
{
    let mut rga = make_empty();
    rga.insert(&user, 0, b"hello world");

    assert_eq!(rga.slice(0, 5), Some("hello".to_string()));
    assert_eq!(rga.slice(6, 11), Some("world".to_string()));
    assert_eq!(rga.slice(0, 11), Some("hello world".to_string()));
    assert_eq!(rga.slice(0, 0), Some("".to_string()));
    assert_eq!(rga.slice(5, 5), Some("".to_string()));
    assert_eq!(rga.slice(0, 100), None); // Out of bounds
    assert_eq!(rga.slice(5, 3), None); // Invalid range
}

// =============================================================================
// Large Document Tests
// =============================================================================

/// Test handling of large documents.
pub fn test_large_document<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId) {
    let mut rga = make_empty();

    // Insert 10,000 characters
    for i in 0..10000 {
        let c = (b'a' + (i % 26) as u8) as char;
        rga.insert(&user, i as u64, c.to_string().as_bytes());
    }

    assert_eq!(rga.len(), 10000);

    // Delete half
    rga.delete(0, 5000);
    assert_eq!(rga.len(), 5000);
}

/// Test many small inserts.
pub fn test_many_small_inserts<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId) {
    let mut rga = make_empty();

    // Simulate typing character by character
    let text = "The quick brown fox jumps over the lazy dog.";
    for (i, c) in text.chars().enumerate() {
        rga.insert(&user, i as u64, c.to_string().as_bytes());
    }

    assert_eq!(rga.to_string(), text);
}

/// Test many small deletes (backspace simulation).
pub fn test_backspace_pattern<R: Rga>(make_empty: impl Fn() -> R, user: R::UserId) {
    let mut rga = make_empty();

    // Type and backspace repeatedly
    rga.insert(&user, 0, b"hello");
    rga.delete(4, 1); // Delete 'o'
    rga.delete(3, 1); // Delete 'l'
    rga.insert(&user, 3, b"p");
    rga.insert(&user, 4, b"!");

    assert_eq!(rga.to_string(), "help!");
}

// =============================================================================
// Test Runner Macro
// =============================================================================

/// Macro to run all conformance tests for an implementation.
#[macro_export]
macro_rules! run_conformance_tests {
    ($impl_name:ident, $make_empty:expr, $user1:expr, $user2:expr, $user3:expr) => {
        mod $impl_name {
            use super::*;

            #[test]
            fn insert_at_beginning() {
                test_insert_at_beginning($make_empty, $user1);
            }

            #[test]
            fn insert_at_end() {
                test_insert_at_end($make_empty, $user1);
            }

            #[test]
            fn insert_in_middle() {
                test_insert_in_middle($make_empty, $user1);
            }

            #[test]
            fn sequential_inserts() {
                test_sequential_inserts($make_empty, $user1);
            }

            #[test]
            fn delete_single() {
                test_delete_single($make_empty, $user1);
            }

            #[test]
            fn delete_range() {
                test_delete_range($make_empty, $user1);
            }

            #[test]
            fn delete_at_beginning() {
                test_delete_at_beginning($make_empty, $user1);
            }

            #[test]
            fn delete_at_end() {
                test_delete_at_end($make_empty, $user1);
            }

            #[test]
            fn delete_all() {
                test_delete_all($make_empty, $user1);
            }

            #[test]
            fn insert_after_delete() {
                test_insert_after_delete($make_empty, $user1);
            }

            #[test]
            fn merge_commutativity() {
                test_merge_commutativity($make_empty, $user1, $user2);
            }

            #[test]
            fn merge_associativity() {
                test_merge_associativity($make_empty, $user1, $user2, $user3);
            }

            #[test]
            fn merge_idempotence() {
                test_merge_idempotence($make_empty, $user1);
            }

            #[test]
            fn merge_empty() {
                test_merge_empty($make_empty, $user1);
            }

            #[test]
            fn merge_into_empty() {
                test_merge_into_empty($make_empty, $user1);
            }

            #[test]
            fn concurrent_insert_same_position() {
                test_concurrent_insert_same_position($make_empty, $user1, $user2);
            }

            #[test]
            fn concurrent_insert_adjacent() {
                test_concurrent_insert_adjacent($make_empty, $user1, $user2);
            }

            #[test]
            fn concurrent_delete_same() {
                test_concurrent_delete_same($make_empty, $user1, $user2);
            }

            #[test]
            fn concurrent_insert_delete() {
                test_concurrent_insert_delete($make_empty, $user1, $user2);
            }

            #[test]
            fn empty_document() {
                test_empty_document($make_empty);
            }

            #[test]
            fn single_character() {
                test_single_character($make_empty, $user1);
            }

            #[test]
            fn unicode() {
                test_unicode($make_empty, $user1);
            }

            #[test]
            fn slice() {
                test_slice($make_empty, $user1);
            }

            #[test]
            fn large_document() {
                test_large_document($make_empty, $user1);
            }

            #[test]
            fn many_small_inserts() {
                test_many_small_inserts($make_empty, $user1);
            }

            #[test]
            fn backspace_pattern() {
                test_backspace_pattern($make_empty, $user1);
            }
        }
    };
}

// =============================================================================
// Tests for implementations
// =============================================================================

use together::crdt::yjs::YjsRga;
use together::key::KeyPair;
use together::key::KeyPub;

fn make_yjs_rga() -> YjsRga {
    return YjsRga::new();
}

fn make_user() -> KeyPub {
    return KeyPair::generate().key_pub;
}

run_conformance_tests!(
    yjs_rga,
    make_yjs_rga,
    make_user(),
    make_user(),
    make_user()
);
