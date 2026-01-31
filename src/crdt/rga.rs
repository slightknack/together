// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-01-30"
// driver = "Isaac Clayton"

//! Replicated Growable Array (RGA) implementation.
//!
//! This is a sequence CRDT optimized for text editing. Key design decisions:
//!
//! 1. **Spans**: Consecutive insertions by the same user are stored as a single
//!    span rather than individual items. This reduces memory ~14x in practice.
//!
//! 2. **Weighted list**: Spans are stored in a weighted list where each span's
//!    weight is its visible character count. This enables position lookup by
//!    character offset.
//!
//! 3. **Append-only columns**: Each user has a column that only appends. This
//!    makes replication trivial - just send new entries.
//!
//! 4. **ItemId index**: A HashMap from (user, seq) to span index enables O(1)
//!    lookup of spans by their CRDT identifier.

use std::collections::HashMap;

use crate::key::KeyPub;
use super::weighted_list::WeightedList;

/// A unique identifier for an item in the RGA.
/// Composed of the user's public key and a sequence number.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId {
    pub user: KeyPub,
    pub seq: u64,
}

impl std::fmt::Debug for ItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return write!(f, "ItemId({:?}, {})", self.user, self.seq);
    }
}

/// A span of consecutive items inserted by the same user.
/// Represents items with IDs (user, seq) through (user, seq + len - 1).
#[derive(Clone, Copy, Debug)]
pub struct Span {
    /// The user who created this span.
    pub user: KeyPub,
    /// The starting sequence number.
    pub seq: u64,
    /// Number of items in this span.
    pub len: u64,
    /// The ID of the item this span was inserted after.
    /// None means inserted at the beginning.
    pub origin: Option<ItemId>,
    /// Offset into the content backing store.
    pub content_offset: usize,
    /// Whether this span has been deleted.
    pub deleted: bool,
}

impl Span {
    /// Get the ItemId for a position within this span.
    pub fn id_at(&self, offset: u64) -> ItemId {
        assert!(offset < self.len);
        return ItemId {
            user: self.user.clone(),
            seq: self.seq + offset,
        };
    }

    /// Check if this span contains the given ItemId.
    pub fn contains(&self, id: &ItemId) -> bool {
        return self.user == id.user
            && id.seq >= self.seq
            && id.seq < self.seq + self.len;
    }

    /// Split this span at the given offset, returning the right half.
    pub fn split(&mut self, offset: u64) -> Span {
        assert!(offset > 0 && offset < self.len);
        let right = Span {
            user: self.user.clone(),
            seq: self.seq + offset,
            len: self.len - offset,
            origin: Some(self.id_at(offset - 1)),
            content_offset: self.content_offset + offset as usize,
            deleted: self.deleted,
        };
        self.len = offset;
        return right;
    }

    /// Visible length (0 if deleted, len otherwise).
    pub fn visible_len(&self) -> u64 {
        if self.deleted {
            return 0;
        }
        return self.len;
    }
}

/// Per-user append-only column storing content.
#[derive(Clone, Debug)]
struct Column {
    /// The content bytes for this user's insertions.
    content: Vec<u8>,
    /// Next sequence number to assign.
    next_seq: u64,
}

impl Column {
    fn new() -> Column {
        return Column {
            content: Vec::new(),
            next_seq: 0,
        };
    }
}

/// A Replicated Growable Array.
///
/// Uses a weighted list of spans where each span's weight is its visible
/// character count. This enables O(log n) position lookup once the weighted
/// list is optimized.
pub struct Rga {
    /// Spans in document order, weighted by visible character count.
    spans: WeightedList<Span>,
    /// Per-user columns for content storage.
    columns: HashMap<KeyPub, Column>,
}

impl Rga {
    /// Create a new empty RGA.
    pub fn new() -> Rga {
        return Rga {
            spans: WeightedList::new(),
            columns: HashMap::new(),
        };
    }

    /// Get the visible length (excluding deleted items).
    pub fn len(&self) -> u64 {
        return self.spans.total_weight();
    }

    /// Check if the RGA is empty.
    pub fn is_empty(&self) -> bool {
        return self.spans.total_weight() == 0;
    }

    /// Get the number of spans (for profiling).
    pub fn span_count(&self) -> usize {
        return self.spans.len();
    }

    /// Insert content after the given visible position.
    /// Position 0 means insert at the beginning.
    /// Returns the ItemId of the first inserted item.
    pub fn insert(&mut self, user: &KeyPub, pos: u64, content: &[u8]) -> ItemId {
        if content.is_empty() {
            panic!("cannot insert empty content");
        }

        // Get or create the user's column
        let column = self.columns.entry(user.clone()).or_insert_with(Column::new);
        let seq = column.next_seq;
        let content_offset = column.content.len();
        column.content.extend_from_slice(content);
        column.next_seq += content.len() as u64;

        // Find the origin (the item we're inserting after)
        let origin = if pos == 0 {
            None
        } else {
            Some(self.id_at_visible_pos(pos - 1))
        };

        // Create the span
        let span = Span {
            user: user.clone(),
            seq,
            len: content.len() as u64,
            origin,
            content_offset,
            deleted: false,
        };

        let id = span.id_at(0);
        self.insert_span_at_pos(span, pos);
        return id;
    }

    /// Delete a range of visible characters starting at `start`.
    pub fn delete(&mut self, start: u64, len: u64) {
        if len == 0 {
            return;
        }
        let visible_len = self.spans.total_weight();
        if start + len > visible_len {
            panic!(
                "delete range {}..{} out of bounds (visible_len={})",
                start,
                start + len,
                visible_len
            );
        }

        let mut remaining = len;

        while remaining > 0 {
            // Find the span at current visible position
            let (span_idx, offset_in_span) = match self.spans.find_by_weight(start) {
                Some((idx, off)) => (idx, off),
                None => panic!("position {} not found", start),
            };

            let span = self.spans.get(span_idx).unwrap();
            let span_visible = span.visible_len();

            if offset_in_span == 0 && remaining >= span_visible {
                // Delete entire span - mark as deleted and update weight to 0
                self.spans.get_mut(span_idx).unwrap().deleted = true;
                self.spans.update_weight(span_idx, 0);
                remaining -= span_visible;
            } else if offset_in_span == 0 {
                // Delete prefix of span - split and delete left part
                let mut span = self.spans.remove(span_idx);
                let right = span.split(remaining);
                span.deleted = true;
                self.spans.insert(span_idx, span, 0);
                self.spans.insert(span_idx + 1, right, right.visible_len());
                remaining = 0;
            } else if offset_in_span + remaining >= span_visible {
                // Delete suffix of span - split and delete right part
                let to_delete = span_visible - offset_in_span;
                let mut span = self.spans.remove(span_idx);
                let mut right = span.split(offset_in_span);
                right.deleted = true;
                self.spans.insert(span_idx, span, span.visible_len());
                self.spans.insert(span_idx + 1, right, 0);
                remaining -= to_delete;
            } else {
                // Delete middle of span - split twice
                let mut span = self.spans.remove(span_idx);
                let mut mid_right = span.split(offset_in_span);
                let right = mid_right.split(remaining);
                mid_right.deleted = true;
                self.spans.insert(span_idx, span, span.visible_len());
                self.spans.insert(span_idx + 1, mid_right, 0);
                self.spans.insert(span_idx + 2, right, right.visible_len());
                remaining = 0;
            }
        }
    }

    /// Get the content as a string (assumes UTF-8).
    pub fn to_string(&self) -> String {
        let mut result = Vec::new();
        for span in self.spans.iter() {
            if !span.deleted {
                let column = self.columns.get(&span.user).unwrap();
                let start = span.content_offset;
                let end = start + span.len as usize;
                result.extend_from_slice(&column.content[start..end]);
            }
        }
        return String::from_utf8(result).unwrap_or_default();
    }

    /// Get the ItemId at a visible position.
    fn id_at_visible_pos(&mut self, pos: u64) -> ItemId {
        let (span_idx, offset) = self.spans.find_by_weight(pos)
            .expect("position out of bounds");
        return self.spans.get(span_idx).unwrap().id_at(offset);
    }

    /// Insert a span at the given visible position (for local edits).
    fn insert_span_at_pos(&mut self, span: Span, pos: u64) {
        let span_len = span.visible_len();

        if self.spans.is_empty() || pos == 0 {
            self.spans.insert(0, span, span_len);
            return;
        }

        // Find where to insert by weight position
        match self.spans.find_by_weight(pos) {
            Some((idx, offset_in_span)) => {
                if offset_in_span > 0 {
                    // Need to split the span
                    let mut existing = self.spans.remove(idx);
                    let right = existing.split(offset_in_span);
                    self.spans.insert(idx, existing, existing.visible_len());
                    self.spans.insert(idx + 1, span, span_len);
                    self.spans.insert(idx + 2, right, right.visible_len());
                } else {
                    // Insert before this span
                    self.spans.insert(idx, span, span_len);
                }
            }
            None => {
                // pos >= total_weight, insert at end
                self.spans.insert(self.spans.len(), span, span_len);
            }
        }
    }

    /// Find span containing the given ItemId using linear search.
    fn find_span_by_id(&self, id: &ItemId) -> Option<usize> {
        for (i, span) in self.spans.iter().enumerate() {
            if span.contains(id) {
                return Some(i);
            }
        }
        return None;
    }
}

impl Default for Rga {
    fn default() -> Self {
        return Self::new();
    }
}

// --- Operation-based interface for integration with logs ---

use super::op::ItemId as OpItemId;
use super::op::Op;
use super::op::OpBlock;

impl Rga {
    /// Convert an op::ItemId to an rga::ItemId.
    fn convert_id(id: &OpItemId) -> ItemId {
        return ItemId {
            user: id.user.clone(),
            seq: id.seq,
        };
    }

    /// Apply an operation from a writer.
    /// Returns true if the operation was applied, false if it was already present.
    pub fn apply(&mut self, user: &KeyPub, block: &OpBlock) -> bool {
        match &block.op {
            Op::Insert { origin, seq, len } => {
                // Check if we already have this insertion
                if let Some(column) = self.columns.get(user) {
                    if *seq < column.next_seq {
                        return false;
                    }
                }

                // Get or create the user's column
                let column = self.columns.entry(user.clone()).or_insert_with(Column::new);

                // Verify sequence is contiguous
                if *seq != column.next_seq {
                    panic!("sequence gap: expected {}, got {}", column.next_seq, seq);
                }

                let content_offset = column.content.len();
                column.content.extend_from_slice(&block.content);
                column.next_seq += *len;

                // Create the span
                let span = Span {
                    user: user.clone(),
                    seq: *seq,
                    len: *len,
                    origin: origin.as_ref().map(Self::convert_id),
                    content_offset,
                    deleted: false,
                };

                self.insert_span_rga(span);
                return true;
            }
            Op::Delete { target } => {
                let target_id = Self::convert_id(target);
                return self.delete_by_id(&target_id);
            }
        }
    }

    /// Insert a span using RGA ordering rules.
    /// When multiple spans have the same origin, order by (user, seq) descending.
    fn insert_span_rga(&mut self, span: Span) {
        let span_len = span.visible_len();

        if self.spans.is_empty() {
            self.spans.insert(0, span, span_len);
            return;
        }

        let insert_idx = if let Some(ref origin) = span.origin {
            // Find the origin span
            if let Some(origin_idx) = self.find_span_by_id(origin) {
                let origin_span = self.spans.get(origin_idx).unwrap();
                let offset_in_span = origin.seq - origin_span.seq;

                // If origin is in the middle of a span, split it
                if offset_in_span < origin_span.len - 1 {
                    let mut existing = self.spans.remove(origin_idx);
                    let right = existing.split(offset_in_span + 1);
                    self.spans.insert(origin_idx, existing.clone(), existing.visible_len());
                    self.spans.insert(origin_idx + 1, right.clone(), right.visible_len());
                }

                // Insert after origin, respecting RGA ordering
                let mut pos = origin_idx + 1;
                while pos < self.spans.len() {
                    let other = self.spans.get(pos).unwrap();
                    if let Some(ref other_origin) = other.origin {
                        if other_origin == origin {
                            if (&other.user, other.seq) > (&span.user, span.seq) {
                                pos += 1;
                                continue;
                            }
                        }
                    }
                    break;
                }
                pos
            } else {
                self.spans.len()
            }
        } else {
            // No origin - insert at beginning with RGA ordering
            let mut pos = 0;
            while pos < self.spans.len() {
                let other = self.spans.get(pos).unwrap();
                if other.origin.is_none() {
                    if (&other.user, other.seq) > (&span.user, span.seq) {
                        pos += 1;
                        continue;
                    }
                }
                break;
            }
            pos
        };

        self.spans.insert(insert_idx, span, span_len);
    }

    /// Delete a single item by its ID.
    fn delete_by_id(&mut self, id: &ItemId) -> bool {
        let idx = match self.find_span_by_id(id) {
            Some(i) => i,
            None => return false,
        };

        let span = self.spans.get(idx).unwrap();
        if span.deleted {
            return false;
        }

        let offset = id.seq - span.seq;
        let span_len = span.len;

        if span_len == 1 {
            self.spans.get_mut(idx).unwrap().deleted = true;
            self.spans.update_weight(idx, 0);
        } else if offset == 0 {
            // Delete first item
            let mut existing = self.spans.remove(idx);
            let right = existing.split(1);
            existing.deleted = true;
            self.spans.insert(idx, existing, 0);
            self.spans.insert(idx + 1, right.clone(), right.visible_len());
        } else if offset == span_len - 1 {
            // Delete last item
            let mut existing = self.spans.remove(idx);
            let mut right = existing.split(offset);
            right.deleted = true;
            self.spans.insert(idx, existing.clone(), existing.visible_len());
            self.spans.insert(idx + 1, right, 0);
        } else {
            // Delete middle item
            let mut existing = self.spans.remove(idx);
            let mut mid_right = existing.split(offset);
            let right = mid_right.split(1);
            mid_right.deleted = true;
            self.spans.insert(idx, existing.clone(), existing.visible_len());
            self.spans.insert(idx + 1, mid_right, 0);
            self.spans.insert(idx + 2, right.clone(), right.visible_len());
        }

        return true;
    }
}

impl super::Crdt for Rga {
    fn merge(&mut self, other: &Self) {
        for span in other.spans.iter() {
            if self.find_span_by_id(&span.id_at(0)).is_some() {
                continue;
            }

            let other_column = other.columns.get(&span.user).unwrap();
            let content =
                &other_column.content[span.content_offset..span.content_offset + span.len as usize];

            let origin = span.origin.as_ref().map(|id| OpItemId {
                user: id.user.clone(),
                seq: id.seq,
            });
            let block = OpBlock::insert(origin, span.seq, content.to_vec());
            self.apply(&span.user, &block);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::KeyPair;

    #[test]
    fn empty_rga() {
        let rga = Rga::new();
        assert_eq!(rga.len(), 0);
        assert!(rga.is_empty());
        assert_eq!(rga.to_string(), "");
    }

    #[test]
    fn insert_at_beginning() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        rga.insert(&pair.key_pub, 0, b"hello");
        assert_eq!(rga.len(), 5);
        assert_eq!(rga.to_string(), "hello");
    }

    #[test]
    fn insert_at_end() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        rga.insert(&pair.key_pub, 0, b"hello");
        rga.insert(&pair.key_pub, 5, b" world");
        assert_eq!(rga.len(), 11);
        assert_eq!(rga.to_string(), "hello world");
    }

    #[test]
    fn insert_in_middle() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        rga.insert(&pair.key_pub, 0, b"helo");
        rga.insert(&pair.key_pub, 2, b"l");
        assert_eq!(rga.to_string(), "hello");
    }

    #[test]
    fn multiple_users() {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();
        let mut rga = Rga::new();

        rga.insert(&alice.key_pub, 0, b"hello");
        rga.insert(&bob.key_pub, 5, b" world");

        assert_eq!(rga.to_string(), "hello world");
    }

    #[test]
    fn delete_entire_span() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        rga.insert(&pair.key_pub, 0, b"hello");
        rga.delete(0, 5);
        assert_eq!(rga.len(), 0);
        assert_eq!(rga.to_string(), "");
    }

    #[test]
    fn delete_prefix() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        rga.insert(&pair.key_pub, 0, b"hello");
        rga.delete(0, 2);
        assert_eq!(rga.len(), 3);
        assert_eq!(rga.to_string(), "llo");
    }

    #[test]
    fn delete_suffix() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        rga.insert(&pair.key_pub, 0, b"hello");
        rga.delete(3, 2);
        assert_eq!(rga.len(), 3);
        assert_eq!(rga.to_string(), "hel");
    }

    #[test]
    fn delete_middle() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        rga.insert(&pair.key_pub, 0, b"hello");
        rga.delete(1, 3);
        assert_eq!(rga.len(), 2);
        assert_eq!(rga.to_string(), "ho");
    }

    #[test]
    fn delete_across_spans() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        rga.insert(&pair.key_pub, 0, b"hello");
        rga.insert(&pair.key_pub, 5, b" world");
        rga.delete(3, 5); // "lo wo"
        assert_eq!(rga.len(), 6);
        assert_eq!(rga.to_string(), "helrld");
    }

    #[test]
    fn span_contains() {
        let pair = KeyPair::generate();
        let span = Span {
            user: pair.key_pub.clone(),
            seq: 10,
            len: 5,
            origin: None,
            content_offset: 0,
            deleted: false,
        };

        assert!(span.contains(&ItemId { user: pair.key_pub.clone(), seq: 10 }));
        assert!(span.contains(&ItemId { user: pair.key_pub.clone(), seq: 14 }));
        assert!(!span.contains(&ItemId { user: pair.key_pub.clone(), seq: 9 }));
        assert!(!span.contains(&ItemId { user: pair.key_pub.clone(), seq: 15 }));
    }

    #[test]
    fn span_split() {
        let pair = KeyPair::generate();
        let mut span = Span {
            user: pair.key_pub.clone(),
            seq: 10,
            len: 10,
            origin: None,
            content_offset: 0,
            deleted: false,
        };

        let right = span.split(4);

        assert_eq!(span.seq, 10);
        assert_eq!(span.len, 4);
        assert_eq!(right.seq, 14);
        assert_eq!(right.len, 6);
        assert_eq!(right.content_offset, 4);
    }

    #[test]
    fn item_id_at() {
        let pair = KeyPair::generate();
        let span = Span {
            user: pair.key_pub.clone(),
            seq: 100,
            len: 5,
            origin: None,
            content_offset: 0,
            deleted: false,
        };

        let id = span.id_at(3);
        assert_eq!(id.seq, 103);
    }

    #[test]
    fn apply_insert_at_beginning() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();

        let block = OpBlock::insert(None, 0, b"hello".to_vec());
        let applied = rga.apply(&pair.key_pub, &block);

        assert!(applied);
        assert_eq!(rga.to_string(), "hello");
    }

    #[test]
    fn apply_insert_after_existing() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();

        let block1 = OpBlock::insert(None, 0, b"hello".to_vec());
        rga.apply(&pair.key_pub, &block1);

        let origin = OpItemId {
            user: pair.key_pub.clone(),
            seq: 4,
        };
        let block2 = OpBlock::insert(Some(origin), 5, b" world".to_vec());
        rga.apply(&pair.key_pub, &block2);

        assert_eq!(rga.to_string(), "hello world");
    }

    #[test]
    fn apply_idempotent() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();

        let block = OpBlock::insert(None, 0, b"hello".to_vec());

        assert!(rga.apply(&pair.key_pub, &block));
        assert!(!rga.apply(&pair.key_pub, &block));

        assert_eq!(rga.to_string(), "hello");
    }

    #[test]
    fn apply_delete() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();

        let block1 = OpBlock::insert(None, 0, b"hello".to_vec());
        rga.apply(&pair.key_pub, &block1);

        let target = OpItemId {
            user: pair.key_pub.clone(),
            seq: 1,
        };
        let block2 = OpBlock::delete(target);
        rga.apply(&pair.key_pub, &block2);

        assert_eq!(rga.to_string(), "hllo");
    }

    #[test]
    fn merge_two_rgas() {
        use crate::crdt::Crdt;

        let alice = KeyPair::generate();
        let bob = KeyPair::generate();

        let mut rga_a = Rga::new();
        let mut rga_b = Rga::new();

        rga_a.insert(&alice.key_pub, 0, b"hello");
        rga_b.insert(&bob.key_pub, 0, b"world");

        rga_a.merge(&rga_b);

        let result = rga_a.to_string();
        assert!(result.contains("hello"));
        assert!(result.contains("world"));
        assert_eq!(result.len(), 10);
    }

    #[test]
    fn concurrent_inserts_same_position() {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();

        let mut rga = Rga::new();

        let block_a = OpBlock::insert(None, 0, b"A".to_vec());
        let block_b = OpBlock::insert(None, 0, b"B".to_vec());

        rga.apply(&alice.key_pub, &block_a);
        rga.apply(&bob.key_pub, &block_b);

        let result = rga.to_string();
        assert_eq!(result.len(), 2);
        assert!(result == "AB" || result == "BA");
    }
}

#[cfg(test)]
mod trace_repro_tests {
    use super::*;
    use crate::key::KeyPair;

    #[test]
    fn sequential_inserts_at_increasing_positions() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        
        // Insert large content at pos 0
        let content0: Vec<u8> = (0..1406).map(|i| (i % 256) as u8).collect();
        rga.insert(&pair.key_pub, 0, &content0);
        assert_eq!(rga.len(), 1406);
        
        // Insert at positions 7, 8, 9, 10 - each after the previous insert
        rga.insert(&pair.key_pub, 7, b"a");
        assert_eq!(rga.len(), 1407);
        
        rga.insert(&pair.key_pub, 8, b"b");
        assert_eq!(rga.len(), 1408);
        
        rga.insert(&pair.key_pub, 9, b"c");
        assert_eq!(rga.len(), 1409);
        
        rga.insert(&pair.key_pub, 10, b"d");
        assert_eq!(rga.len(), 1410);
    }
}
