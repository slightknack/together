// model = "claude-opus-4-5"
// created = "2026-01-31"
// modified = "2026-01-31"
// driver = "Isaac Clayton"

//! Property-based tests for the document API.

use proptest::prelude::*;
use together::crdt::rga::{Rga, AnchorBias};
use together::key::KeyPair;

// =============================================================================
// Test helpers
// =============================================================================

/// Generate a random editing operation
#[derive(Clone, Debug)]
enum EditOp {
    Insert { pos_pct: f64, content: Vec<u8> },
    Delete { pos_pct: f64, len_pct: f64 },
}

fn arbitrary_edit_op() -> impl Strategy<Value = EditOp> {
    prop_oneof![
        // Insert operation: position as percentage, content 1-10 ASCII bytes
        // Use ASCII-only bytes to ensure valid UTF-8 for string comparison
        (0.0..=1.0f64, prop::collection::vec(b'a'..=b'z', 1..10))
            .prop_map(|(pos_pct, content)| EditOp::Insert { pos_pct, content }),
        // Delete operation: position and length as percentages
        (0.0..=1.0f64, 0.0..=0.5f64)
            .prop_map(|(pos_pct, len_pct)| EditOp::Delete { pos_pct, len_pct }),
    ]
}

fn apply_edit(rga: &mut Rga, user: &KeyPair, op: &EditOp) {
    let len = rga.len();
    match op {
        EditOp::Insert { pos_pct, content } => {
            let pos = if len == 0 { 0 } else { ((*pos_pct * len as f64) as u64).min(len) };
            rga.insert(&user.key_pub, pos, content);
        }
        EditOp::Delete { pos_pct, len_pct } => {
            if len == 0 {
                return;
            }
            let start = ((*pos_pct * len as f64) as u64).min(len.saturating_sub(1));
            let max_len = len - start;
            let del_len = ((*len_pct * max_len as f64) as u64).max(1).min(max_len);
            if del_len > 0 && start + del_len <= len {
                rga.delete(start, del_len);
            }
        }
    }
}

// =============================================================================
// Slice properties
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// slice(start, end) should equal the corresponding substring of to_string()
    #[test]
    fn slice_equals_substring_of_to_string(
        ops in prop::collection::vec(arbitrary_edit_op(), 1..50),
        start_pct in 0.0..=1.0f64,
        len_pct in 0.0..=0.5f64,
    ) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();
        
        for op in &ops {
            apply_edit(&mut rga, &user, op);
        }
        
        let doc_len = rga.len();
        if doc_len == 0 {
            prop_assert_eq!(rga.slice(0, 0), Some("".to_string()));
            return Ok(());
        }
        
        let start = ((start_pct * doc_len as f64) as u64).min(doc_len);
        let len = ((len_pct * doc_len as f64) as u64).min(doc_len - start);
        let end = start + len;
        
        let full = rga.to_string();
        let expected = &full[start as usize..end as usize];
        prop_assert_eq!(rga.slice(start, end), Some(expected.to_string()));
    }

    /// slice with invalid range returns None
    #[test]
    fn slice_invalid_range_returns_none(
        ops in prop::collection::vec(arbitrary_edit_op(), 0..20),
        start in 0u64..1000,
        end in 0u64..1000,
    ) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();
        
        for op in &ops {
            apply_edit(&mut rga, &user, op);
        }
        
        let len = rga.len();
        if start > len || end > len {
            prop_assert!(rga.slice(start, end).is_none());
        }
    }

    /// slice(0, len) equals to_string()
    #[test]
    fn slice_full_document_equals_to_string(
        ops in prop::collection::vec(arbitrary_edit_op(), 1..50),
    ) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();
        
        for op in &ops {
            apply_edit(&mut rga, &user, op);
        }
        
        let len = rga.len();
        let full = rga.to_string();
        prop_assert_eq!(rga.slice(0, len), Some(full));
    }
}

// =============================================================================
// Anchor properties
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Anchor at a position resolves back to that position (before any edits)
    #[test]
    fn anchor_resolves_to_original_position(
        initial_content in prop::collection::vec(any::<u8>(), 1..100),
        pos_pct in 0.0..1.0f64,
    ) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();
        rga.insert(&user.key_pub, 0, &initial_content);
        
        let len = rga.len();
        let pos = ((pos_pct * len as f64) as u64).min(len.saturating_sub(1));
        
        if let Some(anchor) = rga.anchor_at(pos, AnchorBias::After) {
            prop_assert_eq!(rga.resolve_anchor(&anchor), Some(pos));
        }
    }

    /// Anchor moves correctly when text is inserted before it
    #[test]
    fn anchor_moves_with_insert_before(
        initial_len in 5u64..50,
        anchor_pos_pct in 0.2..0.8f64,
        insert_pos_pct in 0.0..0.2f64,
        insert_len in 1u64..10,
    ) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();
        
        // Create initial content
        let initial: Vec<u8> = (0..initial_len as usize).map(|i| (b'a' + (i % 26) as u8)).collect();
        rga.insert(&user.key_pub, 0, &initial);
        
        let len = rga.len();
        let anchor_pos = ((anchor_pos_pct * len as f64) as u64).min(len.saturating_sub(1));
        let insert_pos = ((insert_pos_pct * anchor_pos as f64) as u64).min(anchor_pos);
        
        let anchor = rga.anchor_at(anchor_pos, AnchorBias::After).unwrap();
        
        // Insert before the anchor
        let insert_content: Vec<u8> = vec![b'X'; insert_len as usize];
        rga.insert(&user.key_pub, insert_pos, &insert_content);
        
        // Anchor should move right by insert_len
        let expected_pos = anchor_pos + insert_len;
        prop_assert_eq!(rga.resolve_anchor(&anchor), Some(expected_pos));
    }

    /// Anchor stays in place when text is inserted after it
    #[test]
    fn anchor_stable_with_insert_after(
        initial_len in 5u64..50,
        anchor_pos_pct in 0.2..0.5f64,
        insert_pos_pct in 0.6..1.0f64,
        insert_len in 1u64..10,
    ) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();
        
        // Create initial content
        let initial: Vec<u8> = (0..initial_len as usize).map(|i| (b'a' + (i % 26) as u8)).collect();
        rga.insert(&user.key_pub, 0, &initial);
        
        let len = rga.len();
        let anchor_pos = ((anchor_pos_pct * len as f64) as u64).min(len.saturating_sub(1));
        let insert_pos = ((insert_pos_pct * len as f64) as u64).max(anchor_pos + 1).min(len);
        
        let anchor = rga.anchor_at(anchor_pos, AnchorBias::After).unwrap();
        
        // Insert after the anchor
        let insert_content: Vec<u8> = vec![b'X'; insert_len as usize];
        rga.insert(&user.key_pub, insert_pos, &insert_content);
        
        // Anchor should stay in place
        prop_assert_eq!(rga.resolve_anchor(&anchor), Some(anchor_pos));
    }

    /// Anchor returns None when its character is deleted
    #[test]
    fn anchor_none_when_deleted(
        initial_len in 5u64..50,
        anchor_pos_pct in 0.2..0.8f64,
    ) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();
        
        // Create initial content
        let initial: Vec<u8> = (0..initial_len as usize).map(|i| (b'a' + (i % 26) as u8)).collect();
        rga.insert(&user.key_pub, 0, &initial);
        
        let len = rga.len();
        let anchor_pos = ((anchor_pos_pct * len as f64) as u64).min(len.saturating_sub(1));
        
        let anchor = rga.anchor_at(anchor_pos, AnchorBias::After).unwrap();
        
        // Delete the anchored character
        rga.delete(anchor_pos, 1);
        
        // Anchor should return None
        prop_assert_eq!(rga.resolve_anchor(&anchor), None);
    }
}

// =============================================================================
// Version properties
// These tests require full versioning implementation on feature branches.
// They will be enabled when versioning is implemented.
// =============================================================================

// NOTE: Version property tests are disabled until versioning is implemented.
// See feature branches: ibc/document-logical, ibc/document-persistent, ibc/document-checkpoint

// =============================================================================
// Combined properties
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    /// Anchor range content equals slice between resolved positions
    #[test]
    fn anchor_range_equals_slice(
        initial_len in 10u64..50,
        start_pct in 0.1..0.4f64,
        end_pct in 0.5..0.9f64,
        ops in prop::collection::vec(arbitrary_edit_op(), 0..10),
    ) {
        let user = KeyPair::generate();
        let mut rga = Rga::new();
        
        // Create initial content
        let initial: Vec<u8> = (0..initial_len as usize).map(|i| (b'a' + (i % 26) as u8)).collect();
        rga.insert(&user.key_pub, 0, &initial);
        
        let len = rga.len();
        let start = ((start_pct * len as f64) as u64).min(len.saturating_sub(2));
        let end = ((end_pct * len as f64) as u64).max(start + 1).min(len);
        
        let range = match rga.anchor_range(start, end) {
            Some(r) => r,
            None => return Ok(()),
        };
        
        // Apply some edits
        for op in &ops {
            apply_edit(&mut rga, &user, op);
        }
        
        // If both anchors resolve, slice_anchored should equal slice
        if let Some(content) = rga.slice_anchored(&range) {
            let start_resolved = rga.resolve_anchor(&range.start);
            let end_resolved = rga.resolve_anchor(&range.end);
            
            if let (Some(s), Some(e)) = (start_resolved, end_resolved) {
                // The end anchor points to the last char IN the range,
                // so slice_anchored returns slice(s, e+1)
                if s <= e + 1 {
                    prop_assert_eq!(rga.slice(s, e + 1), Some(content));
                }
            }
        }
    }
}
