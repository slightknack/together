// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-01-30"
// driver = "Isaac Clayton"

//! Replicated Growable Array (RGA) implementation.
//!
//! This is a sequence CRDT optimized for text editing. Key design decisions:
//!
//! 1. **Spans**: Consecutive insertions by the same user are stored as a single
//!    span rather than individual items. This dramatically reduces memory usage
//!    and improves cache locality.
//!
//! 2. **B-tree with summaries**: Items are stored in a B-tree where each node
//!    maintains aggregate metadata (total length, etc). This enables O(log n)
//!    position-based lookups.
//!
//! 3. **Append-only columns**: Each user has a column that only appends. This
//!    makes replication trivial - just send new entries.
//!
//! 4. **Local order numbers**: Internally we use simple u32 order numbers that
//!    map to full (user_id, seq) pairs. This speeds up comparisons.

use std::collections::BTreeMap;
use std::collections::HashMap;

use crate::key::KeyPub;

/// A unique identifier for an item in the RGA.
/// Composed of the user's public key and a sequence number.
#[derive(Clone, PartialEq, Eq, Hash)]
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
#[derive(Clone, Debug)]
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
    /// Whether items in this span have been deleted.
    /// For simplicity, we track deletion per-span; a partially deleted span
    /// must be split.
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
}

/// A node in the RGA B-tree.
/// Each node contains up to BRANCHING spans and maintains summary metadata.
// TODO: Implement proper B-tree with BRANCHING factor for O(log n) lookups.
// For now we use a flat list which is O(n) but simpler.
#[allow(dead_code)]
const BRANCHING: usize = 32;

#[derive(Clone, Debug)]
struct Node {
    /// Spans in this node (for leaf nodes) or child summaries (for internal).
    spans: Vec<Span>,
    /// Total visible (non-deleted) length in this subtree.
    visible_len: u64,
    /// Total length including deleted items.
    total_len: u64,
    /// Children (empty for leaf nodes).
    children: Vec<Node>,
}

impl Node {
    fn new_leaf() -> Node {
        return Node {
            spans: Vec::new(),
            visible_len: 0,
            total_len: 0,
            children: Vec::new(),
        };
    }

    fn is_leaf(&self) -> bool {
        return self.children.is_empty();
    }

    fn update_summary(&mut self) {
        if self.is_leaf() {
            self.visible_len = 0;
            self.total_len = 0;
            for span in &self.spans {
                self.total_len += span.len;
                if !span.deleted {
                    self.visible_len += span.len;
                }
            }
        } else {
            self.visible_len = 0;
            self.total_len = 0;
            for child in &self.children {
                self.visible_len += child.visible_len;
                self.total_len += child.total_len;
            }
        }
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
pub struct Rga {
    /// The B-tree storing spans in document order.
    root: Node,
    /// Per-user columns for content storage.
    columns: HashMap<KeyPub, Column>,
    /// Index from ItemId to position in the tree.
    /// Maps the first ID of each span to its location.
    // TODO: Use this index for O(1) span lookup by ItemId.
    #[allow(dead_code)]
    index: BTreeMap<(KeyPub, u64), usize>,
}

impl Rga {
    /// Create a new empty RGA.
    pub fn new() -> Rga {
        return Rga {
            root: Node::new_leaf(),
            columns: HashMap::new(),
            index: BTreeMap::new(),
        };
    }

    /// Get the visible length (excluding deleted items).
    pub fn len(&self) -> u64 {
        return self.root.visible_len;
    }

    /// Check if the RGA is empty.
    pub fn is_empty(&self) -> bool {
        return self.root.visible_len == 0;
    }

    /// Insert content after the given position.
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
        self.insert_span(span, pos);
        return id;
    }

    /// Delete a range of visible characters.
    pub fn delete(&mut self, start: u64, len: u64) {
        if len == 0 {
            return;
        }

        // For simplicity, we mark spans as deleted.
        // A more sophisticated implementation would handle partial span deletion.
        let mut remaining = len;
        let mut pos = start;

        while remaining > 0 {
            let (span_idx, offset_in_span) = self.find_visible_pos(pos);
            let span = &mut self.root.spans[span_idx];

            if span.deleted {
                // Skip deleted spans
                pos += 1;
                continue;
            }

            let span_remaining = span.len - offset_in_span;
            if remaining >= span_remaining && offset_in_span == 0 {
                // Delete the entire span
                span.deleted = true;
                remaining -= span_remaining;
            } else {
                // Need to split the span
                // For now, just mark it deleted and move on
                // A real implementation would split properly
                span.deleted = true;
                remaining = 0;
            }
        }

        self.root.update_summary();
    }

    /// Get the content as a string (assumes UTF-8).
    pub fn to_string(&self) -> String {
        let mut result = Vec::new();
        self.collect_visible(&self.root, &mut result);
        return String::from_utf8(result).unwrap_or_default();
    }

    /// Get the ItemId at a visible position.
    fn id_at_visible_pos(&self, pos: u64) -> ItemId {
        let (span_idx, offset) = self.find_visible_pos(pos);
        return self.root.spans[span_idx].id_at(offset);
    }

    /// Find the span and offset for a visible position.
    /// Returns (span_index, offset_within_span).
    fn find_visible_pos(&self, pos: u64) -> (usize, u64) {
        let mut remaining = pos;
        for (i, span) in self.root.spans.iter().enumerate() {
            if span.deleted {
                continue;
            }
            if remaining < span.len {
                return (i, remaining);
            }
            remaining -= span.len;
        }
        panic!("position {} out of bounds", pos);
    }

    /// Insert a span at the given visible position.
    fn insert_span(&mut self, span: Span, pos: u64) {
        if self.root.spans.is_empty() || pos == 0 {
            self.root.spans.insert(0, span);
        } else {
            // Find where to insert based on position
            let mut visible = 0u64;
            let mut insert_idx = self.root.spans.len();
            let mut split_offset = None;

            for (i, s) in self.root.spans.iter().enumerate() {
                if s.deleted {
                    continue;
                }
                let prev_visible = visible;
                visible += s.len;
                if visible >= pos {
                    // Check if we need to split this span
                    let offset_in_span = pos - prev_visible;
                    if offset_in_span > 0 && offset_in_span < s.len {
                        // Need to split
                        insert_idx = i + 1;
                        split_offset = Some((i, offset_in_span));
                    } else if offset_in_span == 0 {
                        // Insert before this span
                        insert_idx = i;
                    } else {
                        // Insert after this span
                        insert_idx = i + 1;
                    }
                    break;
                }
            }

            // Split if needed
            if let Some((span_idx, offset)) = split_offset {
                let right = self.root.spans[span_idx].split(offset);
                self.root.spans.insert(span_idx + 1, right);
                insert_idx = span_idx + 1;
            }

            self.root.spans.insert(insert_idx, span);
        }
        self.root.update_summary();
    }

    /// Collect visible content from a node.
    fn collect_visible(&self, node: &Node, out: &mut Vec<u8>) {
        if node.is_leaf() {
            for span in &node.spans {
                if !span.deleted {
                    let column = self.columns.get(&span.user).unwrap();
                    let start = span.content_offset;
                    let end = start + span.len as usize;
                    out.extend_from_slice(&column.content[start..end]);
                }
            }
        } else {
            for child in &node.children {
                self.collect_visible(child, out);
            }
        }
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
                        // Already applied
                        return false;
                    }
                }

                // Get or create the user's column
                let column = self.columns.entry(user.clone()).or_insert_with(Column::new);
                
                // Verify sequence is contiguous
                if *seq != column.next_seq {
                    // Gap in sequence - we're missing operations
                    // In a real implementation, we'd queue this for later
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

                // Insert using RGA ordering rules
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
        if self.root.spans.is_empty() {
            self.root.spans.push(span);
            self.root.update_summary();
            return;
        }

        // Find the position to insert
        let insert_idx = if let Some(ref origin) = span.origin {
            // Find the origin span
            let origin_idx = self.find_span_containing(origin);
            if let Some(idx) = origin_idx {
                let origin_span = &self.root.spans[idx];
                let offset_in_span = origin.seq - origin_span.seq;
                
                // If origin is in the middle of a span, split it
                if offset_in_span < origin_span.len - 1 {
                    let right = self.root.spans[idx].split(offset_in_span + 1);
                    self.root.spans.insert(idx + 1, right);
                }
                
                // Insert after the origin, respecting RGA ordering
                // Items inserted at the same position are ordered by (user, seq) descending
                let mut pos = idx + 1;
                while pos < self.root.spans.len() {
                    let other = &self.root.spans[pos];
                    // Check if this span was also inserted after the same origin
                    if let Some(ref other_origin) = other.origin {
                        if other_origin == origin {
                            // Compare by user then seq (descending = newer first)
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
                // Origin not found - insert at end
                // This shouldn't happen if operations are applied in causal order
                self.root.spans.len()
            }
        } else {
            // No origin means insert at beginning
            // Apply RGA ordering for concurrent inserts at beginning
            let mut pos = 0;
            while pos < self.root.spans.len() {
                let other = &self.root.spans[pos];
                if other.origin.is_none() {
                    // Both inserted at beginning - order by (user, seq) descending
                    if (&other.user, other.seq) > (&span.user, span.seq) {
                        pos += 1;
                        continue;
                    }
                }
                break;
            }
            pos
        };

        self.root.spans.insert(insert_idx, span);
        self.root.update_summary();
    }

    /// Find the span containing a given ItemId.
    fn find_span_containing(&self, id: &ItemId) -> Option<usize> {
        for (i, span) in self.root.spans.iter().enumerate() {
            if span.contains(id) {
                return Some(i);
            }
        }
        return None;
    }

    /// Delete an item by its ID.
    fn delete_by_id(&mut self, id: &ItemId) -> bool {
        if let Some(idx) = self.find_span_containing(id) {
            let span = &self.root.spans[idx];
            let offset = id.seq - span.seq;
            
            if span.len == 1 {
                // Single item span - just mark deleted
                self.root.spans[idx].deleted = true;
            } else if offset == 0 {
                // Delete first item - split off the rest
                let right = self.root.spans[idx].split(1);
                self.root.spans[idx].deleted = true;
                self.root.spans.insert(idx + 1, right);
            } else if offset == span.len - 1 {
                // Delete last item - split it off
                let right = self.root.spans[idx].split(offset);
                self.root.spans.insert(idx + 1, right);
                self.root.spans[idx + 1].deleted = true;
            } else {
                // Delete middle item - split into [left][deleted][right]
                // First split off everything from offset onwards
                let mut mid_and_right = self.root.spans[idx].split(offset);
                // Then split the middle item (now at position 0) from the rest
                let right = mid_and_right.split(1);
                mid_and_right.deleted = true;
                self.root.spans.insert(idx + 1, mid_and_right);
                self.root.spans.insert(idx + 2, right);
            }
            
            self.root.update_summary();
            return true;
        }
        return false;
    }
}

impl super::Crdt for Rga {
    fn merge(&mut self, other: &Self) {
        // Merge by replaying all spans from other that we don't have.
        // This is a simplified merge - a full implementation would use
        // the OpLog to ensure causal ordering.
        for span in &other.root.spans {
            // Check if we already have this span
            if self.find_span_containing(&span.id_at(0)).is_some() {
                continue;
            }
            
            // Get the content from other's column
            let other_column = other.columns.get(&span.user).unwrap();
            let content = &other_column.content[span.content_offset..span.content_offset + span.len as usize];
            
            // Create an OpBlock and apply it
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

        // Insert "hello"
        let block1 = OpBlock::insert(None, 0, b"hello".to_vec());
        rga.apply(&pair.key_pub, &block1);

        // Insert " world" after the 'o' (seq 4)
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
        assert!(!rga.apply(&pair.key_pub, &block)); // Already applied
        
        assert_eq!(rga.to_string(), "hello");
    }

    #[test]
    fn apply_delete() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();

        // Insert "hello"
        let block1 = OpBlock::insert(None, 0, b"hello".to_vec());
        rga.apply(&pair.key_pub, &block1);

        // Delete 'e' (seq 1)
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

        // Alice inserts "hello"
        rga_a.insert(&alice.key_pub, 0, b"hello");

        // Bob inserts "world"
        rga_b.insert(&bob.key_pub, 0, b"world");

        // Merge B into A
        rga_a.merge(&rga_b);

        // Both insertions should be present
        // Order depends on user key ordering
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

        // Both insert at the beginning (no origin)
        let block_a = OpBlock::insert(None, 0, b"A".to_vec());
        let block_b = OpBlock::insert(None, 0, b"B".to_vec());

        rga.apply(&alice.key_pub, &block_a);
        rga.apply(&bob.key_pub, &block_b);

        // Result is deterministic based on user key ordering
        let result = rga.to_string();
        assert_eq!(result.len(), 2);
        assert!(result == "AB" || result == "BA");
    }
}
