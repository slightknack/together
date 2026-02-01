// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-01-31"
// driver = "Isaac Clayton"

//! Replicated Growable Array (RGA) - a sequence CRDT for collaborative text editing.
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

/// A unique identifier for an item in the RGA.
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
/// Unlike the previous span_idx/offset approach, this stores the actual
/// item ID (user_idx, seq) which is invariant across different document
/// structures. This ensures merge commutativity: the origin refers to the
/// same logical character regardless of how spans are organized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OriginId {
    /// User index of the origin character (NO_ORIGIN_USER if no origin).
    user_idx: u16,
    /// Sequence number of the origin character.
    seq: u32,
}

/// Sentinel value for user_idx indicating no origin (insert at beginning).
const NO_ORIGIN_USER: u16 = u16::MAX;

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
}

// =============================================================================
// YATA Ordering
// =============================================================================

/// Result of YATA/FugueMax comparison between two sibling spans.
///
/// When two spans share the same left origin (siblings), we use YATA rules
/// to determine their order:
/// 1. Compare right origins (null = "inserted at end" = infinity)
/// 2. If equal, compare (user, seq) descending
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum YataOrder {
    /// The new span comes BEFORE the existing span.
    Before,
    /// The new span comes AFTER the existing span.
    After,
}

/// A compact span of consecutive items inserted by the same user (30 bytes).
/// 
/// Stores both left origin and right origin to match Yjs/YATA/Fugue approach.
/// The dual-origin approach is necessary for correct merge commutativity
/// because it allows detecting subtree boundaries during merge.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct Span {
    seq: u32,
    len: u32,
    /// Left origin user index (NO_ORIGIN_USER if no origin).
    /// The character this span was inserted AFTER.
    origin_user_idx: u16,
    /// Left origin sequence number.
    origin_seq: u32,
    /// Right origin user index (NO_ORIGIN_USER if no origin).
    /// The character that was immediately to the RIGHT when this span was inserted.
    right_origin_user_idx: u16,
    /// Right origin sequence number.
    right_origin_seq: u32,
    content_offset: u32,
    user_idx: u16,
    deleted: bool,
    _padding: u8,
}

impl Span {
    fn new(
        user_idx: u16,
        seq: u32,
        len: u32,
        origin: OriginId,
        right_origin: OriginId,
        content_offset: u32,
    ) -> Span {
        return Span {
            seq,
            len,
            origin_user_idx: origin.user_idx,
            origin_seq: origin.seq,
            right_origin_user_idx: right_origin.user_idx,
            right_origin_seq: right_origin.seq,
            content_offset,
            user_idx,
            deleted: false,
            _padding: 0,
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

    fn right_origin(&self) -> OriginId {
        return OriginId {
            user_idx: self.right_origin_user_idx,
            seq: self.right_origin_seq,
        };
    }

    fn set_right_origin(&mut self, right_origin: OriginId) {
        self.right_origin_user_idx = right_origin.user_idx;
        self.right_origin_seq = right_origin.seq;
    }

    #[inline(always)]
    fn has_right_origin(&self) -> bool {
        return self.right_origin_user_idx != NO_ORIGIN_USER;
    }

    #[inline(always)]
    fn contains_seq(&self, seq: u32) -> bool {
        return seq >= self.seq && seq < self.seq + self.len;
    }

    /// Split this span at the given offset, returning the right part.
    /// 
    /// For left origin:
    /// - The right part's left origin is set to the last character of the left part.
    ///   This is semantically correct because within a span, each character
    ///   (except the first) was conceptually inserted after the previous one.
    /// 
    /// For right origin:
    /// - The right part keeps the original right_origin (what was to its right at insertion).
    /// - The left part ALSO keeps the original right_origin.
    ///   This is important for YATA ordering: the right_origin captures the insertion
    ///   context, not the current document structure. Changing it during splits would
    ///   break merge commutativity.
    #[inline]
    fn split(&mut self, offset: u32) -> Span {
        debug_assert!(offset > 0 && offset < self.len);
        // The right part's left origin is the last character of the left part.
        // This is correct because within a span, seq N was inserted after seq N-1.
        // Both parts keep the original right_origin - it's an insertion-time property.
        let right = Span {
            seq: self.seq + offset,
            len: self.len - offset,
            origin_user_idx: self.user_idx,
            origin_seq: self.seq + offset - 1,
            // Right part keeps the original right_origin
            right_origin_user_idx: self.right_origin_user_idx,
            right_origin_seq: self.right_origin_seq,
            content_offset: self.content_offset + offset,
            user_idx: self.user_idx,
            deleted: self.deleted,
            _padding: 0,
        };
        // Left part keeps its original right_origin - don't change it!
        // The right_origin is an insertion-time property, not a document structure property.
        self.len = offset;
        return right;
    }

    #[inline(always)]
    fn visible_len(&self) -> u32 {
        if self.deleted { 0 } else { self.len }
    }
}

/// Per-user append-only column storing content.
#[derive(Clone, Debug)]
struct Column {
    /// The content bytes for this user's insertions.
    content: Vec<u8>,
    /// Next sequence number to assign.
    next_seq: u32,
}

impl Column {
    fn new() -> Column {
        return Column {
            content: Vec::new(),
            next_seq: 0,
        };
    }
    
    /// Check if a sequence number has been seen (item exists).
    #[inline]
    fn has_seq(&self, seq: u32) -> bool {
        seq < self.next_seq
    }
    
    /// Find the first missing seq in a range [start, start+len).
    /// Returns None if all seqs exist, Some(offset) for first missing.
    #[inline]
    fn first_missing_in_range(&self, start: u32, len: u32) -> Option<u32> {
        if start >= self.next_seq {
            // All items are missing, first missing is at offset 0
            return Some(0);
        }
        if start + len <= self.next_seq {
            // All items exist
            return None;
        }
        // Some items exist, some don't
        // next_seq is the first missing seq
        Some(self.next_seq - start)
    }
}

/// Cursor cache for amortizing sequential lookups.
///
/// Text editing has strong locality: sequential typing inserts at pos+1,
/// backspace deletes at pos-1. By caching the last lookup result, we can
/// scan from the cached position instead of doing a full O(log n) lookup.
///
/// For sequential typing, this turns O(log n) per insert into O(1) amortized.
///
/// This cache also stores chunk location to avoid repeated find_chunk_by_index calls.
#[derive(Clone, Debug)]
struct CursorCache {
    /// The visible position that was looked up (the character BEFORE which we insert).
    /// For insert at pos P, we look up pos P-1 to find the origin.
    visible_pos: u64,
    /// The span index containing that position.
    span_idx: usize,
    /// The offset within the span.
    offset_in_span: u64,
    /// The chunk index containing the span (for avoiding find_chunk_by_index).
    chunk_idx: usize,
    /// The index within the chunk (for avoiding find_chunk_by_index).
    idx_in_chunk: usize,
    /// Whether the cache is valid.
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

    /// Adjust the cache after a delete starting at the given position with the given length.
    /// We can preserve the cache in some cases:
    /// - If the delete is entirely AFTER the cached position, the cache is still valid
    ///   (the span_idx and offset_in_span don't change for earlier positions)
    /// - If the delete touches or precedes the cached position, we must invalidate
    fn adjust_after_delete(&mut self, delete_pos: u64, _delete_len: u64) {
        if !self.valid {
            return;
        }
        // If the delete starts after our cached position, the cache is still valid
        // because deletions after our position don't affect span indices before it
        if delete_pos > self.visible_pos {
            // Cache remains valid - delete is after our cached position
            return;
        }
        // Delete touches or precedes cached position - must invalidate
        self.invalidate();
    }
}

/// A Replicated Growable Array.
///
/// Uses a weighted list of spans where each span's weight is its visible
/// character count. This enables O(log n) position lookup once the weighted
/// list is optimized.
#[derive(Clone)]
pub struct Rga {
    /// Spans in document order, weighted by visible character count.
    spans: BTreeList<Span>,
    /// Per-user columns for content storage, indexed by user_idx.
    columns: Vec<Column>,
    /// Maps KeyPub to user index.
    users: UserTable,
    /// Cursor cache for amortizing sequential lookups.
    cursor_cache: CursorCache,
    /// Lamport timestamp for versioning.
    lamport: u64,
    /// Index from (user_idx, seq) to span index for O(1) ID lookup.
    /// Maps the FIRST seq of each span to its index.
    /// When spans are split, new entries are added.
    id_index: FxHashMap<(u16, u32), usize>,
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
            id_index: FxHashMap::default(),
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

    /// Debug: dump all spans with their origins
    #[cfg(debug_assertions)]
    pub fn debug_spans(&self) -> String {
        let mut out = String::new();
        for (i, span) in self.spans.iter().enumerate() {
            let user_key = self.users.get_key(span.user_idx).unwrap();
            let content = &self.columns[span.user_idx as usize].content
                [span.content_offset as usize..(span.content_offset + span.len) as usize];
            let content_str = std::str::from_utf8(content).unwrap_or("?");
            let origin_str = if span.has_origin() {
                let o = span.origin();
                if let Some(origin_user) = self.users.get_key(o.user_idx) {
                    format!("({:?}[{}], {})", &origin_user.0[..2], o.user_idx, o.seq)
                } else {
                    format!("(?, {})", o.seq)
                }
            } else {
                "NONE".to_string()
            };
            let del_str = if span.deleted { " [DEL]" } else { "" };
            out.push_str(&format!(
                "[{}] user={:?}[{}] seq={}-{} origin={} content={:?}{}\n",
                i, &user_key.0[..2], span.user_idx, span.seq, span.seq + span.len - 1, 
                origin_str, content_str, del_str
            ));
        }
        out
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
    /// This avoids HashMap lookups when the caller already has the user_idx.
    /// Does not return ItemId to avoid the overhead of looking up the user key.
    #[inline]
    fn insert_with_user_idx(&mut self, user_idx: u16, pos: u64, content: &[u8]) {
        // Increment lamport clock
        self.lamport += 1;

        let column = &mut self.columns[user_idx as usize];
        let seq = column.next_seq;
        let content_offset = column.content.len() as u32;
        column.content.extend_from_slice(content);
        column.next_seq += content.len() as u32;

        // Create the span (origin and right_origin are set during insert_span_at_pos_optimized)
        let span = Span::new(
            user_idx,
            seq,
            content.len() as u32,
            OriginId::none(),
            OriginId::none(), // Will be set properly in Part 2
            content_offset,
        );

        self.insert_span_at_pos_optimized(span, pos);
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

        // Increment lamport clock
        self.lamport += 1;

        // Adjust or invalidate the cursor cache
        // Delete operations can create/remove spans, so we invalidate if the delete
        // touches or precedes the cached position to keep things simple and correct.
        self.cursor_cache.adjust_after_delete(start, len);

        let mut remaining = len;

        while remaining > 0 {
            // Find the span at current visible position
            let (span_idx, offset_in_span) = match self.spans.find_by_weight(start) {
                Some((idx, off)) => (idx, off),
                None => panic!("position {} not found", start),
            };

            let span = self.spans.get(span_idx).unwrap();
            let span_visible = span.visible_len() as u64;

            if offset_in_span == 0 && remaining >= span_visible {
                // Delete entire span - mark as deleted and update weight to 0
                self.spans.get_mut(span_idx).unwrap().deleted = true;
                self.spans.update_weight(span_idx, 0);
                remaining -= span_visible;
            } else if offset_in_span == 0 {
                // Delete prefix of span - split and delete left part
                let mut span = self.spans.remove(span_idx);
                let right = span.split(remaining as u32);
                span.deleted = true;
                self.spans.insert(span_idx, span, 0);
                self.spans.insert(span_idx + 1, right, right.visible_len() as u64);
                remaining = 0;
            } else if offset_in_span + remaining >= span_visible {
                // Delete suffix of span - split and delete right part
                let to_delete = span_visible - offset_in_span;
                let mut span = self.spans.remove(span_idx);
                let mut right = span.split(offset_in_span as u32);
                right.deleted = true;
                self.spans.insert(span_idx, span, span.visible_len() as u64);
                self.spans.insert(span_idx + 1, right, 0);
                remaining -= to_delete;
            } else {
                // Delete middle of span - split twice
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

    /// Insert a span at the given visible position (for local edits).
    /// Optimized version that sets the origin during insert to avoid double lookup.
    /// Attempts to coalesce with the preceding span if possible.
    /// Uses cursor caching for O(1) sequential typing with chunk location caching.
    #[inline]
    fn insert_span_at_pos_optimized(&mut self, mut span: Span, pos: u64) {
        let span_len = span.visible_len() as u64;
        let doc_len = self.spans.total_weight();

        if self.spans.is_empty() {
            // No origin for first span - use RGA ordering (no-op for empty doc)
            self.insert_span_rga(span, None, None);
            self.cursor_cache.update(pos + span_len - 1, 0, span_len - 1, 0, 0);
            return;
        }

        if pos == 0 {
            // No left_origin when inserting at beginning
            // right_origin is the first visible character (what will be pushed right)
            let right_origin = self.find_first_visible_item_id();
            self.insert_span_rga(span, None, right_origin);
            self.cursor_cache.invalidate();
            return;
        }

        // The position we need to look up: the character just before insert position
        let lookup_pos = pos - 1;

        // Try to use the cursor cache for sequential typing
        // Sequential typing: last insert was at position P, next insert is at P + last_len
        // So we look up P + last_len - 1, which should be cached as the end of last insert
        // We also cache chunk location to avoid find_chunk_by_index calls
        let (prev_idx, offset_in_prev, chunk_idx, idx_in_chunk) = if self.cursor_cache.valid
            && self.cursor_cache.visible_pos == lookup_pos
        {
            // Cache hit! Use cached position and chunk location directly
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
            // One position forward from cache - common for sequential typing after non-coalescing insert
            // Try to scan forward one character using cached chunk location
            let cached_span = self.spans.get_with_chunk_hint(
                self.cursor_cache.chunk_idx,
                self.cursor_cache.idx_in_chunk,
            ).unwrap();
            let cached_visible = cached_span.visible_len() as u64;
            
            if self.cursor_cache.offset_in_span + 1 < cached_visible {
                // Next position is within the same span
                (
                    self.cursor_cache.span_idx,
                    self.cursor_cache.offset_in_span + 1,
                    self.cursor_cache.chunk_idx,
                    self.cursor_cache.idx_in_chunk,
                )
            } else {
                // Need to move to next span - scan forward
                let mut idx = self.cursor_cache.span_idx + 1;
                while idx < self.spans.len() {
                    let s = self.spans.get(idx).unwrap();
                    if s.visible_len() > 0 {
                        break;
                    }
                    idx += 1;
                }
                if idx < self.spans.len() {
                    // Need full lookup for chunk info since we moved spans
                    match self.spans.find_by_weight_with_chunk(lookup_pos) {
                        Some((span_idx, off, c_idx, i_in_c)) => (span_idx, off, c_idx, i_in_c),
                        None => {
                            self.insert_span_rga(span, None, None);
                            self.cursor_cache.invalidate();
                            return;
                        }
                    }
                } else {
                    // Fallback to full lookup
                    match self.spans.find_by_weight_with_chunk(lookup_pos) {
                        Some((span_idx, off, c_idx, i_in_c)) => (span_idx, off, c_idx, i_in_c),
                        None => {
                            self.insert_span_rga(span, None, None);
                            self.cursor_cache.invalidate();
                            return;
                        }
                    }
                }
            }
        } else {
            // Cache miss - do full lookup with chunk info
            match self.spans.find_by_weight_with_chunk(lookup_pos) {
                Some((span_idx, off, c_idx, i_in_c)) => (span_idx, off, c_idx, i_in_c),
                None => {
                    // pos >= total_weight + 1, insert at end (shouldn't normally happen)
                    self.insert_span_rga(span, None, None);
                    self.cursor_cache.invalidate();
                    return;
                }
            }
        };

        // Use cached chunk location to get prev_span without find_chunk_by_index
        let prev_span = self.spans.get_with_chunk_hint(chunk_idx, idx_in_chunk).unwrap();
        let prev_visible_len = prev_span.visible_len() as u64;
        
        // Build the left_origin ItemId for the character we're inserting after
        let origin_user = *self.users.get_key(prev_span.user_idx).unwrap();
        let origin_seq = prev_span.seq + offset_in_prev as u32;
        let left_origin_id = ItemId {
            user: origin_user,
            seq: origin_seq as u64,
        };

        // Build the right_origin ItemId: the character at position `pos` (what will be pushed right)
        // If pos == doc_len, there's nothing to the right, so right_origin is None
        let right_origin_id = if pos < doc_len {
            self.find_item_id_at_visible_pos(pos)
        } else {
            None
        };

        // Check if we can coalesce: same user, consecutive seq, contiguous content, not deleted
        // Also check offset_in_prev == prev_span.visible_len - 1 to ensure we're at the end of the span
        // Coalescing is safe because consecutive chars from same user at same position
        // will have the same RGA ordering as a single span
        if prev_span.user_idx == span.user_idx
            && !prev_span.deleted
            && prev_span.seq + prev_span.len == span.seq
            && prev_span.content_offset + prev_span.len == span.content_offset
            && offset_in_prev == prev_visible_len - 1
        {
            // Coalesce by extending the previous span
            // Use modify_and_update_weight_with_hint to avoid chunk lookup
            let add_len = span.len;
            let (new_weight, new_chunk_idx, new_idx_in_chunk) = self.spans.modify_and_update_weight_with_hint(
                chunk_idx,
                idx_in_chunk,
                |prev_span| {
                    prev_span.len += add_len;
                    prev_span.visible_len() as u64
                },
            ).unwrap();
            
            // Update cache: point to end of the coalesced span with chunk location
            // After insert at pos with span_len, the last inserted char is at pos + span_len - 1
            self.cursor_cache.update(
                pos + span_len - 1,
                prev_idx,
                new_weight - 1,
                new_chunk_idx,
                new_idx_in_chunk,
            );
            return;
        }

        // Can't coalesce - use RGA ordering to find correct position.
        // 
        // We pass the known origin position (prev_idx) to avoid redundant lookup.
        // YATA ordering is still needed because:
        // 1. There might be siblings (other spans with same left_origin)
        // 2. Concurrent edits during merge require consistent ordering
        self.insert_span_rga_with_hint(span, left_origin_id, right_origin_id, prev_idx);
        self.cursor_cache.invalidate();
    }

    /// Find the ItemId of the first visible character in the document.
    /// Returns None if the document has no visible characters.
    fn find_first_visible_item_id(&self) -> Option<ItemId> {
        for span in self.spans.iter() {
            if !span.deleted && span.len > 0 {
                let user = *self.users.get_key(span.user_idx)?;
                return Some(ItemId {
                    user,
                    seq: span.seq as u64,
                });
            }
        }
        None
    }

    /// Find the ItemId of the character at the given visible position.
    /// Returns None if position is out of bounds.
    fn find_item_id_at_visible_pos(&self, pos: u64) -> Option<ItemId> {
        let (span_idx, offset_in_span) = self.spans.find_by_weight(pos)?;
        let span = self.spans.get(span_idx)?;
        let user = *self.users.get_key(span.user_idx)?;
        Some(ItemId {
            user,
            seq: (span.seq + offset_in_span as u32) as u64,
        })
    }

    /// Find span containing the given ItemId using the ID index.
    /// Falls back to linear search if index doesn't have the entry.
    fn find_span_by_id(&self, id: &ItemId) -> Option<usize> {
        let user_idx = match self.users.get(&id.user) {
            Some(idx) => idx,
            None => return None,
        };
        let seq = id.seq as u32;
        
        // Try index lookup first - find the span that could contain this seq
        // The index maps span start seq -> index, so we need to find the
        // largest start_seq <= seq for this user
        if let Some(&idx) = self.id_index.get(&(user_idx, seq)) {
            // Direct hit - seq is at span start
            let span = self.spans.get(idx)?;
            if span.user_idx == user_idx && span.contains_seq(seq) {
                return Some(idx);
            }
        }
        
        // Fallback to linear search for items not at span start
        // This handles the case where seq is in the middle of a span
        for (i, span) in self.spans.iter().enumerate() {
            if span.user_idx == user_idx && span.contains_seq(seq) {
                return Some(i);
            }
        }
        return None;
    }
    
    /// Update the ID index after inserting a span at the given index.
    fn update_index_after_insert(&mut self, idx: usize) {
        // Update indices for all spans after the insertion point
        let keys_to_update: Vec<(u16, u32)> = self.id_index
            .iter()
            .filter(|&(_, &v)| v >= idx)
            .map(|(&k, _)| k)
            .collect();
        
        for key in keys_to_update {
            if let Some(v) = self.id_index.get_mut(&key) {
                *v += 1;
            }
        }
        
        // Add the new span to the index
        if let Some(span) = self.spans.get(idx) {
            self.id_index.insert((span.user_idx, span.seq), idx);
        }
    }
    
    /// Update the ID index after removing a span at the given index.
    fn update_index_after_remove(&mut self, removed_user_idx: u16, removed_seq: u32, idx: usize) {
        // Remove the old entry
        self.id_index.remove(&(removed_user_idx, removed_seq));
        
        // Update indices for all spans after the removal point
        let keys_to_update: Vec<(u16, u32)> = self.id_index
            .iter()
            .filter(|&(_, &v)| v > idx)
            .map(|(&k, _)| k)
            .collect();
        
        for key in keys_to_update {
            if let Some(v) = self.id_index.get_mut(&key) {
                *v -= 1;
            }
        }
    }
    
    /// Rebuild the ID index from scratch.
    /// Call this after bulk operations that may have invalidated the index.
    fn rebuild_id_index(&mut self) {
        self.id_index.clear();
        for (i, span) in self.spans.iter().enumerate() {
            self.id_index.insert((span.user_idx, span.seq), i);
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
            user: id.user,
            seq: id.seq,
        };
    }

    /// Apply an operation from a writer.
    /// Returns true if the operation was applied, false if it was already present.
    pub fn apply(&mut self, user: &KeyPub, block: &OpBlock) -> bool {
        match &block.op {
            Op::Insert { origin, right_origin, seq, len } => {
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
                    OriginId::none(), // Will be set during insert_span_rga
                    OriginId::none(), // Will be set during insert_span_rga
                    content_offset,
                );

                // Pass both origins to the YATA/FugueMax algorithm
                let left_origin_id = origin.as_ref().map(Self::convert_id);
                let right_origin_id = right_origin.as_ref().map(Self::convert_id);
                self.insert_span_rga(span, left_origin_id, right_origin_id);
                return true;
            }
            Op::Delete { target } => {
                let target_id = Self::convert_id(target);
                return self.delete_by_id(&target_id);
            }
        }
    }

    // =========================================================================
    // YATA Ordering Helpers
    // =========================================================================

    /// Compare two sibling spans using YATA/FugueMax ordering rules.
    ///
    /// Both spans must share the same left origin (be siblings).
    /// Returns whether `new_span` should come Before or After `existing`.
    ///
    /// YATA rules:
    /// 1. Compare right origins: null = "inserted at end" = infinity
    ///    - Non-null (finite) comes BEFORE null (infinite)
    ///    - Higher right_origin ID comes FIRST (inserted with more context)
    /// 2. If right origins equal: higher (user, seq) comes FIRST
    fn yata_compare(
        &self,
        new_right_origin: OriginId,
        new_has_right_origin: bool,
        new_user: KeyPub,
        new_seq: u32,
        existing: &Span,
    ) -> YataOrder {
        let existing_has_ro = existing.has_right_origin();
        
        // Rule 1a: Compare null vs non-null right origin
        if new_has_right_origin != existing_has_ro {
            if !existing_has_ro && new_has_right_origin {
                // Existing has no right origin (infinity), we have one (finite)
                // Finite < infinity, so we come BEFORE
                return YataOrder::Before;
            } else {
                // We have no right origin (infinity), existing has one (finite)
                // Finite < infinity, so existing comes before us
                return YataOrder::After;
            }
        }
        
        // Rule 1b: Both have right origins - compare IDs
        if new_has_right_origin && existing_has_ro {
            let existing_ro = existing.right_origin();
            
            let existing_ro_user = match self.users.get_key(existing_ro.user_idx) {
                Some(u) => *u,
                None => return YataOrder::After, // Unknown user, skip existing
            };
            let new_ro_user = match self.users.get_key(new_right_origin.user_idx) {
                Some(u) => *u,
                None => return YataOrder::Before, // Unknown user, insert here
            };
            
            let existing_ro_key = (existing_ro_user, existing_ro.seq);
            let new_ro_key = (new_ro_user, new_right_origin.seq);
            
            if existing_ro_key > new_ro_key {
                // Existing's right origin is higher - existing was inserted later
                return YataOrder::After;
            } else if existing_ro_key < new_ro_key {
                // Our right origin is higher - we were inserted later
                return YataOrder::Before;
            }
            // Equal, fall through to tiebreaker
        }
        
        // Rule 2: Right origins equal - use (user, seq) as tiebreaker
        let existing_user = *self.users.get_key(existing.user_idx).unwrap();
        if (existing_user, existing.seq) > (new_user, new_seq) {
            // Existing has higher precedence
            return YataOrder::After;
        }
        // We have higher or equal precedence
        return YataOrder::Before;
    }

    /// Check if a span's origin is within any of the tracked subtree ranges.
    #[inline]
    fn origin_in_subtree(origin: OriginId, subtree_ranges: &[(u16, u32, u32)]) -> bool {
        subtree_ranges.iter().any(|&(user_idx, seq_start, seq_end)| {
            origin.user_idx == user_idx 
                && origin.seq >= seq_start 
                && origin.seq <= seq_end
        })
    }

    /// Add a span's range to the subtree tracking.
    #[inline]
    fn add_to_subtree(span: &Span, subtree_ranges: &mut SmallVec<[(u16, u32, u32); 8]>) {
        subtree_ranges.push((span.user_idx, span.seq, span.seq + span.len - 1));
    }

    /// Check if `other` is a sibling of the span being inserted.
    /// Siblings share the same left origin.
    fn is_sibling(&self, other: &Span, origin_user: KeyPub, origin_seq: u64) -> bool {
        if !other.has_origin() {
            return false;
        }
        let other_origin = other.origin();
        let other_origin_user = match self.users.get_key(other_origin.user_idx) {
            Some(u) => *u,
            None => return false,
        };
        other_origin_user == origin_user && other_origin.seq as u64 == origin_seq
    }

    // =========================================================================
    // Insert Position Finding
    // =========================================================================

    /// Find insertion position when the new span has a left origin.
    ///
    /// Scans right from the origin, comparing with siblings using YATA rules.
    /// Skips descendants of siblings we pass over.
    /// Returns the index where the new span should be inserted.
    fn find_position_with_origin(
        &mut self,
        origin_id: &ItemId,
        span_user: KeyPub,
        span_seq: u32,
        span_right_origin: OriginId,
        span_has_right_origin: bool,
    ) -> usize {
        // Find the left origin span
        let origin_idx = match self.find_span_by_id(origin_id) {
            Some(idx) => idx,
            None => return self.spans.len(), // Origin not found, insert at end
        };
        
        // Split origin span if needed
        let origin_span = self.spans.get(origin_idx).unwrap();
        let offset_in_span = (origin_id.seq as u32) - origin_span.seq;
        if offset_in_span < origin_span.len - 1 {
            let mut existing = self.spans.remove(origin_idx);
            let right = existing.split(offset_in_span + 1);
            self.spans.insert(origin_idx, existing, existing.visible_len() as u64);
            self.spans.insert(origin_idx + 1, right, right.visible_len() as u64);
        }
        
        let mut pos = origin_idx + 1;
        
        // Fast path: check if we can exit immediately without YATA scan
        if pos >= self.spans.len() {
            return pos;
        }
        
        let other = self.spans.get(pos).unwrap();
        if !other.has_origin() {
            return pos;
        }
        
        let other_origin = other.origin();
        let other_origin_user = self.users.get_key(other_origin.user_idx);
        if other_origin_user != Some(&origin_id.user) || other_origin.seq as u64 != origin_id.seq {
            return pos;
        }
        
        // Slow path: need full YATA scan with subtree tracking
        let origin_user_idx = self.ensure_user(&origin_id.user);
        let mut subtree_ranges: SmallVec<[(u16, u32, u32); 8]> = SmallVec::new();
        subtree_ranges.push((origin_user_idx, origin_id.seq as u32, origin_id.seq as u32));
        
        while pos < self.spans.len() {
            let other = self.spans.get(pos).unwrap();
            
            // Check if this span is a sibling
            if !self.is_sibling(other, origin_id.user, origin_id.seq) {
                // Not a sibling - check if it's a descendant
                if other.has_origin() {
                    let other_origin = other.origin();
                    if Self::origin_in_subtree(other_origin, &subtree_ranges) {
                        Self::add_to_subtree(other, &mut subtree_ranges);
                        pos += 1;
                        continue;
                    }
                }
                // Not in subtree - we've exited
                break;
            }
            
            // It's a sibling - use YATA comparison
            let order = self.yata_compare(
                span_right_origin,
                span_has_right_origin,
                span_user,
                span_seq,
                other,
            );
            
            match order {
                YataOrder::Before => break,
                YataOrder::After => {
                    Self::add_to_subtree(other, &mut subtree_ranges);
                    pos = self.skip_subtree(pos);
                }
            }
        }
        
        pos
    }

    /// Find insertion position when the new span has no left origin (root level).
    ///
    /// Scans from the beginning, comparing with other root-level spans.
    /// Skips descendants of root spans we pass over.
    /// Returns the index where the new span should be inserted.
    fn find_position_at_root(
        &self,
        span_user: KeyPub,
        span_seq: u32,
        span_right_origin: OriginId,
        span_has_right_origin: bool,
    ) -> usize {
        let mut pos = 0;
        
        while pos < self.spans.len() {
            let other = self.spans.get(pos).unwrap();
            
            // Skip descendants (spans with origins)
            if other.has_origin() {
                pos += 1;
                continue;
            }
            
            // Root-level sibling - use YATA comparison
            let order = self.yata_compare(
                span_right_origin,
                span_has_right_origin,
                span_user,
                span_seq,
                other,
            );
            
            match order {
                YataOrder::Before => break,
                YataOrder::After => {
                    pos = self.skip_subtree(pos);
                }
            }
        }
        
        pos
    }

    /// Skip over a span and its entire subtree.
    /// 
    /// In RGA, spans form a tree where each span's children are spans whose origin
    /// points to a character within that span. In document order, a span's subtree
    /// immediately follows it.
    /// 
    /// Given a span at position `start_pos`, this returns the position after the
    /// span and all its descendants.
    fn skip_subtree(&self, start_pos: usize) -> usize {
        if start_pos >= self.spans.len() {
            return start_pos;
        }
        
        let start_span = self.spans.get(start_pos).unwrap();
        let start_user_idx = start_span.user_idx;
        let start_seq = start_span.seq;
        let start_end_seq = start_span.seq + start_span.len - 1;
        
        let mut pos = start_pos + 1;
        
        // Skip all spans that are descendants of start_span.
        // A span is a descendant if its origin chain leads back to start_span.
        // 
        // We use a simplified check: a span is in our subtree if its origin
        // is within start_span OR if its origin is in a span we've already
        // determined to be in our subtree.
        // 
        // To avoid recursion, we track the "frontier" - the rightmost seq we've
        // seen from any span in the subtree. Any span whose origin is <= this
        // frontier (for the same user) could be in the subtree.
        //
        // Actually, for correctness we need to track which (user, seq) ranges
        // are part of the subtree. This is complex, so we use a simpler approach:
        // scan forward and check if each span's origin is within any span we've
        // already included in the subtree.
        
        // Track all (user_idx, seq_start, seq_end) ranges in the subtree
        let mut subtree_ranges: SmallVec<[(u16, u32, u32); 8]> = SmallVec::new();
        subtree_ranges.push((start_user_idx, start_seq, start_end_seq));
        
        while pos < self.spans.len() {
            let other = self.spans.get(pos).unwrap();
            
            // If this span has no origin, it's at the root level - not a descendant
            if !other.has_origin() {
                break;
            }
            
            let other_origin = other.origin();
            
            // Check if this span's origin is within any span in our subtree
            let is_descendant = subtree_ranges.iter().any(|&(user_idx, seq_start, seq_end)| {
                other_origin.user_idx == user_idx 
                    && other_origin.seq >= seq_start 
                    && other_origin.seq <= seq_end
            });
            
            if is_descendant {
                // Add this span's range to the subtree
                subtree_ranges.push((other.user_idx, other.seq, other.seq + other.len - 1));
                pos += 1;
            } else {
                // Not a descendant - we've exited the subtree
                break;
            }
        }
        
        pos
    }

    /// Insert a span using YATA/FugueMax ordering rules with dual origins.
    /// 
    /// The left_origin is the character this span was inserted AFTER.
    /// The right_origin is the character that was immediately to the RIGHT when inserted.
    /// 
    /// The YATA/FugueMax algorithm uses both origins to achieve correct merge commutativity:
    /// Insert a span using YATA/FugueMax ordering rules with dual origins.
    ///
    /// The algorithm:
    /// 1. Set origins on the span
    /// 2. Find insertion position using YATA rules
    /// 3. Insert at that position
    ///
    /// See `find_position_with_origin` and `find_position_at_root` for details.
    fn insert_span_rga(
        &mut self,
        mut span: Span,
        left_origin: Option<ItemId>,
        right_origin: Option<ItemId>,
    ) {
        let span_len = span.visible_len() as u64;

        // Set origins on the span
        if let Some(ref origin_id) = left_origin {
            let origin_user_idx = self.ensure_user(&origin_id.user);
            span.set_origin(OriginId::some(origin_user_idx, origin_id.seq as u32));
        }
        if let Some(ref ro) = right_origin {
            let ro_user_idx = self.ensure_user(&ro.user);
            span.set_right_origin(OriginId::some(ro_user_idx, ro.seq as u32));
        }

        // Handle empty document
        if self.spans.is_empty() {
            self.spans.insert(0, span, span_len);
            return;
        }

        // Get span info for YATA comparison
        let span_user = *self.users.get_key(span.user_idx).unwrap();
        let span_right_origin = span.right_origin();
        let span_has_right_origin = span.has_right_origin();

        // Find insertion position
        let insert_idx = if let Some(ref origin_id) = left_origin {
            self.find_position_with_origin(
                origin_id,
                span_user,
                span.seq,
                span_right_origin,
                span_has_right_origin,
            )
        } else {
            self.find_position_at_root(
                span_user,
                span.seq,
                span_right_origin,
                span_has_right_origin,
            )
        };

        self.spans.insert(insert_idx, span, span_len);
    }

    /// Insert a span with a hint for the origin position.
    /// 
    /// This is an optimization for local inserts where we already know the origin
    /// span's index from a previous lookup. Avoids redundant O(n) find_span_by_id.
    fn insert_span_rga_with_hint(
        &mut self,
        mut span: Span,
        left_origin: ItemId,
        right_origin: Option<ItemId>,
        origin_idx_hint: usize,
    ) {
        let span_len = span.visible_len() as u64;

        // Set origins on the span
        let origin_user_idx = self.ensure_user(&left_origin.user);
        span.set_origin(OriginId::some(origin_user_idx, left_origin.seq as u32));
        if let Some(ref ro) = right_origin {
            let ro_user_idx = self.ensure_user(&ro.user);
            span.set_right_origin(OriginId::some(ro_user_idx, ro.seq as u32));
        }

        // Get span info for YATA comparison
        let span_user = *self.users.get_key(span.user_idx).unwrap();
        let span_right_origin = span.right_origin();
        let span_has_right_origin = span.has_right_origin();

        // Use the hint to find position, avoiding find_span_by_id
        let insert_idx = self.find_position_with_origin_hint(
            &left_origin,
            origin_idx_hint,
            span_user,
            span.seq,
            span_right_origin,
            span_has_right_origin,
        );

        self.spans.insert(insert_idx, span, span_len);
    }

    /// Find insertion position using a hint for the origin index.
    /// 
    /// Like find_position_with_origin but skips the find_span_by_id call.
    fn find_position_with_origin_hint(
        &mut self,
        origin_id: &ItemId,
        origin_idx: usize,
        span_user: KeyPub,
        span_seq: u32,
        span_right_origin: OriginId,
        span_has_right_origin: bool,
    ) -> usize {
        // Split origin span if needed
        let origin_span = self.spans.get(origin_idx).unwrap();
        let offset_in_span = (origin_id.seq as u32) - origin_span.seq;
        let mut idx_after_origin = origin_idx + 1;
        
        if offset_in_span < origin_span.len - 1 {
            let mut existing = self.spans.remove(origin_idx);
            let right = existing.split(offset_in_span + 1);
            self.spans.insert(origin_idx, existing, existing.visible_len() as u64);
            self.spans.insert(origin_idx + 1, right, right.visible_len() as u64);
            idx_after_origin = origin_idx + 1; // Insert position is after left half
        }
        
        let mut pos = idx_after_origin;
        
        // Fast path: check if we can exit immediately without YATA scan
        if pos >= self.spans.len() {
            return pos;
        }
        
        let other = self.spans.get(pos).unwrap();
        if !other.has_origin() {
            return pos;
        }
        
        let other_origin = other.origin();
        let other_origin_user = self.users.get_key(other_origin.user_idx);
        if other_origin_user != Some(&origin_id.user) || other_origin.seq as u64 != origin_id.seq {
            return pos;
        }
        
        // Slow path: need full YATA scan with subtree tracking
        let origin_user_idx = self.ensure_user(&origin_id.user);
        let mut subtree_ranges: SmallVec<[(u16, u32, u32); 8]> = SmallVec::new();
        subtree_ranges.push((origin_user_idx, origin_id.seq as u32, origin_id.seq as u32));
        
        while pos < self.spans.len() {
            let other = self.spans.get(pos).unwrap();
            
            // Check if this span is a sibling
            if !self.is_sibling(other, origin_id.user, origin_id.seq) {
                // Not a sibling - check if it's a descendant
                if other.has_origin() {
                    let other_origin = other.origin();
                    if Self::origin_in_subtree(other_origin, &subtree_ranges) {
                        Self::add_to_subtree(other, &mut subtree_ranges);
                        pos += 1;
                        continue;
                    }
                }
                // Not in subtree - we've exited
                break;
            }
            
            // It's a sibling - use YATA comparison
            let order = self.yata_compare(
                span_right_origin,
                span_has_right_origin,
                span_user,
                span_seq,
                other,
            );
            
            match order {
                YataOrder::Before => break,
                YataOrder::After => {
                    Self::add_to_subtree(other, &mut subtree_ranges);
                    pos = self.skip_subtree(pos);
                }
            }
        }
        
        pos
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

        let offset = (id.seq as u32) - span.seq;
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
            self.spans.insert(idx + 1, right, right.visible_len() as u64);
        } else if offset == span_len - 1 {
            // Delete last item
            let mut existing = self.spans.remove(idx);
            let mut right = existing.split(offset);
            right.deleted = true;
            self.spans.insert(idx, existing, existing.visible_len() as u64);
            self.spans.insert(idx + 1, right, 0);
        } else {
            // Delete middle item
            let mut existing = self.spans.remove(idx);
            let mut mid_right = existing.split(offset);
            let right = mid_right.split(1);
            mid_right.deleted = true;
            self.spans.insert(idx, existing, existing.visible_len() as u64);
            self.spans.insert(idx + 1, mid_right, 0);
            self.spans.insert(idx + 2, right, right.visible_len() as u64);
        }

        return true;
    }
}

// --- Buffered wrapper for batching adjacent operations ---

/// A pending insert operation waiting to be flushed.
#[derive(Clone, Debug)]
struct PendingInsert {
    /// The user performing the insert.
    user_idx: u16,
    /// The starting position.
    pos: u64,
    /// The accumulated content bytes.
    /// SmallVec avoids heap allocation for small inserts (most are 1-byte).
    /// 32 bytes inline = fits typical typing bursts without allocation.
    content: SmallVec<[u8; 32]>,
}

/// A pending delete operation waiting to be flushed.
#[derive(Clone, Debug)]
struct PendingDelete {
    /// The starting position of the delete range.
    start: u64,
    /// The length of the delete range.
    len: u64,
}

/// Pending operation type for RgaBuf.
#[derive(Clone, Debug)]
enum PendingOp {
    Insert(PendingInsert),
    Delete(PendingDelete),
}

/// A buffered wrapper around Rga that batches adjacent operations.
///
/// Text editing traces show strong locality: sequential typing inserts at
/// positions P, P+1, P+2, etc. By buffering these adjacent inserts and
/// applying them as a single operation, we can significantly reduce overhead.
///
/// This wrapper also optimizes:
/// - Backspace at end of pending insert: trim buffer instead of flush+delete
/// - Adjacent deletes (backspace): buffer deletes at P, P-1, P-2...
/// - Adjacent deletes (forward delete): buffer deletes at P, P, P...
///
/// JumpRopeBuf (used by diamond-types) achieves ~10x speedup for sequential
/// editing patterns using this technique.
///
/// Usage:
/// - Use `insert` and `delete` as normal
/// - Call `flush` before any read operation (len, to_string)
/// - The wrapper automatically flushes when switching between insert/delete
pub struct RgaBuf {
    /// The underlying RGA.
    rga: Rga,
    /// Pending operation, if any.
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
        // Build the ID index for fast lookups during merge
        self.rebuild_id_index();
        
        // Merge spans from other into self.
        // 
        // IMPORTANT: We must process spans in topological (causal) order.
        // The YATA algorithm requires that when we insert a span, its left_origin
        // already exists in the document. If we iterate in document order, a span's
        // origin might refer to a span that hasn't been inserted yet.
        //
        // Solution: Collect all new spans, then repeatedly scan and insert any
        // span whose origin either:
        // - Is None (no origin, inserted at beginning)
        // - Already exists in self (pre-existing or just inserted)
        
        // First pass: collect spans that need to be inserted and handle deletions
        // for spans that already exist.
        struct PendingSpan {
            span: Span,
            left_origin: Option<ItemId>,
            right_origin: Option<ItemId>,
        }
        
        let mut pending: Vec<PendingSpan> = Vec::new();
        
        for span in other.spans.iter() {
            // Get the user's KeyPub from other's UserTable
            let other_user = other.users.get_key(span.user_idx).unwrap();
            
            // Check which items from this span we already have.
            // We need to find the first item that doesn't exist in self.
            // 
            // Optimization: first check if the span START exists. If not,
            // the entire span is new. If yes, scan to find first missing.
            let first_item_id = ItemId {
                user: *other_user,
                seq: span.seq as u64,
            };
            
            let first_missing_offset = if self.find_span_by_id(&first_item_id).is_none() {
                // First item doesn't exist - entire span is new
                Some(0)
            } else {
                // First item exists - need to check the rest
                let mut first_missing: Option<u32> = None;
                for offset in 1..span.len {
                    let item_id = ItemId {
                        user: *other_user,
                        seq: (span.seq + offset) as u64,
                    };
                    if self.find_span_by_id(&item_id).is_none() {
                        first_missing = Some(offset);
                        break;
                    }
                }
                first_missing
            };
            
            // If all items exist, handle deletions and skip
            if first_missing_offset.is_none() {
                // Items already exist. Propagate deletions if needed.
                if span.deleted {
                    let user_idx = self.ensure_user(other_user);
                    for seq in span.seq..(span.seq + span.len) {
                        let item_id = ItemId {
                            user: *other_user,
                            seq: seq as u64,
                        };
                        if let Some(idx) = self.find_span_by_id(&item_id) {
                            let existing = self.spans.get(idx).unwrap();
                            if !existing.deleted && existing.user_idx == user_idx {
                                let offset = seq - existing.seq;
                                if existing.len == 1 {
                                    self.spans.get_mut(idx).unwrap().deleted = true;
                                    self.spans.update_weight(idx, 0);
                                } else if offset == 0 && existing.len > 1 {
                                    let mut existing = self.spans.remove(idx);
                                    let right = existing.split(1);
                                    existing.deleted = true;
                                    self.spans.insert(idx, existing, 0);
                                    self.spans.insert(idx + 1, right, right.visible_len() as u64);
                                } else if offset == existing.len - 1 {
                                    let mut existing = self.spans.remove(idx);
                                    let mut right = existing.split(offset);
                                    right.deleted = true;
                                    self.spans.insert(idx, existing, existing.visible_len() as u64);
                                    self.spans.insert(idx + 1, right, 0);
                                } else {
                                    let mut existing = self.spans.remove(idx);
                                    let mut mid_right = existing.split(offset);
                                    let right = mid_right.split(1);
                                    mid_right.deleted = true;
                                    self.spans.insert(idx, existing, existing.visible_len() as u64);
                                    self.spans.insert(idx + 1, mid_right, 0);
                                    self.spans.insert(idx + 2, right, right.visible_len() as u64);
                                }
                            }
                        }
                    }
                }
                continue;
            }

            // Some items are missing - we need to insert the missing portion.
            // first_missing_offset tells us where the missing items start.
            let missing_offset = first_missing_offset.unwrap();
            let missing_seq = span.seq + missing_offset;
            let missing_len = span.len - missing_offset;

            // If the source span is deleted and some items already exist in self,
            // we need to propagate the deletion for those existing items.
            // This handles the case where the source has a coalesced deleted span
            // but the dest only has part of it (the existing part should be deleted).
            if span.deleted && missing_offset > 0 {
                let user_idx = self.ensure_user(other_user);
                for offset in 0..missing_offset {
                    let seq = span.seq + offset;
                    let item_id = ItemId {
                        user: *other_user,
                        seq: seq as u64,
                    };
                    if let Some(idx) = self.find_span_by_id(&item_id) {
                        let existing = self.spans.get(idx).unwrap();
                        if !existing.deleted && existing.user_idx == user_idx {
                            let offset_in_existing = seq - existing.seq;
                            if existing.len == 1 {
                                self.spans.get_mut(idx).unwrap().deleted = true;
                                self.spans.update_weight(idx, 0);
                            } else if offset_in_existing == 0 && existing.len > 1 {
                                let mut existing = self.spans.remove(idx);
                                let right = existing.split(1);
                                existing.deleted = true;
                                self.spans.insert(idx, existing, 0);
                                self.spans.insert(idx + 1, right, right.visible_len() as u64);
                            } else if offset_in_existing == existing.len - 1 {
                                let mut existing = self.spans.remove(idx);
                                let mut right = existing.split(offset_in_existing);
                                right.deleted = true;
                                self.spans.insert(idx, existing, existing.visible_len() as u64);
                                self.spans.insert(idx + 1, right, 0);
                            } else {
                                let mut existing = self.spans.remove(idx);
                                let mut mid_right = existing.split(offset_in_existing);
                                let right = mid_right.split(1);
                                mid_right.deleted = true;
                                self.spans.insert(idx, existing, existing.visible_len() as u64);
                                self.spans.insert(idx + 1, mid_right, 0);
                                self.spans.insert(idx + 2, right, right.visible_len() as u64);
                            }
                        }
                    }
                }
            }

            // Get or create the user index in our table
            let user_idx = self.ensure_user(other_user);
            
            // Copy only the missing content from other's column to our column
            let other_column = &other.columns[span.user_idx as usize];
            let content = &other_column.content
                [(span.content_offset + missing_offset) as usize..(span.content_offset + span.len) as usize];
            
            let our_column = &mut self.columns[user_idx as usize];
            let content_offset = our_column.content.len() as u32;
            our_column.content.extend_from_slice(content);
            
            // Update next_seq if this span extends it
            let span_end_seq = span.seq + span.len;
            if span_end_seq > our_column.next_seq {
                our_column.next_seq = span_end_seq;
            }

            // Determine origins for the missing portion.
            // The left_origin of the missing portion is the last existing item (seq before missing_seq).
            let left_origin = if missing_offset > 0 {
                // The item just before the missing portion exists in self
                Some(ItemId {
                    user: *other_user,
                    seq: (missing_seq - 1) as u64,
                })
            } else if span.has_origin() {
                // Use the span's original left origin
                let origin_id = span.origin();
                let origin_user = other.users.get_key(origin_id.user_idx).unwrap();
                Some(ItemId {
                    user: *origin_user,
                    seq: origin_id.seq as u64,
                })
            } else {
                None
            };

            // Right origin: use the span's original right_origin for ALL items.
            // When items are coalesced, they share the same insertion context.
            // If user types "hello" coalesced into one span with right_origin=X,
            // then ALL of "hello" was inserted with X to the right. If we later
            // merge just "llo" (because "he" already exists), that "llo" should
            // still have right_origin=X.
            let right_origin = if span.has_right_origin() {
                let ro_id = span.right_origin();
                let ro_user = other.users.get_key(ro_id.user_idx).unwrap();
                Some(ItemId {
                    user: *ro_user,
                    seq: ro_id.seq as u64,
                })
            } else {
                None
            };

            // Create a new span for just the missing items
            let mut new_span = Span::new(
                user_idx,
                missing_seq,
                missing_len,
                OriginId::none(),
                OriginId::none(),
                content_offset,
            );
            new_span.deleted = span.deleted;
            
            pending.push(PendingSpan {
                span: new_span,
                left_origin,
                right_origin,
            });
        }
        
        // Second pass: insert spans in topological order.
        // Repeatedly scan pending, inserting any span whose left_origin exists.
        // This is O(n^2) in the worst case but simple and correct.
        // 
        // IMPORTANT: We use remove(i) instead of swap_remove(i) to preserve
        // the relative order of spans. This ensures that spans from the same
        // document maintain their document order when inserted.
        while !pending.is_empty() {
            let mut made_progress = false;
            
            // Rebuild the ID index before each round for fast origin lookups
            self.rebuild_id_index();
            
            let mut i = 0;
            while i < pending.len() {
                let can_insert = {
                    let p = &pending[i];
                    match &p.left_origin {
                        None => true, // No origin - can always insert
                        Some(origin_id) => {
                            // Check if origin exists in self
                            self.find_span_by_id(origin_id).is_some()
                        }
                    }
                };
                
                if can_insert {
                    let p = pending.remove(i);
                    self.insert_span_rga(p.span, p.left_origin, p.right_origin);
                    made_progress = true;
                    // Don't increment i since remove shifted elements down
                } else {
                    i += 1;
                }
            }
            
            if !made_progress && !pending.is_empty() {
                // This shouldn't happen if the data is valid (no cycles in origin chains)
                panic!("merge: circular dependency in origin chains");
            }
        }
        
        // Invalidate cursor cache since we modified the structure
        self.cursor_cache.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::KeyPair;

    #[test]
    fn span_size() {
        // Verify our span is compact
        // With dual origins (left + right), span is 30 bytes:
        // - seq: u32 (4)
        // - len: u32 (4)
        // - origin_user_idx: u16 (2)
        // - origin_seq: u32 (4)
        // - right_origin_user_idx: u16 (2)
        // - right_origin_seq: u32 (4)
        // - content_offset: u32 (4)
        // - user_idx: u16 (2)
        // - deleted: bool (1)
        // - _padding: u8 (1)
        // Total: 28 bytes, but with alignment may be 32
        let size = std::mem::size_of::<Span>();
        assert!(size <= 32, "Span is {} bytes, expected <= 32", size);
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

        let block = OpBlock::insert(None, None, 0, b"hello".to_vec());
        let applied = rga.apply(&pair.key_pub, &block);

        assert!(applied);
        assert_eq!(rga.to_string(), "hello");
    }

    #[test]
    fn apply_insert_after_existing() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();

        let block1 = OpBlock::insert(None, None, 0, b"hello".to_vec());
        rga.apply(&pair.key_pub, &block1);

        let origin = OpItemId {
            user: pair.key_pub,
            seq: 4,
        };
        let block2 = OpBlock::insert(Some(origin), None, 5, b" world".to_vec());
        rga.apply(&pair.key_pub, &block2);

        assert_eq!(rga.to_string(), "hello world");
    }

    #[test]
    fn apply_idempotent() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();

        let block = OpBlock::insert(None, None, 0, b"hello".to_vec());

        assert!(rga.apply(&pair.key_pub, &block));
        assert!(!rga.apply(&pair.key_pub, &block));

        assert_eq!(rga.to_string(), "hello");
    }

    #[test]
    fn apply_delete() {
        let pair = KeyPair::generate();
        let mut rga = Rga::new();

        let block1 = OpBlock::insert(None, None, 0, b"hello".to_vec());
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

        let block_a = OpBlock::insert(None, None, 0, b"A".to_vec());
        let block_b = OpBlock::insert(None, None, 0, b"B".to_vec());

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
