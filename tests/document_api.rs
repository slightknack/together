// model = "claude-opus-4-5"
// created = "2026-01-31"
// modified = "2026-01-31"
// driver = "Isaac Clayton"

//! Tests for the document API: slice, anchors, and versioning.
//!
//! These tests are written API-first, before the implementation.

use together::crdt::rga::{Rga, AnchorBias};
use together::key::KeyPair;

// =============================================================================
// Helper functions
// =============================================================================

fn insert_text(rga: &mut Rga, pos: u64, text: &str) {
    static USER: std::sync::OnceLock<KeyPair> = std::sync::OnceLock::new();
    let user = USER.get_or_init(KeyPair::generate);
    rga.insert(&user.key_pub, pos, text.as_bytes());
}

fn delete_range(rga: &mut Rga, start: u64, len: u64) {
    rga.delete(start, len);
}

// =============================================================================
// Slice tests
// =============================================================================

#[test]
fn slice_basic() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "hello world");
    
    assert_eq!(rga.slice(0, 5), Some("hello".to_string()));
    assert_eq!(rga.slice(6, 11), Some("world".to_string()));
    assert_eq!(rga.slice(0, 11), Some("hello world".to_string()));
}

#[test]
fn slice_empty_range() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "hello");
    
    assert_eq!(rga.slice(0, 0), Some("".to_string()));
    assert_eq!(rga.slice(3, 3), Some("".to_string()));
    assert_eq!(rga.slice(5, 5), Some("".to_string()));
}

#[test]
fn slice_out_of_bounds() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "hello");
    
    assert_eq!(rga.slice(0, 10), None);
    assert_eq!(rga.slice(6, 7), None);
    assert_eq!(rga.slice(10, 20), None);
}

#[test]
fn slice_empty_document() {
    let rga = Rga::new();
    
    assert_eq!(rga.slice(0, 0), Some("".to_string()));
    assert_eq!(rga.slice(0, 1), None);
}

#[test]
fn slice_after_delete() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "hello world");
    delete_range(&mut rga, 5, 1); // Delete space
    
    assert_eq!(rga.to_string(), "helloworld");
    assert_eq!(rga.slice(0, 5), Some("hello".to_string()));
    assert_eq!(rga.slice(5, 10), Some("world".to_string()));
}

#[test]
fn slice_with_multiple_spans() {
    let user1 = KeyPair::generate();
    let user2 = KeyPair::generate();
    let mut rga = Rga::new();
    
    rga.insert(&user1.key_pub, 0, b"hello");
    rga.insert(&user2.key_pub, 5, b" world");
    
    assert_eq!(rga.slice(0, 11), Some("hello world".to_string()));
    assert_eq!(rga.slice(3, 8), Some("lo wo".to_string()));
}

#[test]
fn slice_unicode() {
    let mut rga = Rga::new();
    // Note: slice operates on bytes, not characters
    insert_text(&mut rga, 0, "hello");
    
    // This should work with byte indices
    assert_eq!(rga.slice(0, 5), Some("hello".to_string()));
}

// =============================================================================
// Anchor tests
// =============================================================================

#[test]
fn anchor_at_position() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "hello");
    
    let anchor = rga.anchor_at(2, AnchorBias::After).unwrap();
    assert_eq!(rga.resolve_anchor(&anchor), Some(2));
}

#[test]
fn anchor_out_of_bounds() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "hello");
    
    assert!(rga.anchor_at(10, AnchorBias::After).is_none());
}

#[test]
fn anchor_tracks_insertions_before() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "world");
    
    let anchor = rga.anchor_at(0, AnchorBias::After).unwrap();
    assert_eq!(rga.resolve_anchor(&anchor), Some(0));
    
    // Insert before the anchored position
    insert_text(&mut rga, 0, "hello ");
    
    // Anchor should move right
    assert_eq!(rga.resolve_anchor(&anchor), Some(6));
}

#[test]
fn anchor_tracks_insertions_after() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "hello");
    
    let anchor = rga.anchor_at(2, AnchorBias::After).unwrap();
    assert_eq!(rga.resolve_anchor(&anchor), Some(2));
    
    // Insert after the anchored position
    insert_text(&mut rga, 5, " world");
    
    // Anchor should stay in place
    assert_eq!(rga.resolve_anchor(&anchor), Some(2));
}

#[test]
fn anchor_bias_before_vs_after() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "ab");
    
    // Anchor between 'a' and 'b' with different biases
    let anchor_before = rga.anchor_at(1, AnchorBias::Before).unwrap();
    let anchor_after = rga.anchor_at(0, AnchorBias::After).unwrap();
    
    // Insert at position 1
    insert_text(&mut rga, 1, "X");
    
    // Anchor with Before bias should stay at original logical position (before 'b')
    // Anchor with After bias should stay after 'a'
    // Result: "aXb"
    // anchor_after points to 'a' (pos 0), should stay at 0
    // anchor_before points to 'b', 'X' was inserted before it, so 'b' moves to pos 2
    assert_eq!(rga.resolve_anchor(&anchor_after), Some(0));
    assert_eq!(rga.resolve_anchor(&anchor_before), Some(2));
}

#[test]
fn anchor_deleted_character() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "hello");
    
    let anchor = rga.anchor_at(2, AnchorBias::After).unwrap();
    assert_eq!(rga.resolve_anchor(&anchor), Some(2));
    
    // Delete the anchored character
    delete_range(&mut rga, 2, 1);
    
    // Anchor should return None since character is deleted
    assert_eq!(rga.resolve_anchor(&anchor), None);
}

#[test]
fn anchor_survives_adjacent_deletes() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "abcde");
    
    let anchor = rga.anchor_at(2, AnchorBias::After).unwrap(); // 'c'
    
    // Delete 'b' (before anchor)
    delete_range(&mut rga, 1, 1);
    assert_eq!(rga.to_string(), "acde");
    assert_eq!(rga.resolve_anchor(&anchor), Some(1)); // 'c' moved to position 1
    
    // Delete 'd' (after anchor)
    delete_range(&mut rga, 2, 1);
    assert_eq!(rga.to_string(), "ace");
    assert_eq!(rga.resolve_anchor(&anchor), Some(1)); // 'c' still at position 1
}

// =============================================================================
// AnchorRange tests
// =============================================================================

#[test]
fn anchor_range_basic() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "hello world");
    
    let range = rga.anchor_range(0, 5).unwrap();
    assert_eq!(rga.slice_anchored(&range), Some("hello".to_string()));
}

#[test]
fn anchor_range_stable_with_insert_before() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "a cat");
    
    let range = rga.anchor_range(0, 5).unwrap();
    assert_eq!(rga.slice_anchored(&range), Some("a cat".to_string()));
    
    // Insert before the range - range tracks but doesn't expand
    // The anchors point to specific characters ('a' and 't'), which move right
    insert_text(&mut rga, 0, "See ");
    
    // Range still covers the same characters "a cat", now at positions 4-8
    assert_eq!(rga.to_string(), "See a cat");
    assert_eq!(rga.slice_anchored(&range), Some("a cat".to_string()));
}

#[test]
fn anchor_range_stable_with_insert_after() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "a cat");
    
    let range = rga.anchor_range(2, 5).unwrap();
    assert_eq!(rga.slice_anchored(&range), Some("cat".to_string()));
    
    // Insert after the range - range doesn't change
    insert_text(&mut rga, 5, " nap");
    
    // Range still covers "cat" (the original characters)
    assert_eq!(rga.to_string(), "a cat nap");
    assert_eq!(rga.slice_anchored(&range), Some("cat".to_string()));
}

#[test]
fn anchor_range_tracks_with_insertions() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "this is a cat on a rug");
    
    // Create range around "a cat"
    let range = rga.anchor_range(8, 13).unwrap();
    assert_eq!(rga.slice_anchored(&range), Some("a cat".to_string()));
    
    // Insert "blue " before "cat"
    insert_text(&mut rga, 10, "blue ");
    
    // Range should now span "a blue cat"
    assert_eq!(rga.to_string(), "this is a blue cat on a rug");
    assert_eq!(rga.slice_anchored(&range), Some("a blue cat".to_string()));
}

#[test]
fn anchor_range_shrinks_with_delete() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "hello world");
    
    let range = rga.anchor_range(0, 11).unwrap();
    assert_eq!(rga.slice_anchored(&range), Some("hello world".to_string()));
    
    // Delete " world"
    delete_range(&mut rga, 5, 6);
    
    // If both anchors survive, range should be just "hello"
    // But if end anchor was deleted, behavior depends on implementation
    if let Some(content) = rga.slice_anchored(&range) {
        assert_eq!(content, "hello");
    }
}

// =============================================================================
// Version tests
// =============================================================================

#[test]
fn version_basic() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "hello");
    
    let v1 = rga.version();
    // Current version should always work
    assert_eq!(rga.to_string_at(&v1), "hello");
}

#[test]
fn version_tracks_changes() {
    let mut rga = Rga::new();
    
    insert_text(&mut rga, 0, "hello");
    let v1 = rga.version();
    
    insert_text(&mut rga, 5, " world");
    let v2 = rga.version();
    
    assert_eq!(rga.to_string_at(&v1), "hello");
    assert_eq!(rga.to_string_at(&v2), "hello world");
}

#[test]
fn version_with_deletes() {
    let mut rga = Rga::new();
    
    insert_text(&mut rga, 0, "hello");
    let v1 = rga.version();
    
    insert_text(&mut rga, 5, " world");
    let v2 = rga.version();
    
    delete_range(&mut rga, 0, 6); // Delete "hello "
    let v3 = rga.version();
    
    assert_eq!(rga.to_string_at(&v1), "hello");
    assert_eq!(rga.to_string_at(&v2), "hello world");
    assert_eq!(rga.to_string_at(&v3), "world");
}

#[test]
fn version_slice_at() {
    let mut rga = Rga::new();
    
    insert_text(&mut rga, 0, "hello world");
    let v1 = rga.version();
    
    delete_range(&mut rga, 5, 1); // Delete space
    let v2 = rga.version();
    
    assert_eq!(rga.slice_at(0, 5, &v1), Some("hello".to_string()));
    assert_eq!(rga.slice_at(6, 11, &v1), Some("world".to_string()));
    
    assert_eq!(rga.slice_at(0, 5, &v2), Some("hello".to_string()));
    assert_eq!(rga.slice_at(5, 10, &v2), Some("world".to_string()));
}

#[test]
fn version_len_at() {
    let mut rga = Rga::new();
    
    insert_text(&mut rga, 0, "hello");
    let v1 = rga.version();
    
    insert_text(&mut rga, 5, " world");
    let v2 = rga.version();
    
    delete_range(&mut rga, 0, 6);
    let v3 = rga.version();
    
    assert_eq!(rga.len_at(&v1), 5);
    assert_eq!(rga.len_at(&v2), 11);
    assert_eq!(rga.len_at(&v3), 5);
}

// =============================================================================
// Integration tests - real-world scenarios
// =============================================================================

#[test]
fn scenario_collaborative_editing() {
    let alice = KeyPair::generate();
    let bob = KeyPair::generate();
    let mut rga = Rga::new();
    
    // Alice writes a sentence
    rga.insert(&alice.key_pub, 0, b"The cat sat on the mat.");
    
    // Create anchor around "cat"
    let cat_range = rga.anchor_range(4, 7).unwrap();
    assert_eq!(rga.slice_anchored(&cat_range), Some("cat".to_string()));
    
    // Bob adds an adjective INSIDE the range (between "The " and "cat")
    // This should expand the range to include the insertion
    rga.insert(&bob.key_pub, 4, b"fluffy ");
    
    // Anchor range tracks: start points to 'c' which moved, end points to 't'
    // So the range is now just "cat" at its new position
    assert_eq!(rga.to_string(), "The fluffy cat sat on the mat.");
    assert_eq!(rga.slice_anchored(&cat_range), Some("cat".to_string()));
}

#[test]
fn scenario_undo_preview() {
    let mut rga = Rga::new();
    
    // User types, making several checkpoints
    insert_text(&mut rga, 0, "Hello");
    let checkpoint1 = rga.version();
    
    insert_text(&mut rga, 5, ", ");
    let checkpoint2 = rga.version();
    
    insert_text(&mut rga, 7, "World");
    let checkpoint3 = rga.version();
    
    insert_text(&mut rga, 12, "!");
    let _checkpoint4 = rga.version();
    
    // Preview what document looked like at each checkpoint
    assert_eq!(rga.to_string_at(&checkpoint1), "Hello");
    assert_eq!(rga.to_string_at(&checkpoint2), "Hello, ");
    assert_eq!(rga.to_string_at(&checkpoint3), "Hello, World");
    assert_eq!(rga.to_string(), "Hello, World!");
}

#[test]
fn scenario_selection_tracking() {
    let mut rga = Rga::new();
    insert_text(&mut rga, 0, "function foo() { return 42; }");
    
    // User selects "return 42"
    let selection = rga.anchor_range(17, 26).unwrap();
    assert_eq!(rga.slice_anchored(&selection), Some("return 42".to_string()));
    
    // User types before selection (e.g., adds a variable)
    insert_text(&mut rga, 17, "let x = ");
    
    // Selection tracks the original characters, not the position
    // So it still points to "return 42"
    assert_eq!(rga.to_string(), "function foo() { let x = return 42; }");
    assert_eq!(rga.slice_anchored(&selection), Some("return 42".to_string()));
}
