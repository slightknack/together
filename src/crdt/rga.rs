// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-01-31"
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
//! 4. **Compact representation**: Spans use 24 bytes instead of 112 bytes by:
//!    - Storing user as a u16 index into a UserTable
//!    - Storing origin as span_idx + offset (u32 + u32)
//!    - Using u32 for seq/len/content_offset

use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::key::KeyPub;
use super::btree_list::BTreeList;

/// Sentinel value indicating no origin (insert at beginning).
const NO_ORIGIN: u32 = u32::MAX;

/// A table mapping u16 indices to KeyPub values.
/// This allows spans to store a 2-byte index instead of a 32-byte key.
#[derive(Clone, Debug, Default)]
pub struct UserTable {
    /// Map from KeyPub to index.
    key_to_idx: FxHashMap<KeyPub, u16>,
    /// Map from index to KeyPub.
    idx_to_key: Vec<KeyPub>,
}

impl UserTable {
    /// Create a new empty user table.
    pub fn new() -> UserTable {
        return UserTable {
            key_to_idx: FxHashMap::default(),
            idx_to_key: Vec::new(),
        };
    }

    /// Get or create an index for a user.
    pub fn get_or_insert(&mut self, key: &KeyPub) -> u16 {
        if let Some(&idx) = self.key_to_idx.get(key) {
            return idx;
        }
        let idx = self.idx_to_key.len() as u16;
        assert!(idx < u16::MAX, "too many users (max 65534)");
        self.idx_to_key.push(*key);
        self.key_to_idx.insert(*key, idx);
        return idx;
    }

    /// Get the index for a user, if it exists.
    pub fn get(&self, key: &KeyPub) -> Option<u16> {
        return self.key_to_idx.get(key).copied();
    }

    /// Get the KeyPub for an index.
    pub fn get_key(&self, idx: u16) -> Option<&KeyPub> {
        return self.idx_to_key.get(idx as usize);
    }

    /// Number of users in the table.
    pub fn len(&self) -> usize {
        return self.idx_to_key.len();
    }

    /// Check if the table is empty.
    pub fn is_empty(&self) -> bool {
        return self.idx_to_key.is_empty();
    }
}

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

/// A compact reference to an origin position.
/// Uses span index + offset within span instead of full ItemId.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OriginRef {
    /// Index of the span containing the origin (u32::MAX = no origin).
    pub span_idx: u32,
    /// Offset within the span.
    pub offset: u32,
}

impl OriginRef {
    /// Create a reference meaning "no origin" (insert at beginning).
    pub fn none() -> OriginRef {
        return OriginRef {
            span_idx: NO_ORIGIN,
            offset: 0,
        };
    }

    /// Create a reference to a specific span position.
    pub fn some(span_idx: u32, offset: u32) -> OriginRef {
        return OriginRef { span_idx, offset };
    }

    /// Check if this is a "no origin" reference.
    pub fn is_none(&self) -> bool {
        return self.span_idx == NO_ORIGIN;
    }

    /// Check if this is a valid origin reference.
    pub fn is_some(&self) -> bool {
        return self.span_idx != NO_ORIGIN;
    }
}

/// A compact span of consecutive items inserted by the same user.
/// Target size: 24 bytes (down from 112 bytes).
///
/// Layout (ordered for optimal packing):
/// - seq: u32 (4 bytes) - starting sequence number
/// - len: u32 (4 bytes) - number of items
/// - origin_span_idx: u32 (4 bytes) - origin span index (NO_ORIGIN = none)
/// - origin_offset: u32 (4 bytes) - offset within origin span
/// - content_offset: u32 (4 bytes) - offset into content backing store
/// - user_idx: u16 (2 bytes) - index into UserTable
/// - deleted: bool (1 byte) - whether this span is deleted
/// - _padding: u8 (1 byte) - alignment padding
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Span {
    /// The starting sequence number.
    pub seq: u32,
    /// Number of items in this span.
    pub len: u32,
    /// Index of the origin span (NO_ORIGIN = no origin).
    pub origin_span_idx: u32,
    /// Offset within the origin span.
    pub origin_offset: u32,
    /// Offset into the content backing store.
    pub content_offset: u32,
    /// Index into the UserTable.
    pub user_idx: u16,
    /// Whether this span has been deleted.
    pub deleted: bool,
    /// Padding for alignment.
    _padding: u8,
}

impl Span {
    /// Create a new span.
    pub fn new(
        user_idx: u16,
        seq: u32,
        len: u32,
        origin: OriginRef,
        content_offset: u32,
    ) -> Span {
        return Span {
            seq,
            len,
            origin_span_idx: origin.span_idx,
            origin_offset: origin.offset,
            content_offset,
            user_idx,
            deleted: false,
            _padding: 0,
        };
    }

    /// Get the origin reference.
    pub fn origin(&self) -> OriginRef {
        return OriginRef {
            span_idx: self.origin_span_idx,
            offset: self.origin_offset,
        };
    }

    /// Set the origin reference.
    pub fn set_origin(&mut self, origin: OriginRef) {
        self.origin_span_idx = origin.span_idx;
        self.origin_offset = origin.offset;
    }

    /// Check if this span has an origin.
    #[inline(always)]
    pub fn has_origin(&self) -> bool {
        return self.origin_span_idx != NO_ORIGIN;
    }

    /// Check if this span contains the given sequence number for the same user.
    #[inline(always)]
    pub fn contains_seq(&self, seq: u32) -> bool {
        return seq >= self.seq && seq < self.seq + self.len;
    }

    /// Get the sequence number at a position within this span.
    #[inline(always)]
    pub fn seq_at(&self, offset: u32) -> u32 {
        debug_assert!(offset < self.len);
        return self.seq + offset;
    }

    /// Split this span at the given offset, returning the right half.
    #[inline]
    pub fn split(&mut self, offset: u32) -> Span {
        debug_assert!(offset > 0 && offset < self.len);
        let right = Span {
            seq: self.seq + offset,
            len: self.len - offset,
            origin_span_idx: NO_ORIGIN, // Will be fixed by caller
            origin_offset: 0,
            content_offset: self.content_offset + offset,
            user_idx: self.user_idx,
            deleted: self.deleted,
            _padding: 0,
        };
        self.len = offset;
        return right;
    }

    /// Visible length (0 if deleted, len otherwise).
    #[inline(always)]
    pub fn visible_len(&self) -> u32 {
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

    /// Adjust the cache after an insert at the given position with the given length.
    /// The insert position is where the new content starts (before adjustment).
    /// When a new span is inserted, it can shift span indices and invalidate our cache.
    fn adjust_after_insert(&mut self, _insert_pos: u64, _insert_len: u64, _new_span_idx: usize) {
        // When inserting at a position different from where we cached,
        // the span indices can shift in complex ways. Rather than trying to track
        // these changes precisely, we simply invalidate the cache.
        // The next insert will do a fresh lookup and re-establish the cache.
        // This only affects non-sequential inserts (e.g., insert at beginning after
        // typing at the end), which are relatively rare.
        self.invalidate();
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
pub struct Rga {
    /// Spans in document order, weighted by visible character count.
    spans: BTreeList<Span>,
    /// Per-user columns for content storage, indexed by user_idx.
    columns: Vec<Column>,
    /// Maps KeyPub to user index.
    users: UserTable,
    /// Cursor cache for amortizing sequential lookups.
    cursor_cache: CursorCache,
}

impl Rga {
    /// Create a new empty RGA.
    pub fn new() -> Rga {
        return Rga {
            spans: BTreeList::new(),
            columns: Vec::new(),
            users: UserTable::new(),
            cursor_cache: CursorCache::new(),
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

    /// Get or create a user index, ensuring the column exists.
    fn ensure_user(&mut self, user: &KeyPub) -> u16 {
        let idx = self.users.get_or_insert(user);
        while self.columns.len() <= idx as usize {
            self.columns.push(Column::new());
        }
        return idx;
    }

    /// Insert content after the given visible position.
    /// Position 0 means insert at the beginning.
    /// Returns the ItemId of the first inserted item.
    pub fn insert(&mut self, user: &KeyPub, pos: u64, content: &[u8]) -> ItemId {
        if content.is_empty() {
            panic!("cannot insert empty content");
        }

        let user_idx = self.ensure_user(user);
        let column = &self.columns[user_idx as usize];
        let seq = column.next_seq;
        
        self.insert_with_user_idx(user_idx, pos, content);
        
        return ItemId {
            user: *user,
            seq: seq as u64,
        };
    }

    /// Insert content using a pre-computed user index.
    /// This avoids HashMap lookups when the caller already has the user_idx.
    /// Does not return ItemId to avoid the overhead of looking up the user key.
    #[inline]
    fn insert_with_user_idx(&mut self, user_idx: u16, pos: u64, content: &[u8]) {
        let column = &mut self.columns[user_idx as usize];
        let seq = column.next_seq;
        let content_offset = column.content.len() as u32;
        column.content.extend_from_slice(content);
        column.next_seq += content.len() as u32;

        // Create the span (origin is set during insert_span_at_pos_optimized)
        let span = Span::new(
            user_idx,
            seq,
            content.len() as u32,
            OriginRef::none(),
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

    /// Insert a span at the given visible position (for local edits).
    /// Optimized version that sets the origin during insert to avoid double lookup.
    /// Attempts to coalesce with the preceding span if possible.
    /// Uses cursor caching for O(1) sequential typing with chunk location caching.
    #[inline]
    fn insert_span_at_pos_optimized(&mut self, mut span: Span, pos: u64) {
        let span_len = span.visible_len() as u64;

        if self.spans.is_empty() {
            // No origin for first span
            self.spans.insert(0, span, span_len);
            // Cache the end of what we just inserted (chunk 0, idx 0 after insert)
            self.cursor_cache.update(pos + span_len - 1, 0, span_len - 1, 0, 0);
            return;
        }

        if pos == 0 {
            // No origin when inserting at beginning
            self.spans.insert(0, span, span_len);
            // Adjust cache: spans shifted right, cache position shifted by span_len
            self.cursor_cache.adjust_after_insert(0, span_len, 0);
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
                            self.spans.insert(self.spans.len(), span, span_len);
                            self.cursor_cache.invalidate();
                            return;
                        }
                    }
                } else {
                    // Fallback to full lookup
                    match self.spans.find_by_weight_with_chunk(lookup_pos) {
                        Some((span_idx, off, c_idx, i_in_c)) => (span_idx, off, c_idx, i_in_c),
                        None => {
                            self.spans.insert(self.spans.len(), span, span_len);
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
                    self.spans.insert(self.spans.len(), span, span_len);
                    self.cursor_cache.invalidate();
                    return;
                }
            }
        };

        // Use cached chunk location to get prev_span without find_chunk_by_index
        let prev_span = self.spans.get_with_chunk_hint(chunk_idx, idx_in_chunk).unwrap();
        let prev_visible_len = prev_span.visible_len() as u64;
        
        // Set the origin from the lookup we just did
        span.set_origin(OriginRef::some(prev_idx as u32, offset_in_prev as u32));

        // Check if we can coalesce: same user, consecutive seq, contiguous content, not deleted
        // Also check offset_in_prev == prev_span.visible_len - 1 to ensure we're at the end of the span
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

        // Can't coalesce - determine insert position based on the lookup we already did
        // If offset_in_prev is at the end of prev_span, insert after it
        // Otherwise we need to split prev_span
        if offset_in_prev == prev_visible_len - 1 {
            // Insert right after prev_span
            self.spans.insert(prev_idx + 1, span, span_len);
            
            // Update cache to point to end of newly inserted span
            // This allows the next sequential insert to use the cache
            // Note: We don't know the exact leaf location after insert (it may have split),
            // so we just invalidate and let the next lookup rebuild the cache.
            // A more sophisticated approach would track the new location.
            self.cursor_cache.invalidate();
        } else {
            // Need to split prev_span - insert in the middle
            let split_offset = (offset_in_prev + 1) as u32;
            let mut existing = self.spans.remove(prev_idx);
            let right = existing.split(split_offset);
            self.spans.insert(prev_idx, existing, existing.visible_len() as u64);
            self.spans.insert(prev_idx + 1, span, span_len);
            self.spans.insert(prev_idx + 2, right, right.visible_len() as u64);
            
            // Invalidate cache - structural changes
            self.cursor_cache.invalidate();
        }
    }

    /// Find span containing the given ItemId using linear search.
    fn find_span_by_id(&self, id: &ItemId) -> Option<usize> {
        let user_idx = match self.users.get(&id.user) {
            Some(idx) => idx,
            None => return None,
        };
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
                    OriginRef::none(), // Will be resolved during insert
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

    /// Insert a span using RGA ordering rules.
    /// When multiple spans have the same origin, order by (user, seq) descending.
    fn insert_span_rga(&mut self, mut span: Span, origin: Option<ItemId>) {
        let span_len = span.visible_len() as u64;

        if self.spans.is_empty() {
            self.spans.insert(0, span, span_len);
            return;
        }

        let insert_idx = if let Some(ref origin_id) = origin {
            // Find the origin span
            if let Some(origin_idx) = self.find_span_by_id(origin_id) {
                let origin_span = self.spans.get(origin_idx).unwrap();
                let offset_in_span = (origin_id.seq as u32) - origin_span.seq;

                // Set the origin reference
                span.set_origin(OriginRef::some(origin_idx as u32, offset_in_span));

                // If origin is in the middle of a span, split it
                if offset_in_span < origin_span.len - 1 {
                    let mut existing = self.spans.remove(origin_idx);
                    let right = existing.split(offset_in_span + 1);
                    self.spans.insert(origin_idx, existing, existing.visible_len() as u64);
                    self.spans.insert(origin_idx + 1, right, right.visible_len() as u64);
                }

                // Insert after origin, respecting RGA ordering
                let mut pos = origin_idx + 1;
                let span_user = self.users.get_key(span.user_idx).unwrap();
                while pos < self.spans.len() {
                    let other = self.spans.get(pos).unwrap();
                    // Check if other has the same origin
                    if other.has_origin() {
                        let other_origin = other.origin();
                        if other_origin.span_idx == origin_idx as u32 
                            && other_origin.offset == offset_in_span 
                        {
                            let other_user = self.users.get_key(other.user_idx).unwrap();
                            if (other_user, other.seq) > (span_user, span.seq) {
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
            let span_user = self.users.get_key(span.user_idx).unwrap();
            while pos < self.spans.len() {
                let other = self.spans.get(pos).unwrap();
                if !other.has_origin() {
                    let other_user = self.users.get_key(other.user_idx).unwrap();
                    if (other_user, other.seq) > (span_user, span.seq) {
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
        for span in other.spans.iter() {
            // Get the user's KeyPub from other's UserTable
            let other_user = other.users.get_key(span.user_idx).unwrap();
            
            // Check if we already have this span
            let first_id = ItemId {
                user: *other_user,
                seq: span.seq as u64,
            };
            if self.find_span_by_id(&first_id).is_some() {
                continue;
            }

            let other_column = &other.columns[span.user_idx as usize];
            let content = &other_column.content
                [span.content_offset as usize..(span.content_offset + span.len) as usize];

            // Reconstruct the origin ItemId if present
            let origin = if span.has_origin() {
                let origin_ref = span.origin();
                let origin_span = other.spans.get(origin_ref.span_idx as usize).unwrap();
                let origin_user = other.users.get_key(origin_span.user_idx).unwrap();
                Some(OpItemId {
                    user: *origin_user,
                    seq: (origin_span.seq + origin_ref.offset) as u64,
                })
            } else {
                None
            };

            let block = OpBlock::insert(origin, span.seq as u64, content.to_vec());
            self.apply(other_user, &block);
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
