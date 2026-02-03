// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-02-01"
// driver = "Isaac Clayton"

//! Replicated Growable Array (RGA) - a sequence CRDT for collaborative text editing.
//!
//! RGA maintains a total order over all inserted characters, even when edits happen
//! concurrently on different replicas. Each character gets a unique ID (user, seq),
//! and the ordering algorithm ensures all replicas converge to the same document.
//!
//! # Key concepts
//!
//! - **Span**: A contiguous run of characters from the same user. Spans are the unit
//!   of storage; individual characters are not stored separately.
//! - **Origin**: Each span knows which character it was inserted after. This enables
//!   deterministic ordering of concurrent insertions at the same position.
//! - **Tombstone deletion**: Deleted characters are marked as deleted but not removed,
//!   preserving the total order for future merges.
//!
//! # Example
//!
//! ```
//! use together::crdt::rga::RgaBuf;
//! use together::key::KeyPair;
//!
//! let user = KeyPair::generate();
//! let mut doc = RgaBuf::new();
//!
//! doc.insert(&user.key_pub, 0, b"Hello");
//! doc.insert(&user.key_pub, 5, b" World");
//! assert_eq!(doc.to_string(), "Hello World");
//!
//! doc.delete(5, 6); // Delete " World"
//! assert_eq!(doc.to_string(), "Hello");
//! ```

use std::sync::Arc;

use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::key::KeyPub;
use super::btree_list::BTreeList;

/// Sentinel value for spans with no origin (i.e., inserted at the document beginning).
/// We use u16::MAX because valid user indices start at 0 and grow upward.
const NO_ORIGIN_USER: u16 = u16::MAX;

/// A table mapping u16 indices to KeyPub values.
/// This allows spans to store a 2-byte index instead of a 32-byte key.
#[derive(Clone, Debug, Default)]
struct UserTable {
    /// Map from KeyPub to index.
    key_to_idx: FxHashMap<KeyPub, u16>,
    /// Map from index to KeyPub.
    idx_to_key: Vec<KeyPub>,
}

impl UserTable {
    fn new() -> UserTable {
        return UserTable {
            key_to_idx: FxHashMap::default(),
            idx_to_key: Vec::new(),
        };
    }

    fn get_or_insert(&mut self, key: &KeyPub) -> u16 {
        if let Some(&idx) = self.key_to_idx.get(key) {
            return idx;
        }
        let idx = self.idx_to_key.len() as u16;
        assert!(idx < u16::MAX, "too many users (max 65534)");
        self.idx_to_key.push(*key);
        self.key_to_idx.insert(*key, idx);
        return idx;
    }

    fn get(&self, key: &KeyPub) -> Option<u16> {
        return self.key_to_idx.get(key).copied();
    }

    fn get_key(&self, idx: u16) -> Option<&KeyPub> {
        return self.idx_to_key.get(idx as usize);
    }
}

/// A unique identifier for an item (character) in the RGA.
///
/// Each character inserted into the document gets a unique ItemId based on
/// who inserted it (user) and when in their personal sequence (seq).
/// This ID is stable across all replicas and survives merges.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct ItemId {
    user: KeyPub,
    seq: u64,
}

impl std::fmt::Debug for ItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return write!(f, "ItemId({:?}, {})", self.user, self.seq);
    }
}

// =============================================================================
// Public types for document API
// =============================================================================

/// Whether an anchor stays before or after its target character
/// when text is inserted exactly at the anchor position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorBias {
    /// Anchor stays before the character (insertion at anchor pushes anchor right).
    Before,
    /// Anchor stays after the character (insertion at anchor keeps anchor in place).
    After,
}

/// A position in the document that tracks a specific character.
///
/// Anchors move with edits: if text is inserted before the anchor,
/// the anchor's resolved position increases. If the anchored character
/// is deleted, the anchor resolves to None.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    /// User index of the anchored character.
    user_idx: u16,
    /// Sequence number of the anchored character.
    seq: u32,
    /// Bias for insertion at anchor position.
    bias: AnchorBias,
}

/// A range defined by two anchors.
///
/// The start anchor has After bias (range expands when inserting at start).
/// The end anchor has Before bias (range expands when inserting at end).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorRange {
    /// Start anchor (After bias - stays after its character).
    pub start: Anchor,
    /// End anchor (Before bias - stays before its character).
    pub end: Anchor,
}

/// A snapshot of document state at a point in time.
///
/// Uses Arc for cheap cloning and structural sharing.
#[derive(Clone, Debug)]
struct Snapshot {
    /// The spans at this version.
    spans: Vec<Span>,
    /// Cached length (sum of visible lengths).
    len: u64,
}

/// A version identifier for accessing historical document states.
///
/// For the persistent approach, this holds a reference-counted snapshot
/// of the document state, enabling O(1) access to historical versions.
#[derive(Clone, Debug)]
pub struct Version {
    /// The snapshot at this version.
    snapshot: Arc<Snapshot>,
    /// Lamport timestamp for ordering.
    lamport: u64,
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.lamport == other.lamport
    }
}

impl Eq for Version {}

/// A compact reference to an origin item by its unique ID.
/// 
/// The origin is the character that a span was inserted immediately after.
/// For example, if you type "world" after "hello", the origin of "world"
/// is the 'o' in "hello".
///
/// Uses (user_idx, seq) which is stable across document modifications.
/// This enables the origin index optimization: we can map from origin ID
/// to the list of spans that share that origin (siblings inserted at
/// the same position concurrently).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct OriginId {
    /// User index of the origin character (NO_ORIGIN_USER if no origin).
    user_idx: u16,
    /// Sequence number of the origin character.
    seq: u32,
}

impl OriginId {
    fn none() -> OriginId {
        return OriginId {
            user_idx: NO_ORIGIN_USER,
            seq: 0,
        };
    }

    fn some(user_idx: u16, seq: u32) -> OriginId {
        return OriginId { user_idx, seq };
    }
    
    /// Convert to a key for the origin index.
    fn as_key(&self) -> (u16, u32) {
        return (self.user_idx, self.seq);
    }
}

/// A compact span of consecutive characters inserted by the same user.
/// 
/// Spans are the fundamental unit of storage in RGA. Rather than storing
/// each character individually (which would be expensive), we group
/// consecutive characters from the same user into spans.
///
/// A span can be split when:
/// - A concurrent insert lands in the middle of the span
/// - A delete operation targets part of the span
///
/// Origin is stored as (user_idx, seq) which is stable across modifications.
/// The origin identifies which character this span was inserted after.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct Span {
    /// Starting sequence number for this span's characters.
    seq: u32,
    /// Number of characters in this span.
    len: u32,
    /// Origin user index (NO_ORIGIN_USER if inserted at document beginning).
    origin_user_idx: u16,
    /// Origin sequence number (the specific character we inserted after).
    origin_seq: u32,
    /// Offset into the user's content column where this span's bytes start.
    content_offset: u32,
    /// Index of the user who created this span.
    user_idx: u16,
    /// Whether this span has been deleted (tombstone).
    deleted: bool,
    /// Padding for alignment (not used).
    _padding: [u8; 1],
}

impl Span {
    fn new(
        user_idx: u16,
        seq: u32,
        len: u32,
        origin: OriginId,
        content_offset: u32,
    ) -> Span {
        return Span {
            seq,
            len,
            origin_user_idx: origin.user_idx,
            origin_seq: origin.seq,
            content_offset,
            user_idx,
            deleted: false,
            _padding: [0; 1],
        };
    }

    fn origin(&self) -> OriginId {
        return OriginId {
            user_idx: self.origin_user_idx,
            seq: self.origin_seq,
        };
    }

    fn set_origin(&mut self, origin: OriginId) {
        self.origin_user_idx = origin.user_idx;
        self.origin_seq = origin.seq;
    }

    #[inline(always)]
    fn has_origin(&self) -> bool {
        return self.origin_user_idx != NO_ORIGIN_USER;
    }

    /// Returns true if this span is a "split continuation" - the right part of
    /// a span that was split. Such spans have their origin pointing to the last
    /// character of the left part, and are NOT true siblings for RGA ordering.
    ///
    /// A split continuation has: same user as origin, seq = origin_seq + 1
    #[inline(always)]
    fn is_split_continuation(&self) -> bool {
        return self.user_idx == self.origin_user_idx && self.seq == self.origin_seq + 1;
    }

    #[inline(always)]
    fn contains_seq(&self, seq: u32) -> bool {
        return seq >= self.seq && seq < self.seq + self.len;
    }

    /// Split this span at the given offset, returning the right part.
    ///
    /// After splitting "hello" at offset 2:
    /// - Left span: "he" (seq 0..2)
    /// - Right span: "llo" (seq 2..5), origin points to 'e' (the last char of left)
    ///
    /// The right part's origin is automatically set to the last character of the
    /// left part, maintaining the invariant that each span knows what it follows.
    ///
    /// NOTE: This method is for INSERTION splits. For deletion splits, use
    /// `split_preserving_origin` instead to maintain correct RGA ordering.
    #[inline]
    fn split(&mut self, offset: u32) -> Span {
        debug_assert!(offset > 0 && offset < self.len);
        // The right part's origin is the character immediately before the split point.
        // This maintains the RGA invariant: we know exactly what each span follows.
        let right = Span {
            seq: self.seq + offset,
            len: self.len - offset,
            origin_user_idx: self.user_idx,
            origin_seq: self.seq + offset - 1,
            content_offset: self.content_offset + offset,
            user_idx: self.user_idx,
            deleted: self.deleted,
            _padding: [0; 1],
        };
        self.len = offset;
        return right;
    }
    
    /// Split this span for deletion, preserving the original origin.
    ///
    /// Unlike `split()`, this method keeps the right part's origin the same as
    /// the original span. This is critical for deletion: when we delete the
    /// prefix of a span, the remaining suffix should keep its original origin
    /// so that RGA ordering remains correct during merges.
    ///
    /// Example: If "ABCDEF" was inserted at the beginning (no origin), and we
    /// delete "AB", the remaining "CDEF" should still have no origin - it was
    /// still inserted at the beginning of the document.
    #[inline]
    fn split_preserving_origin(&mut self, offset: u32) -> Span {
        debug_assert!(offset > 0 && offset < self.len);
        let right = Span {
            seq: self.seq + offset,
            len: self.len - offset,
            origin_user_idx: self.origin_user_idx,  // Keep original origin
            origin_seq: self.origin_seq,            // Keep original origin
            content_offset: self.content_offset + offset,
            user_idx: self.user_idx,
            deleted: self.deleted,
            _padding: [0; 1],
        };
        self.len = offset;
        return right;
    }

    #[inline(always)]
    fn visible_len(&self) -> u32 {
        if self.deleted { 0 } else { self.len }
    }
}

/// Per-user column storing content bytes.
///
/// Each user's inserted content is stored in their own column as a contiguous
/// byte array. Spans reference into their user's column via content_offset.
/// The column is append-only: new inserts add bytes to the end.
#[derive(Clone, Debug)]
struct Column {
    /// The content bytes for this user's insertions.
    content: Vec<u8>,
    /// Next sequence number to assign for new inserts by this user.
    next_seq: u32,
}

impl Column {
    fn new() -> Column {
        return Column {
            content: Vec::new(),
            next_seq: 0,
        };
    }
}

/// Cursor cache for amortizing sequential lookups.
///
/// Text editing exhibits strong locality: when typing "hello", inserts happen
/// at positions 0, 1, 2, 3, 4 in sequence. Without caching, each insert would
/// require an O(log n) tree traversal to find the insertion point.
///
/// By caching the last lookup result, sequential typing becomes O(1) amortized:
/// - Cache hit: use cached position directly
/// - One position forward: scan from cached position (usually same span)
/// - Cache miss: fall back to full O(log n) lookup, then cache result
///
/// The cache also stores B-tree chunk location to avoid repeated tree traversals.
#[derive(Clone, Debug)]
struct CursorCache {
    /// The visible position of the last lookup (position of the character we found).
    /// For an insert at pos P, we look up pos P-1 to find the origin character.
    visible_pos: u64,
    /// Index of the span containing the cached position.
    span_idx: usize,
    /// Offset within the span where the cached position falls.
    offset_in_span: u64,
    /// B-tree chunk index containing the span (optimization to skip tree traversal).
    chunk_idx: usize,
    /// Index of the span within its B-tree chunk.
    idx_in_chunk: usize,
    /// Whether the cache contains valid data.
    valid: bool,
}

impl CursorCache {
    fn new() -> CursorCache {
        return CursorCache {
            visible_pos: 0,
            span_idx: 0,
            offset_in_span: 0,
            chunk_idx: 0,
            idx_in_chunk: 0,
            valid: false,
        };
    }

    /// Invalidate the cache.
    fn invalidate(&mut self) {
        self.valid = false;
    }

    /// Update the cache with a new lookup result.
    fn update(&mut self, visible_pos: u64, span_idx: usize, offset_in_span: u64, chunk_idx: usize, idx_in_chunk: usize) {
        self.visible_pos = visible_pos;
        self.span_idx = span_idx;
        self.offset_in_span = offset_in_span;
        self.chunk_idx = chunk_idx;
        self.idx_in_chunk = idx_in_chunk;
        self.valid = true;
    }

    /// Invalidate the cache after a delete operation.
    /// 
    /// Deletions can cause span splits which change span indices unpredictably.
    /// Rather than tracking index shifts precisely (which would be error-prone),
    /// we conservatively invalidate the cache when a delete might affect it.
    ///
    /// The cache remains valid only if the delete is entirely after the cached position.
    fn adjust_after_delete(&mut self, delete_pos: u64) {
        if !self.valid {
            return;
        }
        // If the delete starts after our cached position, it cannot affect
        // the span containing our cached character.
        if delete_pos > self.visible_pos {
            return;
        }
        self.invalidate();
    }
}

/// A Replicated Growable Array.
///
/// The core data structure for collaborative text editing. Stores document
/// content as a list of spans, where each span's weight is its visible
/// character count. This enables O(log n) position-to-span lookup.
///
/// # Architecture
///
/// - **Spans**: Stored in a B-tree weighted by visible length. Deleted spans
///   have weight 0, so they are skipped during position lookups.
/// - **Columns**: Each user has a column storing their inserted content bytes.
///   Spans reference into their user's column via content_offset.
/// - **Origin index**: Maps origin IDs to spans that share that origin,
///   enabling efficient sibling lookup during concurrent insert resolution.
#[derive(Clone)]
pub struct Rga {
    /// Spans in document order, weighted by visible character count.
    /// The B-tree enables O(log n) lookup by visible position.
    spans: BTreeList<Span>,
    /// Per-user columns storing content bytes, indexed by user_idx.
    /// Spans reference their content via (user_idx, content_offset, len).
    columns: Vec<Column>,
    /// Bidirectional mapping between KeyPub and compact user indices.
    users: UserTable,
    /// Cache for amortizing sequential typing lookups.
    cursor_cache: CursorCache,
    /// Lamport timestamp, incremented on each local operation.
    lamport: u64,
    /// Index from origin ID to list of span indices sharing that origin.
    /// Used to efficiently find siblings during concurrent insert resolution.
    /// Key is (user_idx, seq) of the origin character.
    origin_index: FxHashMap<(u16, u32), SmallVec<[usize; 4]>>,
}

impl Rga {
    /// Create a new empty RGA.
    pub fn new() -> Rga {
        return Rga {
            spans: BTreeList::new(),
            columns: Vec::new(),
            users: UserTable::new(),
            cursor_cache: CursorCache::new(),
            lamport: 0,
            origin_index: FxHashMap::default(),
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

    /// Debug: dump all spans with their weights and content.
    #[allow(dead_code)]
    pub fn debug_dump_users(&self) {
        eprintln!("=== User Table ({} users) ===", self.users.idx_to_key.len());
        for (idx, key) in self.users.idx_to_key.iter().enumerate() {
            eprintln!("  user_idx={} -> {:02x}{:02x}...", idx, key.0[0], key.0[1]);
        }
        eprintln!("=== End User Table ===");
    }
    
    pub fn debug_dump_spans(&self) {
        eprintln!("=== RGA Span Dump ({} spans, len={}) ===", self.spans.len(), self.spans.total_weight());
        let mut visible_pos = 0u64;
        for i in 0..self.spans.len() {
            let span = self.spans.get(i).unwrap();
            let user_key = self.users.get_key(span.user_idx).unwrap();
            let user_short = format!("{:02x}{:02x}", user_key.0[0], user_key.0[1]);
            let col = &self.columns[span.user_idx as usize];
            let start = span.content_offset as usize;
            let end = start + span.len as usize;
            let content = &col.content[start..end];
            let content_str = String::from_utf8_lossy(content);
            let origin_str = if span.has_origin() {
                let origin = span.origin();
                format!("(u{},s{})", origin.user_idx, origin.seq)
            } else {
                "none".to_string()
            };
            let del = if span.deleted { "D" } else { "" };
            eprintln!(
                "  [{}] u={} seq={} len={}{} origin={} @{} {:?}",
                i, user_short, span.seq, span.len, del, origin_str, visible_pos, content_str
            );
            visible_pos += span.visible_len() as u64;
        }
        eprintln!("=== End Dump ===");
    }

    /// Get or create a user index, ensuring the column exists.
    fn ensure_user(&mut self, user: &KeyPub) -> u16 {
        let idx = self.users.get_or_insert(user);
        while self.columns.len() <= idx as usize {
            self.columns.push(Column::new());
        }
        return idx;
    }

    /// Insert content at the given visible position.
    pub fn insert(&mut self, user: &KeyPub, pos: u64, content: &[u8]) {
        if content.is_empty() {
            panic!("cannot insert empty content");
        }
        let user_idx = self.ensure_user(user);
        self.insert_with_user_idx(user_idx, pos, content);
    }

    /// Insert content using a pre-computed user index.
    #[inline]
    fn insert_with_user_idx(&mut self, user_idx: u16, pos: u64, content: &[u8]) {
        self.lamport += 1;

        let column = &mut self.columns[user_idx as usize];
        let seq = column.next_seq;
        let content_offset = column.content.len() as u32;
        column.content.extend_from_slice(content);
        column.next_seq += content.len() as u32;

        let span = Span::new(
            user_idx,
            seq,
            content.len() as u32,
            OriginId::none(),
            content_offset,
        );

        self.insert_span_at_pos_optimized(span, pos);
    }

    /// Delete a range of visible characters starting at `start`.
    ///
    /// RGA uses tombstone deletion: characters are marked as deleted but not
    /// removed from the span list. This preserves the total order for merging
    /// with other replicas. Deleted spans have weight 0, so they are skipped
    /// during position lookups and iteration.
    ///
    /// A delete may need to split spans if the deletion range doesn't align
    /// with span boundaries. Four cases are handled:
    /// - Delete entire span: just mark as deleted
    /// - Delete prefix: split, delete left part
    /// - Delete suffix: split, delete right part
    /// - Delete middle: split twice, delete middle part
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

        self.lamport += 1;
        self.cursor_cache.adjust_after_delete(start);

        let mut remaining = len;

        while remaining > 0 {
            let (span_idx, offset_in_span) = match self.spans.find_by_weight(start) {
                Some((idx, off)) => (idx, off),
                None => panic!("position {} not found", start),
            };

            let span = self.spans.get(span_idx).unwrap();
            let span_visible = span.visible_len() as u64;

            if offset_in_span == 0 && remaining >= span_visible {
                // Case 1: Delete covers entire span - just mark as deleted
                self.spans.get_mut(span_idx).unwrap().deleted = true;
                self.spans.update_weight(span_idx, 0);
                remaining -= span_visible;
            } else if offset_in_span == 0 {
                // Case 2: Delete prefix of span - split and delete left part
                // Use split() so the surviving right part's origin points to
                // the last char of the deleted left part (the tombstone).
                // This ensures correct RGA ordering: the right part follows
                // the tombstone, not competing with other no-origin spans.
                let mut span = self.spans.remove(span_idx);
                let right = span.split(remaining as u32);
                span.deleted = true;
                self.spans.insert(span_idx, span, 0);
                self.spans.insert(span_idx + 1, right, right.visible_len() as u64);
                remaining = 0;
            } else if offset_in_span + remaining >= span_visible {
                // Case 3: Delete suffix of span - split and delete right part
                // Use split() so the tombstone's origin points to the last char
                // of the surviving left part. This ensures correct positioning
                // when the tombstone is merged into other replicas.
                let to_delete = span_visible - offset_in_span;
                let mut span = self.spans.remove(span_idx);
                let mut right = span.split(offset_in_span as u32);
                right.deleted = true;
                self.spans.insert(span_idx, span, span.visible_len() as u64);
                self.spans.insert(span_idx + 1, right, 0);
                remaining -= to_delete;
            } else {
                // Case 4: Delete middle of span - split twice, delete middle
                // The left span keeps its original origin.
                // The right span needs origin pointing to the last char of the
                // deleted middle part (tombstone), so it maintains correct RGA
                // ordering during merges.
                // We achieve this by using split() for both splits:
                // 1. First split: mid_right gets origin -> last char of span
                // 2. Second split: right gets origin -> last char of mid_right (tombstone)
                let mut span = self.spans.remove(span_idx);
                let mut mid_right = span.split(offset_in_span as u32);
                let right = mid_right.split(remaining as u32);
                mid_right.deleted = true;
                self.spans.insert(span_idx, span, span.visible_len() as u64);
                self.spans.insert(span_idx + 1, mid_right, 0);
                self.spans.insert(span_idx + 2, right, right.visible_len() as u64);
                remaining = 0;
            }
        }
    }

    /// Get the content as a string (assumes UTF-8).
    pub fn to_string(&self) -> String {
        let mut result = Vec::new();
        for span in self.spans.iter() {
            if !span.deleted {
                let column = &self.columns[span.user_idx as usize];
                let start = span.content_offset as usize;
                let end = start + span.len as usize;
                result.extend_from_slice(&column.content[start..end]);
            }
        }
        return String::from_utf8(result).unwrap_or_default();
    }

    /// Read characters in the range [start, end) without allocating the full document.
    ///
    /// Returns None if the range is out of bounds.
    ///
    /// # Example
    /// ```
    /// use together::crdt::rga::Rga;
    /// use together::key::KeyPair;
    ///
    /// let user = KeyPair::generate();
    /// let mut rga = Rga::new();
    /// rga.insert(&user.key_pub, 0, b"hello world");
    ///
    /// assert_eq!(rga.slice(0, 5), Some("hello".to_string()));
    /// assert_eq!(rga.slice(6, 11), Some("world".to_string()));
    /// ```
    pub fn slice(&self, start: u64, end: u64) -> Option<String> {
        if start > end {
            return None;
        }
        if end > self.len() {
            return None;
        }
        if start == end {
            return Some(String::new());
        }

        let mut result = Vec::with_capacity((end - start) as usize);
        let mut pos: u64 = 0;

        for span in self.spans.iter() {
            if span.deleted {
                continue;
            }

            let span_len = span.len as u64;
            let span_end = pos + span_len;

            // Skip spans entirely before our range
            if span_end <= start {
                pos = span_end;
                continue;
            }

            // Stop if we've passed the end
            if pos >= end {
                break;
            }

            // Calculate overlap
            let overlap_start = start.max(pos);
            let overlap_end = end.min(span_end);
            let offset_in_span = (overlap_start - pos) as usize;
            let len_to_copy = (overlap_end - overlap_start) as usize;

            let column = &self.columns[span.user_idx as usize];
            let content_start = span.content_offset as usize + offset_in_span;
            let content_end = content_start + len_to_copy;
            result.extend_from_slice(&column.content[content_start..content_end]);

            pos = span_end;
        }

        return Some(String::from_utf8(result).unwrap_or_default());
    }

    /// Create an anchor at the given visible position.
    ///
    /// Returns None if position is out of bounds.
    pub fn anchor_at(&self, pos: u64, bias: AnchorBias) -> Option<Anchor> {
        if pos >= self.len() {
            return None;
        }

        // Find the span containing this position
        let (span_idx, offset_in_span) = self.spans.find_by_weight(pos)?;
        let span = self.spans.get(span_idx)?;

        // The anchor refers to the character at pos
        let seq = span.seq + offset_in_span as u32;

        return Some(Anchor {
            user_idx: span.user_idx,
            seq,
            bias,
        });
    }

    /// Resolve an anchor to its current visible position.
    ///
    /// Returns None if the anchored character has been deleted.
    pub fn resolve_anchor(&self, anchor: &Anchor) -> Option<u64> {
        // Find the span containing this anchor's character
        let mut pos: u64 = 0;

        for span in self.spans.iter() {
            if span.user_idx == anchor.user_idx && span.contains_seq(anchor.seq) {
                // Found the span containing our character
                if span.deleted {
                    return None;
                }
                let offset = anchor.seq - span.seq;
                return Some(pos + offset as u64);
            }

            if !span.deleted {
                pos += span.len as u64;
            }
        }

        return None;
    }

    /// Create an anchor range for [start, end).
    ///
    /// The start anchor has After bias (expands when inserting at start).
    /// The end anchor has Before bias (expands when inserting at end).
    ///
    /// Returns None if either position is out of bounds.
    pub fn anchor_range(&self, start: u64, end: u64) -> Option<AnchorRange> {
        if start > end || end > self.len() {
            return None;
        }

        // Handle empty range at end of document
        if start == end {
            if start == 0 {
                return None; // Can't create empty range at start of empty doc
            }
            // Create anchors at the same position
            let anchor = self.anchor_at(start.saturating_sub(1), AnchorBias::After)?;
            return Some(AnchorRange {
                start: anchor.clone(),
                end: anchor,
            });
        }

        // Start anchor: points to first char of range with After bias
        // This means if we insert AT start position, the inserted text
        // appears BEFORE the anchored char, so the range expands to include it
        let start_anchor = self.anchor_at(start, AnchorBias::After)?;

        // End anchor: points to last char IN the range (position end-1) with Before bias
        // This means if we insert AT end position, the inserted text
        // appears AFTER the anchored char, so the range expands to include it
        let end_anchor = self.anchor_at(end - 1, AnchorBias::Before)?;

        return Some(AnchorRange {
            start: start_anchor,
            end: end_anchor,
        });
    }

    /// Get the current slice for an anchor range.
    ///
    /// Returns None if either anchor's character has been deleted.
    pub fn slice_anchored(&self, range: &AnchorRange) -> Option<String> {
        let start = self.resolve_anchor(&range.start)?;
        let end_char_pos = self.resolve_anchor(&range.end)?;

        // The end anchor points to the last character IN the range
        // So the slice end is end_char_pos + 1
        let end = end_char_pos + 1;

        if start > end {
            return Some(String::new());
        }

        return self.slice(start, end);
    }

    /// Get the current version.
    ///
    /// The version can be used with `to_string_at`, `slice_at`, and `len_at`
    /// to access historical document states.
    ///
    /// This creates a snapshot of the current document state. The snapshot
    /// is reference-counted, so taking multiple versions is cheap if the
    /// document hasn't changed.
    pub fn version(&self) -> Version {
        // Create a snapshot of current spans
        let spans: Vec<Span> = self.spans.iter().cloned().collect();
        let len = self.len();
        
        return Version {
            snapshot: Arc::new(Snapshot { spans, len }),
            lamport: self.lamport,
        };
    }

    /// Get the full document at a specific version.
    ///
    /// Uses the snapshot stored in the version for O(n) reconstruction
    /// where n is the document length at that version.
    pub fn to_string_at(&self, version: &Version) -> String {
        let snapshot = &version.snapshot;
        let mut result = Vec::with_capacity(snapshot.len as usize);
        
        for span in &snapshot.spans {
            if !span.deleted {
                let user_idx = span.user_idx as usize;
                if user_idx < self.columns.len() {
                    let col = &self.columns[user_idx];
                    let start = span.content_offset as usize;
                    let end = start + span.len as usize;
                    if end <= col.content.len() {
                        result.extend_from_slice(&col.content[start..end]);
                    }
                }
            }
        }
        
        return String::from_utf8_lossy(&result).into_owned();
    }

    /// Read a slice at a specific version.
    ///
    /// Returns characters in range [start, end) from the snapshot.
    pub fn slice_at(&self, start: u64, end: u64, version: &Version) -> Option<String> {
        let snapshot = &version.snapshot;
        
        if start > end || start > snapshot.len {
            return None;
        }
        
        let end = end.min(snapshot.len);
        if start == end {
            return Some(String::new());
        }
        
        let mut result = Vec::with_capacity((end - start) as usize);
        let mut pos: u64 = 0;
        
        for span in &snapshot.spans {
            if span.deleted {
                continue;
            }
            
            let span_len = span.len as u64;
            let span_end = pos + span_len;
            
            // Check if this span overlaps with our range
            if span_end > start && pos < end {
                let user_idx = span.user_idx as usize;
                if user_idx < self.columns.len() {
                    let col = &self.columns[user_idx];
                    
                    // Calculate the portion of this span to include
                    let span_start_in_range = if pos < start { (start - pos) as u32 } else { 0 };
                    let span_end_in_range = if span_end > end { span.len - (span_end - end) as u32 } else { span.len };
                    
                    let content_start = (span.content_offset + span_start_in_range) as usize;
                    let content_end = (span.content_offset + span_end_in_range) as usize;
                    
                    if content_end <= col.content.len() {
                        result.extend_from_slice(&col.content[content_start..content_end]);
                    }
                }
            }
            
            pos = span_end;
            if pos >= end {
                break;
            }
        }
        
        return Some(String::from_utf8_lossy(&result).into_owned());
    }

    /// Get the document length at a specific version.
    ///
    /// Returns the cached length from the snapshot (O(1)).
    pub fn len_at(&self, version: &Version) -> u64 {
        return version.snapshot.len;
    }

    /// Insert a span at the given visible position for local edits.
    /// 
    /// This is the hot path for local typing. It attempts two optimizations:
    ///
    /// 1. **Cursor caching**: If the insert position is at or near the cached
    ///    position, we skip the O(log n) tree lookup and use the cached location.
    ///
    /// 2. **Span coalescing**: If the new span is contiguous with the previous
    ///    span (same user, consecutive sequence numbers, adjacent content), we
    ///    extend the existing span instead of creating a new one. This keeps
    ///    span count low during sequential typing.
    #[inline]
    fn insert_span_at_pos_optimized(&mut self, mut span: Span, pos: u64) {
        let span_len = span.visible_len() as u64;

        if self.spans.is_empty() {
            self.spans.insert(0, span, span_len);
            self.cursor_cache.update(pos + span_len - 1, 0, span_len - 1, 0, 0);
            return;
        }

        if pos == 0 {
            // Inserting at document beginning in a LOCAL edit.
            //
            // We must use RGA ordering among no-origin spans to ensure convergence.
            // If we just prepend, the local order won't match what merge produces,
            // causing divergence when this span is merged into other replicas.
            //
            // Find the correct position using RGA sibling ordering: higher (user, seq)
            // has priority and goes first. Skip spans that have origins (they are
            // children of other spans).
            let span_user = self.users.get_key(span.user_idx).unwrap();
            let mut insert_idx = 0;
            while insert_idx < self.spans.len() {
                let other = self.spans.get(insert_idx).unwrap();
                // Skip spans that have an origin - they are children of some
                // other span and don't participate in no-origin ordering.
                if other.has_origin() {
                    insert_idx += 1;
                    continue;
                }
                // This is a no-origin span - compare for RGA ordering
                let other_user = self.users.get_key(other.user_idx).unwrap();
                if (other_user, other.seq) > (span_user, span.seq) {
                    // Other span has higher priority - we go after it and its descendants
                    insert_idx += 1;
                } else {
                    // We have higher priority - insert here
                    break;
                }
            }
            self.spans.insert(insert_idx, span, span_len);
            self.cursor_cache.invalidate();
            return;
        }

        let lookup_pos = pos - 1;

        // Try to use the cursor cache for sequential typing.
        // Three cases:
        // 1. Exact cache hit: lookup_pos matches cached position
        // 2. One position forward: common during sequential typing
        // 3. Cache miss: fall back to full O(log n) tree lookup
        let (prev_idx, offset_in_prev, chunk_idx, idx_in_chunk) = if self.cursor_cache.valid
            && self.cursor_cache.visible_pos == lookup_pos
        {
            // Case 1: Exact cache hit - use cached values directly
            (
                self.cursor_cache.span_idx,
                self.cursor_cache.offset_in_span,
                self.cursor_cache.chunk_idx,
                self.cursor_cache.idx_in_chunk,
            )
        } else if self.cursor_cache.valid
            && self.cursor_cache.visible_pos + 1 == lookup_pos
            && self.cursor_cache.span_idx < self.spans.len()
        {
            // Case 2: One position forward from cache (sequential typing)
            let cached_span = self.spans.get_with_chunk_hint(
                self.cursor_cache.chunk_idx,
                self.cursor_cache.idx_in_chunk,
            ).unwrap();
            let cached_visible = cached_span.visible_len() as u64;
            
            if self.cursor_cache.offset_in_span + 1 < cached_visible {
                // Next position is still within the same span - just increment offset
                (
                    self.cursor_cache.span_idx,
                    self.cursor_cache.offset_in_span + 1,
                    self.cursor_cache.chunk_idx,
                    self.cursor_cache.idx_in_chunk,
                )
            } else {
                // We've reached the end of the cached span.
                // Fall back to full lookup to find the next visible span.
                // This happens when typing reaches a span boundary.
                match self.spans.find_by_weight_with_chunk(lookup_pos) {
                    Some((span_idx, off, c_idx, i_in_c)) => (span_idx, off, c_idx, i_in_c),
                    None => {
                        self.spans.insert(self.spans.len(), span, span_len);
                        self.cursor_cache.invalidate();
                        return;
                    }
                }
            }
        } else {
            // Case 3: Cache miss - perform full O(log n) tree lookup
            match self.spans.find_by_weight_with_chunk(lookup_pos) {
                Some((span_idx, off, c_idx, i_in_c)) => (span_idx, off, c_idx, i_in_c),
                None => {
                    // Position not found - append at end
                    self.spans.insert(self.spans.len(), span, span_len);
                    self.cursor_cache.invalidate();
                    return;
                }
            }
        };

        let prev_span = self.spans.get_with_chunk_hint(chunk_idx, idx_in_chunk).unwrap();
        let prev_visible_len = prev_span.visible_len() as u64;
        
        // The origin is the character we're inserting after (at offset_in_prev in prev_span)
        let origin_seq = prev_span.seq + offset_in_prev as u32;
        span.set_origin(OriginId::some(prev_span.user_idx, origin_seq));

        // Try to coalesce with previous span. Coalescing is possible when:
        // - Same user created both spans
        // - Previous span is not deleted
        // - Sequence numbers are contiguous (prev ends where new begins)
        // - Content bytes are contiguous in the column
        // - We're inserting at the end of the previous span
        let can_coalesce = prev_span.user_idx == span.user_idx
            && !prev_span.deleted
            && prev_span.seq + prev_span.len == span.seq
            && prev_span.content_offset + prev_span.len == span.content_offset
            && offset_in_prev == prev_visible_len - 1;

        if can_coalesce {
            // Extend the previous span instead of creating a new one.
            // This keeps span count low during sequential typing.
            let add_len = span.len;
            let (new_weight, new_chunk_idx, new_idx_in_chunk) = self.spans.modify_and_update_weight_with_hint(
                chunk_idx,
                idx_in_chunk,
                |prev_span| {
                    prev_span.len += add_len;
                    prev_span.visible_len() as u64
                },
            ).unwrap();
            
            self.cursor_cache.update(
                pos + span_len - 1,
                prev_idx,
                new_weight - 1,
                new_chunk_idx,
                new_idx_in_chunk,
            );
            return;
        }

        // Cannot coalesce - must insert as a new span.
        // We need to respect RGA sibling ordering: if there are other spans
        // with the same origin that have higher (user, seq), we insert after them.
        let origin_user_idx = prev_span.user_idx;
        let span_user = self.users.get_key(span.user_idx).unwrap().clone();
        
        // First, handle any necessary split
        let (base_insert_idx, _right_split_idx) = if offset_in_prev >= prev_visible_len.saturating_sub(1) {
            // Inserting at the end of prev_span - no split needed
            (prev_idx + 1, None)
        } else {
            // Inserting in the middle of prev_span - must split it first.
            // The right part gets origin pointing to the last char of the left part.
            // We'll update its origin later to point to our new insert.
            let split_offset = (offset_in_prev + 1) as u32;
            let mut existing = self.spans.remove(prev_idx);
            debug_assert!(
                split_offset > 0 && split_offset < existing.len,
                "Invalid split: offset={}, len={}, prev_visible_len={}, offset_in_prev={}",
                split_offset, existing.len, prev_visible_len, offset_in_prev
            );
            let right = existing.split(split_offset);
            self.spans.insert(prev_idx, existing, existing.visible_len() as u64);
            self.spans.insert(prev_idx + 1, right, right.visible_len() as u64);
            // Insert position is after the left part (prev_idx), before the right part
            (prev_idx + 1, Some(prev_idx + 1))
        };
        
        // Determine where to insert among siblings with the same origin.
        // RGA ordering: higher (user, seq) comes first among siblings.
        //
        // IMPORTANT: We skip "split continuations" during sibling ordering.
        // A split continuation is a span created by splitting an existing span,
        // identifiable by: user_idx == origin_user_idx && seq == origin_seq + 1.
        // These are not real concurrent inserts and should stay in their natural
        // position (after the original content they were split from).
        let origin_key = (origin_user_idx, origin_seq);
        let mut insert_idx = base_insert_idx;
        
        // Check for siblings in the origin index
        if let Some(sibling_indices) = self.origin_index.get(&origin_key) {
            for &sibling_idx in sibling_indices {
                if sibling_idx >= self.spans.len() {
                    continue;
                }
                let sibling = self.spans.get(sibling_idx).unwrap();
                if !sibling.has_origin() {
                    continue;
                }
                let sibling_origin = sibling.origin();
                if sibling_origin.user_idx != origin_user_idx || sibling_origin.seq != origin_seq {
                    continue;
                }
                // Skip split continuations - they're not real siblings
                if sibling.is_split_continuation() {
                    continue;
                }
                // RGA ordering: if sibling has higher (user, seq), we insert after it
                let sibling_user = self.users.get_key(sibling.user_idx).unwrap();
                if (sibling_user, sibling.seq) > (&span_user, span.seq) {
                    insert_idx = insert_idx.max(sibling_idx + 1);
                }
            }
        }
        
        // Scan linearly to find all siblings and determine our position.
        // We must scan past lower-priority siblings because there may be
        // higher-priority siblings after them (siblings can be interleaved
        // with their descendants).
        //
        // Key insight: when we find a higher-priority sibling at position P,
        // we need to insert after P AND after all descendants of that sibling.
        // We track whether we're currently "inside" a higher-priority subtree.
        let scan_start = base_insert_idx;
        let mut scan_pos = scan_start;
        let mut in_higher_priority_subtree = false;
        
        while scan_pos < self.spans.len() {
            let other = self.spans.get(scan_pos).unwrap();
            if !other.has_origin() {
                // Hit a root span - stop scanning
                break;
            }
            let other_origin = other.origin();
            let is_sibling = other_origin.user_idx == origin_user_idx 
                && other_origin.seq == origin_seq;
            
            if is_sibling {
                // Skip split continuations - they're not real siblings
                if other.is_split_continuation() {
                    scan_pos += 1;
                    continue;
                }
                let other_user = self.users.get_key(other.user_idx).unwrap();
                if (other_user, other.seq) > (&span_user, span.seq) {
                    // Higher priority sibling - we go after it
                    in_higher_priority_subtree = true;
                    scan_pos += 1;
                    insert_idx = insert_idx.max(scan_pos);
                } else {
                    // Lower priority sibling - we're no longer in a higher-priority subtree
                    in_higher_priority_subtree = false;
                    scan_pos += 1;
                }
            } else {
                // This is a descendant of some sibling
                if in_higher_priority_subtree {
                    // We're in a higher-priority sibling's subtree - stay after it
                    scan_pos += 1;
                    insert_idx = insert_idx.max(scan_pos);
                } else {
                    // We're in a lower-priority sibling's subtree - just skip
                    scan_pos += 1;
                }
            }
        }
        
        self.spans.insert(insert_idx, span, span_len);
        
        // Update origin index for the new span
        self.origin_index
            .entry(origin_key)
            .or_insert_with(SmallVec::new)
            .push(insert_idx);
            
        self.cursor_cache.invalidate();
    }

    /// Find the span containing a character identified by ItemId.
    ///
    /// Uses linear search O(n). This is acceptable because:
    /// - It's only used for remote operations (apply/merge), not local edits
    /// - Remote operations are less frequent than local typing
    /// - A more complex index would add memory overhead
    fn find_span_by_id(&self, id: &ItemId) -> Option<usize> {
        let user_idx = self.users.get(&id.user)?;
        let seq = id.seq as u32;
        for (i, span) in self.spans.iter().enumerate() {
            if span.user_idx == user_idx && span.contains_seq(seq) {
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
            user: id.user,
            seq: id.seq,
        };
    }

    /// Apply an operation from a writer.
    /// Returns true if the operation was applied, false if it was already present.
    pub fn apply(&mut self, user: &KeyPub, block: &OpBlock) -> bool {
        match &block.op {
            Op::Insert { origin, seq, len } => {
                let user_idx = self.ensure_user(user);
                let column = &self.columns[user_idx as usize];
                
                // Check if we already have this insertion
                if (*seq as u32) < column.next_seq {
                    return false;
                }

                // Verify sequence is contiguous
                if (*seq as u32) != column.next_seq {
                    panic!("sequence gap: expected {}, got {}", column.next_seq, seq);
                }

                let column = &mut self.columns[user_idx as usize];
                let content_offset = column.content.len() as u32;
                column.content.extend_from_slice(&block.content);
                column.next_seq += *len as u32;

                // Create the span
                let span = Span::new(
                    user_idx,
                    *seq as u32,
                    *len as u32,
                    OriginId::none(), // Will be resolved during insert
                    content_offset,
                );

                self.insert_span_rga(span, origin.as_ref().map(Self::convert_id));
                return true;
            }
            Op::Delete { target } => {
                let target_id = Self::convert_id(target);
                return self.delete_by_id(&target_id);
            }
        }
    }

    /// Insert a span using RGA ordering rules for remote/merge operations.
    /// 
    /// RGA uses a deterministic ordering to resolve concurrent insertions at
    /// the same position. When multiple users insert after the same origin
    /// character (creating "siblings"), they are ordered by (user, seq) descending.
    /// This ensures all replicas converge to the same document order.
    ///
    /// # Origin Index Optimization
    ///
    /// Finding siblings normally requires O(n) linear scan. The origin_index
    /// maps each origin to spans that share it, enabling O(k) lookup where
    /// k = number of concurrent edits at that position (typically small).
    fn insert_span_rga(&mut self, mut span: Span, origin: Option<ItemId>) {
        let span_len = span.visible_len() as u64;

        if self.spans.is_empty() {
            self.spans.insert(0, span, span_len);
            return;
        }

        let insert_idx = if let Some(ref origin_id) = origin {
            // Convert origin ItemId to stable (user_idx, seq) form
            let origin_user_idx = self.ensure_user(&origin_id.user);
            let origin_seq = origin_id.seq as u32;
            span.set_origin(OriginId::some(origin_user_idx, origin_seq));
            
            if let Some(origin_idx) = self.find_span_by_id(origin_id) {
                let origin_span = self.spans.get(origin_idx).unwrap();
                let offset_in_span = origin_seq - origin_span.seq;

                // If the origin character is in the middle of a span, we must
                // split it so we can insert immediately after the origin.
                // The right part gets origin pointing to the last char of the left part.
                if offset_in_span < origin_span.len - 1 {
                    let mut existing = self.spans.remove(origin_idx);
                    let right = existing.split(offset_in_span + 1);
                    self.spans.insert(origin_idx, existing, existing.visible_len() as u64);
                    self.spans.insert(origin_idx + 1, right, right.visible_len() as u64);
                }

                // Find the correct position among siblings (spans sharing this origin).
                // RGA ordering: higher (user, seq) comes first.
                //
                // IMPORTANT: We skip "split continuations" during sibling ordering.
                // A split continuation is a span created by splitting an existing span,
                // identifiable by: user_idx == origin_user_idx && seq == origin_seq + 1.
                // These are not real concurrent inserts and should stay in their natural
                // position (after the original content they were split from).
                let origin_key = (origin_user_idx, origin_seq);
                let span_user = self.users.get_key(span.user_idx).unwrap();
                let mut insert_pos = origin_idx + 1;
                
                // Use origin index to find siblings in O(k) instead of O(n)
                if let Some(sibling_indices) = self.origin_index.get(&origin_key) {
                    for &sibling_idx in sibling_indices {
                        // Skip stale index entries (can happen after spans are removed)
                        if sibling_idx >= self.spans.len() {
                            continue;
                        }
                        let sibling = self.spans.get(sibling_idx).unwrap();
                        
                        // Verify this is actually a sibling with the same origin
                        if !sibling.has_origin() {
                            continue;
                        }
                        let sibling_origin = sibling.origin();
                        if sibling_origin.user_idx != origin_user_idx || sibling_origin.seq != origin_seq {
                            continue;
                        }
                        
                        // Skip split continuations - they're not real siblings
                        if sibling.is_split_continuation() {
                            continue;
                        }
                        
                        // RGA ordering: if sibling has higher (user, seq), we insert after it
                        let sibling_user = self.users.get_key(sibling.user_idx).unwrap();
                        if (sibling_user, sibling.seq) > (span_user, span.seq) {
                            insert_pos = insert_pos.max(sibling_idx + 1);
                        }
                    }
                }
                
                // Also scan linearly from origin+1 to catch any unindexed siblings.
                // This ensures correctness even if the index is incomplete.
                //
                // IMPORTANT: When we find a sibling with higher priority, we must
                // skip past it AND all its descendants. Descendants are spans with
                // any origin (they're children of some span in the tree). We only
                // stop when we find another sibling (same origin as us) with lower
                // priority, or a span with no origin (another root-level insert).
                let scan_start = origin_idx + 1;
                let mut pos = scan_start;
                let mut in_higher_priority_subtree = false;
                
                while pos < self.spans.len() {
                    let other = self.spans.get(pos).unwrap();
                    
                    // If we hit a span with no origin, stop - it's a root-level insert
                    if !other.has_origin() {
                        break;
                    }
                    
                    let other_origin = other.origin();
                    // Check if this is a sibling (same origin as us).
                    let is_sibling = other_origin.user_idx == origin_user_idx 
                        && other_origin.seq == origin_seq;
                    
                    if is_sibling {
                        // Skip split continuations - they're not real siblings
                        if other.is_split_continuation() {
                            pos += 1;
                            continue;
                        }
                        // This is a sibling - check RGA ordering
                        let other_user = self.users.get_key(other.user_idx).unwrap();
                        if (other_user, other.seq) > (span_user, span.seq) {
                            // Higher priority sibling - we go after it
                            in_higher_priority_subtree = true;
                            pos += 1;
                            insert_pos = insert_pos.max(pos);
                        } else {
                            // Lower priority sibling - we're no longer in a higher-priority subtree
                            // but continue scanning in case there are more higher-priority siblings
                            in_higher_priority_subtree = false;
                            pos += 1;
                        }
                    } else {
                        // This is a descendant of some sibling
                        if in_higher_priority_subtree {
                            // We're in a higher-priority sibling's subtree - stay after it
                            pos += 1;
                            insert_pos = insert_pos.max(pos);
                        } else {
                            // We're in a lower-priority sibling's subtree - just skip
                            pos += 1;
                        }
                    }
                }
                
                insert_pos
            } else {
                // Origin not found - append at end (this shouldn't happen in normal operation)
                self.spans.len()
            }
        } else {
            // No origin means insert at document beginning.
            // Must respect RGA ordering among other no-origin spans.
            //
            // IMPORTANT: Spans with origins are "children" of their origin span.
            // When comparing no-origin spans, we must skip over these children
            // to find all no-origin siblings. The document is linearized as a
            // pre-order traversal: each span is followed by all its descendants.
            //
            // Example: If we have [C (no-origin), XY (origin=C), DE (origin=C)]
            // and we want to insert AB (no-origin, user0 < user2), AB should go
            // AFTER all of C's descendants: [C, XY, DE, AB].
            let span_user = self.users.get_key(span.user_idx).unwrap();
            let mut pos = 0;
            while pos < self.spans.len() {
                let other = self.spans.get(pos).unwrap();
                // Skip spans that have an origin - they are children of some
                // other span and don't participate in no-origin ordering.
                if other.has_origin() {
                    pos += 1;
                    continue;
                }
                // This is a no-origin span - compare for RGA ordering
                let other_user = self.users.get_key(other.user_idx).unwrap();
                if (other_user, other.seq) > (span_user, span.seq) {
                    // Other span has higher priority - we go after it and all
                    // its descendants. Skip to find the next no-origin span.
                    pos += 1;
                } else {
                    // We have higher priority - insert here
                    break;
                }
            }
            pos
        };

        // Insert the span at the determined position
        self.spans.insert(insert_idx, span, span_len);
        
        // Update the origin index for future sibling lookups
        if span.has_origin() {
            let origin_key = span.origin().as_key();
            self.origin_index
                .entry(origin_key)
                .or_insert_with(SmallVec::new)
                .push(insert_idx);
        }
    }
    
    fn delete_by_id(&mut self, id: &ItemId) -> bool {
        let idx = match self.find_span_by_id(id) {
            Some(i) => i,
            None => return false,
        };

        let span = self.spans.get(idx).unwrap();
        if span.deleted {
            return false;
        }

        let offset = (id.seq as u32) - span.seq;
        let span_len = span.len;

        if span_len == 1 {
            self.spans.get_mut(idx).unwrap().deleted = true;
            self.spans.update_weight(idx, 0);
        } else if offset == 0 {
            // Delete first item - use split_preserving_origin so survivor keeps origin
            let mut existing = self.spans.remove(idx);
            let right = existing.split_preserving_origin(1);
            existing.deleted = true;
            self.spans.insert(idx, existing, 0);
            self.spans.insert(idx + 1, right, right.visible_len() as u64);
        } else if offset == span_len - 1 {
            // Delete last item - use split_preserving_origin for consistency
            let mut existing = self.spans.remove(idx);
            let mut right = existing.split_preserving_origin(offset);
            right.deleted = true;
            self.spans.insert(idx, existing, existing.visible_len() as u64);
            self.spans.insert(idx + 1, right, 0);
        } else {
            // Delete middle item - first split sets origin, second preserves it
            let mut existing = self.spans.remove(idx);
            let mut mid_right = existing.split(offset);
            let right = mid_right.split_preserving_origin(1);
            mid_right.deleted = true;
            self.spans.insert(idx, existing, existing.visible_len() as u64);
            self.spans.insert(idx + 1, mid_right, 0);
            self.spans.insert(idx + 2, right, right.visible_len() as u64);
        }

        return true;
    }
}

// --- Buffered wrapper for batching adjacent operations ---

/// A pending insert operation waiting to be flushed to the underlying RGA.
#[derive(Clone, Debug)]
struct PendingInsert {
    /// Index of the user performing the insert.
    user_idx: u16,
    /// Starting visible position for the insert.
    pos: u64,
    /// Accumulated content bytes.
    /// SmallVec avoids heap allocation for small inserts (most are 1-byte).
    /// 32 bytes inline capacity fits typical typing bursts without allocation.
    content: SmallVec<[u8; 32]>,
}

/// A pending delete operation waiting to be flushed to the underlying RGA.
#[derive(Clone, Debug)]
struct PendingDelete {
    /// Starting visible position of the delete range.
    start: u64,
    /// Number of characters to delete.
    len: u64,
}

/// Pending operation type for RgaBuf buffering.
#[derive(Clone, Debug)]
enum PendingOp {
    Insert(PendingInsert),
    Delete(PendingDelete),
}

/// A buffered wrapper around Rga that batches adjacent operations.
///
/// Text editing exhibits strong locality: when typing "hello", inserts happen
/// at positions 0, 1, 2, 3, 4 in sequence. Without buffering, each keystroke
/// would trigger a separate RGA insert operation.
///
/// RgaBuf buffers adjacent operations and applies them as a single batch:
/// - Sequential typing at positions P, P+1, P+2... is buffered into one insert
/// - Backspace at end of pending insert trims the buffer (no RGA operation needed)
/// - Adjacent backspaces (P, P-1, P-2...) are buffered into one delete
/// - Adjacent forward deletes (P, P, P...) are buffered into one delete
///
/// This technique (inspired by JumpRopeBuf in diamond-types) achieves significant
/// speedup for sequential editing patterns by reducing per-keystroke overhead.
///
/// # Usage
///
/// Use `insert` and `delete` as normal. Read operations (`len`, `to_string`,
/// `span_count`) automatically flush pending operations first. The buffer also
/// auto-flushes when switching between insert and delete operations.
pub struct RgaBuf {
    /// The underlying RGA document.
    rga: Rga,
    /// Pending operation waiting to be flushed, if any.
    pending: Option<PendingOp>,
}

impl RgaBuf {
    /// Create a new buffered RGA wrapper.
    pub fn new() -> RgaBuf {
        return RgaBuf {
            rga: Rga::new(),
            pending: None,
        };
    }

    /// Flush any pending operation to the underlying RGA.
    pub fn flush(&mut self) {
        if let Some(pending) = self.pending.take() {
            match pending {
                PendingOp::Insert(ins) => {
                    if !ins.content.is_empty() {
                        self.rga.insert_with_user_idx(ins.user_idx, ins.pos, &ins.content);
                    }
                }
                PendingOp::Delete(del) => {
                    if del.len > 0 {
                        self.rga.delete(del.start, del.len);
                    }
                }
            }
        }
    }

    /// Insert content at the given position.
    ///
    /// If this insert is adjacent to a pending insert by the same user,
    /// the content is buffered. Otherwise, the pending operation is flushed
    /// and a new pending insert is started.
    pub fn insert(&mut self, user: &KeyPub, pos: u64, content: &[u8]) {
        if content.is_empty() {
            return;
        }

        let user_idx = self.rga.ensure_user(user);

        // Check if we can extend a pending insert
        if let Some(PendingOp::Insert(ref mut pending)) = self.pending {
            if pending.user_idx == user_idx
                && pos == pending.pos + pending.content.len() as u64
            {
                // Same user and adjacent position - extend the pending insert
                pending.content.extend_from_slice(content);
                return;
            }
        }

        // Can't extend - flush any pending operation and start a new one
        self.flush();
        self.pending = Some(PendingOp::Insert(PendingInsert {
            user_idx,
            pos,
            content: SmallVec::from_slice(content),
        }));
    }

    /// Delete a range of visible characters.
    ///
    /// Optimized for:
    /// - Backspace at end of pending insert: trim the buffer instead of delete
    /// - Adjacent deletes (backspace pattern): buffer deletes at P, P-1, P-2...
    /// - Adjacent deletes (forward delete): buffer deletes at P, P, P...
    pub fn delete(&mut self, start: u64, len: u64) {
        if len == 0 {
            return;
        }

        // Check if we can trim a pending insert
        if let Some(PendingOp::Insert(ref mut pending)) = self.pending {
            let pending_end = pending.pos + pending.content.len() as u64;

            // Backspace at end of pending insert
            // Example: typed "hello" at pos 0, now delete at pos 4, len 1
            // This deletes 'o' which is still in the buffer
            if start + len == pending_end && start >= pending.pos {
                // The delete is entirely within the pending insert, at the end
                let trim_start = (start - pending.pos) as usize;
                pending.content.truncate(trim_start);
                // If we've trimmed everything, remove the pending op
                if pending.content.is_empty() {
                    self.pending = None;
                }
                return;
            }
        }

        // Check if we can extend a pending delete
        if let Some(PendingOp::Delete(ref mut pending)) = self.pending {
            // Backspace pattern: delete at (pending.start - len)
            // Example: pending is {start: 5, len: 1}, new delete is {start: 4, len: 1}
            // Result should be {start: 4, len: 2}
            if start + len == pending.start {
                pending.start = start;
                pending.len += len;
                return;
            }

            // Forward delete pattern: delete at pending.start (same position)
            // Example: pending is {start: 5, len: 1}, new delete is {start: 5, len: 1}
            // After first delete at 5, the next char moves to 5, so we delete at 5 again
            // Result should be {start: 5, len: 2}
            if start == pending.start {
                pending.len += len;
                return;
            }
        }

        // Can't optimize - flush any pending operation and start new delete
        self.flush();
        self.pending = Some(PendingOp::Delete(PendingDelete { start, len }));
    }

    /// Get the visible length (excluding deleted items).
    ///
    /// Flushes any pending operation first.
    pub fn len(&mut self) -> u64 {
        self.flush();
        return self.rga.len();
    }

    /// Check if the RGA is empty.
    ///
    /// Flushes any pending operation first.
    pub fn is_empty(&mut self) -> bool {
        self.flush();
        return self.rga.is_empty();
    }

    /// Get the content as a string.
    ///
    /// Flushes any pending operation first.
    pub fn to_string(&mut self) -> String {
        self.flush();
        return self.rga.to_string();
    }

    /// Get the number of spans (for profiling).
    ///
    /// Flushes any pending operation first.
    pub fn span_count(&mut self) -> usize {
        self.flush();
        return self.rga.span_count();
    }

    /// Get a reference to the underlying RGA.
    ///
    /// WARNING: Does not flush. Use only when you know there are no pending ops.
    pub fn inner(&self) -> &Rga {
        return &self.rga;
    }

    /// Get a mutable reference to the underlying RGA.
    ///
    /// WARNING: Does not flush. Use only when you know there are no pending ops.
    pub fn inner_mut(&mut self) -> &mut Rga {
        return &mut self.rga;
    }
}

impl Default for RgaBuf {
    fn default() -> Self {
        return Self::new();
    }
}

impl super::Crdt for Rga {
    fn merge(&mut self, other: &Self) {
        // Phase 1: Sync all inserts
        // We must apply spans in sequence order per user, not document order.
        // Collect spans and sort by (user_idx, seq) to ensure contiguous sequences.
        let mut spans_to_apply: Vec<_> = other.spans.iter().collect();
        spans_to_apply.sort_by_key(|s| (s.user_idx, s.seq));
        
        for span in spans_to_apply.iter() {
            // Get the user's KeyPub from other's UserTable
            let other_user = other.users.get_key(span.user_idx).unwrap();
            
            // Check what sequences we already have for this user.
            // A coalesced span might have grown since we last synced, so we
            // can't just check if the first ID exists - we need to apply any
            // new content that extends beyond what we have.
            let self_user_idx = self.users.get(other_user);
            let our_next_seq = if let Some(idx) = self_user_idx {
                self.columns[idx as usize].next_seq
            } else {
                0
            };
            
            // Skip spans we fully have
            let span_end_seq = span.seq + span.len;
            if span_end_seq <= our_next_seq {
                continue;
            }
            
            // We might have partial content. Calculate what's new.
            let new_start_offset = if span.seq < our_next_seq {
                (our_next_seq - span.seq) as usize
            } else {
                0
            };
            let new_seq = span.seq + new_start_offset as u32;
            let _new_len = span.len - new_start_offset as u32;
            
            let other_column = &other.columns[span.user_idx as usize];
            let content = &other_column.content
                [(span.content_offset + new_start_offset as u32) as usize
                 ..(span.content_offset + span.len) as usize];

            // Compute the origin for the new content.
            // If we're applying a partial span (new_start_offset > 0), the origin
            // is the last character we already have (seq = new_seq - 1).
            // Otherwise, use the span's stored origin.
            let origin = if new_start_offset > 0 {
                // Origin is the character just before the new content
                Some(OpItemId {
                    user: *other_user,
                    seq: (new_seq - 1) as u64,
                })
            } else if span.has_origin() {
                let origin_id = span.origin();
                let origin_user = other.users.get_key(origin_id.user_idx).unwrap();
                Some(OpItemId {
                    user: *origin_user,
                    seq: origin_id.seq as u64,
                })
            } else {
                None
            };

            let block = OpBlock::insert(origin, new_seq as u64, content.to_vec());
            self.apply(other_user, &block);
        }
        
        // Phase 2: Sync all deletions
        // For each deleted span in `other`, mark the corresponding characters
        // as deleted in `self`. We must do this after inserts to ensure the
        // spans exist before we try to delete them.
        for span in spans_to_apply.iter() {
            if !span.deleted {
                continue;
            }
            
            let other_user = other.users.get_key(span.user_idx).unwrap();
            
            // Delete each character in this span
            for offset in 0..span.len {
                let target = OpItemId {
                    user: *other_user,
                    seq: (span.seq + offset) as u64,
                };
                let block = OpBlock::delete(target);
                self.apply(other_user, &block);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::KeyPair;

    #[test]
    fn span_size() {
        // Verify our span is compact
        let size = std::mem::size_of::<Span>();
        assert!(size <= 32, "Span is {} bytes, expected <= 32", size);
        // Ideally 24 bytes
        assert_eq!(size, 24, "Span should be exactly 24 bytes");
    }

    #[test]
    fn user_table_basics() {
        let mut table = UserTable::new();
        let key1 = KeyPair::generate().key_pub;
        let key2 = KeyPair::generate().key_pub;

        let idx1 = table.get_or_insert(&key1);
        let idx2 = table.get_or_insert(&key2);
        
        assert_eq!(idx1, 0);
        assert_eq!(idx2, 1);
        
        // Same key returns same index
        assert_eq!(table.get_or_insert(&key1), 0);
        
        // Lookup works
        assert_eq!(table.get(&key1), Some(0));
        assert_eq!(table.get_key(0), Some(&key1));
    }

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
            user: pair.key_pub,
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
            user: pair.key_pub,
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

    #[test]
    fn span_coalescing_sequential_inserts() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        
        // Sequential inserts at end should coalesce into one span
        rga.insert(&pair.key_pub, 0, b"a");
        assert_eq!(rga.span_count(), 1);
        
        rga.insert(&pair.key_pub, 1, b"b");
        assert_eq!(rga.span_count(), 1); // Should coalesce
        
        rga.insert(&pair.key_pub, 2, b"c");
        assert_eq!(rga.span_count(), 1); // Should coalesce
        
        assert_eq!(rga.to_string(), "abc");
        assert_eq!(rga.len(), 3);
    }

    #[test]
    fn span_coalescing_non_sequential() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        
        // Insert at beginning
        rga.insert(&pair.key_pub, 0, b"hello");
        assert_eq!(rga.span_count(), 1);
        
        // Insert at beginning again - can't coalesce (different position)
        rga.insert(&pair.key_pub, 0, b"X");
        assert_eq!(rga.span_count(), 2);
        
        assert_eq!(rga.to_string(), "Xhello");
    }

    #[test]
    fn span_coalescing_after_delete() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        
        // Insert, delete, insert - should not coalesce across delete
        rga.insert(&pair.key_pub, 0, b"abc");
        assert_eq!(rga.span_count(), 1);
        
        rga.delete(2, 1); // Delete 'c'
        // Delete splits span, so we have 2 spans now (one for 'ab', one deleted for 'c')
        
        rga.insert(&pair.key_pub, 2, b"d");
        // Can't coalesce with the deleted span
        
        assert_eq!(rga.to_string(), "abd");
    }
}

#[cfg(test)]
mod cursor_cache_tests {
    use super::*;
    use crate::key::KeyPair;

    #[test]
    fn cursor_cache_sequential_typing() {
        // Sequential typing should use the cursor cache for O(1) lookups
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        
        // Type "hello" one character at a time
        rga.insert(&pair.key_pub, 0, b"h");
        assert!(rga.cursor_cache.valid);
        assert_eq!(rga.cursor_cache.visible_pos, 0); // Position of 'h'
        
        rga.insert(&pair.key_pub, 1, b"e");
        assert!(rga.cursor_cache.valid);
        assert_eq!(rga.cursor_cache.visible_pos, 1); // Position of 'e'
        
        rga.insert(&pair.key_pub, 2, b"l");
        assert!(rga.cursor_cache.valid);
        assert_eq!(rga.cursor_cache.visible_pos, 2);
        
        rga.insert(&pair.key_pub, 3, b"l");
        assert!(rga.cursor_cache.valid);
        assert_eq!(rga.cursor_cache.visible_pos, 3);
        
        rga.insert(&pair.key_pub, 4, b"o");
        assert!(rga.cursor_cache.valid);
        assert_eq!(rga.cursor_cache.visible_pos, 4);
        
        assert_eq!(rga.to_string(), "hello");
        // All inserts coalesced into one span
        assert_eq!(rga.span_count(), 1);
    }

    #[test]
    fn cursor_cache_after_delete() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        
        rga.insert(&pair.key_pub, 0, b"hello");
        assert!(rga.cursor_cache.valid);
        
        // Delete in the middle - cache is always invalidated on delete
        // because deletes can cause span splits that change span indices
        rga.delete(2, 1); // Delete 'l'
        
        // Cache should be invalidated after any delete
        assert!(!rga.cursor_cache.valid);
        
        assert_eq!(rga.to_string(), "helo");
    }

    #[test]
    fn cursor_cache_insert_at_beginning() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        
        rga.insert(&pair.key_pub, 0, b"world");
        assert!(rga.cursor_cache.valid);
        let old_pos = rga.cursor_cache.visible_pos;
        
        // Insert at beginning - cache should shift
        rga.insert(&pair.key_pub, 0, b"hello ");
        
        // Cache was adjusted: old position shifted by insert length
        if rga.cursor_cache.valid {
            assert_eq!(rga.cursor_cache.visible_pos, old_pos + 6);
        }
        
        assert_eq!(rga.to_string(), "hello world");
    }

    #[test]
    fn cursor_cache_multiple_users() {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();
        let mut rga = Rga::new();
        
        // Alice types
        rga.insert(&alice.key_pub, 0, b"aaa");
        assert!(rga.cursor_cache.valid);
        
        // Bob types at end - different user means no coalescing, so cache is invalidated
        // (we could track chunk location, but it's simpler to invalidate for non-coalescing inserts)
        rga.insert(&bob.key_pub, 3, b"bbb");
        // Cache may be invalidated after non-coalescing insert
        
        assert_eq!(rga.to_string(), "aaabbb");
    }

    #[test]
    fn cursor_cache_random_access() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        
        rga.insert(&pair.key_pub, 0, b"0123456789");
        
        // Insert at position 5 (middle) - this splits a span, so cache is invalidated
        rga.insert(&pair.key_pub, 5, b"X");
        // Cache may be invalidated after span split
        
        // Insert at position 2 (far from cache) - cache miss triggers full lookup
        rga.insert(&pair.key_pub, 2, b"Y");
        // Cache may be invalidated after span split
        
        assert_eq!(rga.to_string(), "01Y234X56789");
    }

    #[test]
    fn cursor_cache_empty_then_insert() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        
        // Cache starts invalid
        assert!(!rga.cursor_cache.valid);
        
        // First insert
        rga.insert(&pair.key_pub, 0, b"hello");
        
        // Cache should now be valid
        assert!(rga.cursor_cache.valid);
        assert_eq!(rga.cursor_cache.visible_pos, 4); // Last char position
    }

    #[test]
    fn cursor_cache_delete_before_cache() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        
        rga.insert(&pair.key_pub, 0, b"hello world");
        assert!(rga.cursor_cache.valid);
        
        // Cache points to end of "hello world" (position 10)
        assert_eq!(rga.cursor_cache.visible_pos, 10);
        
        // Delete "hello " (positions 0-5)
        rga.delete(0, 6);
        
        // Cache is invalidated after any delete because deletes can cause
        // span splits that change span indices
        assert!(!rga.cursor_cache.valid);
        
        assert_eq!(rga.to_string(), "world");
    }

    #[test]
    fn cursor_cache_delete_at_cache() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        
        rga.insert(&pair.key_pub, 0, b"hello");
        assert!(rga.cursor_cache.valid);
        assert_eq!(rga.cursor_cache.visible_pos, 4); // Points to 'o'
        
        // Delete 'o' (the cached position)
        rga.delete(4, 1);
        
        // Cache should be invalidated since we deleted the cached position
        assert!(!rga.cursor_cache.valid);
        
        assert_eq!(rga.to_string(), "hell");
    }

    #[test]
    fn cursor_cache_backspace_pattern() {
        // Simulate backspace: delete at current position, then continue typing
        let pair = KeyPair::generate();
        let mut rga = Rga::new();
        
        // Type "hello"
        rga.insert(&pair.key_pub, 0, b"hello");
        assert_eq!(rga.to_string(), "hello");
        
        // Backspace (delete 'o')
        rga.delete(4, 1);
        assert_eq!(rga.to_string(), "hell");
        
        // Type 'p' at position 4
        rga.insert(&pair.key_pub, 4, b"p");
        assert_eq!(rga.to_string(), "hellp");
        
        // Continue typing
        rga.insert(&pair.key_pub, 5, b"!");
        assert_eq!(rga.to_string(), "hellp!");
    }
}

#[cfg(test)]
mod rga_buf_tests {
    use super::*;
    use crate::key::KeyPair;

    #[test]
    fn basic_insert() {
        let pair = KeyPair::generate();
        let mut buf = RgaBuf::new();
        
        buf.insert(&pair.key_pub, 0, b"hello");
        assert_eq!(buf.to_string(), "hello");
    }

    #[test]
    fn sequential_inserts_buffered() {
        let pair = KeyPair::generate();
        let mut buf = RgaBuf::new();
        
        // Sequential typing: h, e, l, l, o
        buf.insert(&pair.key_pub, 0, b"h");
        buf.insert(&pair.key_pub, 1, b"e");
        buf.insert(&pair.key_pub, 2, b"l");
        buf.insert(&pair.key_pub, 3, b"l");
        buf.insert(&pair.key_pub, 4, b"o");
        
        // Should be buffered, not yet in RGA
        assert!(buf.pending.is_some());
        match buf.pending.as_ref().unwrap() {
            PendingOp::Insert(ins) => assert_eq!(ins.content.as_slice(), b"hello"),
            _ => panic!("expected PendingOp::Insert"),
        }
        
        // Flush and verify
        assert_eq!(buf.to_string(), "hello");
        assert!(buf.pending.is_none());
    }

    #[test]
    fn non_sequential_flushes() {
        let pair = KeyPair::generate();
        let mut buf = RgaBuf::new();
        
        // Insert at position 0
        buf.insert(&pair.key_pub, 0, b"world");
        
        // Insert at position 0 again (not adjacent) - should flush previous
        buf.insert(&pair.key_pub, 0, b"hello ");
        
        // Verify the previous insert was flushed
        assert_eq!(buf.to_string(), "hello world");
    }

    #[test]
    fn delete_flushes_pending() {
        let pair = KeyPair::generate();
        let mut buf = RgaBuf::new();
        
        buf.insert(&pair.key_pub, 0, b"hello");
        // Delete should flush pending first
        buf.delete(2, 2); // Delete "ll"
        
        assert_eq!(buf.to_string(), "heo");
    }

    #[test]
    fn different_user_flushes() {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();
        let mut buf = RgaBuf::new();
        
        buf.insert(&alice.key_pub, 0, b"alice");
        // Different user should flush
        buf.insert(&bob.key_pub, 5, b"bob");
        
        assert_eq!(buf.to_string(), "alicebob");
    }

    #[test]
    fn empty_content_ignored() {
        let pair = KeyPair::generate();
        let mut buf = RgaBuf::new();
        
        buf.insert(&pair.key_pub, 0, b"hello");
        buf.insert(&pair.key_pub, 5, b""); // Empty - should be ignored
        
        // Pending should still be "hello"
        assert!(buf.pending.is_some());
        match buf.pending.as_ref().unwrap() {
            PendingOp::Insert(ins) => assert_eq!(ins.content.as_slice(), b"hello"),
            _ => panic!("expected PendingOp::Insert"),
        }
    }

    #[test]
    fn len_flushes() {
        let pair = KeyPair::generate();
        let mut buf = RgaBuf::new();
        
        buf.insert(&pair.key_pub, 0, b"hello");
        assert!(buf.pending.is_some());
        
        let len = buf.len();
        assert_eq!(len, 5);
        assert!(buf.pending.is_none()); // Flushed
    }

    #[test]
    fn complex_editing_pattern() {
        let pair = KeyPair::generate();
        let mut buf = RgaBuf::new();
        
        // Type "hello"
        buf.insert(&pair.key_pub, 0, b"h");
        buf.insert(&pair.key_pub, 1, b"e");
        buf.insert(&pair.key_pub, 2, b"l");
        buf.insert(&pair.key_pub, 3, b"l");
        buf.insert(&pair.key_pub, 4, b"o");
        
        // Type " world"
        buf.insert(&pair.key_pub, 5, b" ");
        buf.insert(&pair.key_pub, 6, b"w");
        buf.insert(&pair.key_pub, 7, b"o");
        buf.insert(&pair.key_pub, 8, b"r");
        buf.insert(&pair.key_pub, 9, b"l");
        buf.insert(&pair.key_pub, 10, b"d");
        
        assert_eq!(buf.to_string(), "hello world");
    }

    #[test]
    fn backspace_then_continue() {
        let pair = KeyPair::generate();
        let mut buf = RgaBuf::new();
        
        // Type "helllo" (typo with extra 'l')
        buf.insert(&pair.key_pub, 0, b"helllo");
        
        // Backspace to delete the extra 'l' at position 3
        // "helllo" -> "hello"
        buf.delete(3, 1);
        
        // Continue typing at end (position 5)
        buf.insert(&pair.key_pub, 5, b"!");
        buf.insert(&pair.key_pub, 6, b"!");
        
        assert_eq!(buf.to_string(), "hello!!");
    }
}
