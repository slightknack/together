// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

//! Span representations for RGA implementations.
//!
//! A span represents a contiguous run of characters from the same user.
//! Different implementations may need different tradeoffs:
//!
//! - `CompactSpan`: Minimal memory (for large documents)
//! - `RichSpan`: Additional metadata (for debugging/analysis)
//!
//! # Memory Layout
//!
//! CompactSpan is designed for cache-friendly access:
//! - 32 bytes total (fits in half a cache line)
//! - Most frequently accessed fields at the start
//! - Padding explicit for predictable layout

use super::id::{CompactOpId, UserIdx};

/// A compact span for memory-efficient storage.
///
/// Represents a contiguous run of characters from the same user.
/// Uses 32 bytes per span.
///
/// Layout (32 bytes):
/// ```text
/// [0..4]   seq: u32           - Starting sequence number
/// [4..8]   len: u32           - Number of characters
/// [8..10]  user_idx: u16      - User index
/// [10..11] flags: u8          - Deleted flag + reserved
/// [11..12] _pad1: u8          - Padding
/// [12..16] content_off: u32   - Offset into content buffer
/// [16..20] left_seq: u32      - Left origin seq
/// [20..22] left_user: u16     - Left origin user
/// [22..24] _pad2: u16         - Padding
/// [24..28] right_seq: u32     - Right origin seq
/// [28..30] right_user: u16    - Right origin user
/// [30..32] _pad3: u16         - Padding
/// ```
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct CompactSpan {
    /// Starting sequence number for this span.
    pub seq: u32,
    /// Number of characters in this span.
    pub len: u32,
    /// User index (into a user table).
    pub user_idx: u16,
    /// Flags byte: bit 0 = deleted.
    flags: u8,
    /// Padding.
    _pad1: u8,
    /// Offset into the user's content buffer.
    pub content_offset: u32,
    /// Left origin sequence number.
    left_origin_seq: u32,
    /// Left origin user index.
    left_origin_user: u16,
    /// Padding.
    _pad2: u16,
    /// Right origin sequence number.
    right_origin_seq: u32,
    /// Right origin user index.
    right_origin_user: u16,
    /// Padding.
    _pad3: u16,
}

const FLAG_DELETED: u8 = 0x01;

impl CompactSpan {
    /// Create a new span.
    pub fn new(
        user_idx: u16,
        seq: u32,
        len: u32,
        content_offset: u32,
        left_origin: CompactOpId,
        right_origin: CompactOpId,
    ) -> CompactSpan {
        return CompactSpan {
            seq,
            len,
            user_idx,
            flags: 0,
            _pad1: 0,
            content_offset,
            left_origin_seq: left_origin.seq,
            left_origin_user: left_origin.user_idx.0,
            _pad2: 0,
            right_origin_seq: right_origin.seq,
            right_origin_user: right_origin.user_idx.0,
            _pad3: 0,
        };
    }

    /// Check if this span is deleted.
    #[inline]
    pub fn is_deleted(&self) -> bool {
        return (self.flags & FLAG_DELETED) != 0;
    }

    /// Mark this span as deleted.
    #[inline]
    pub fn set_deleted(&mut self, deleted: bool) {
        if deleted {
            self.flags |= FLAG_DELETED;
        } else {
            self.flags &= !FLAG_DELETED;
        }
    }

    /// Get the visible length (0 if deleted).
    #[inline]
    pub fn visible_len(&self) -> u32 {
        if self.is_deleted() {
            return 0;
        }
        return self.len;
    }

    /// Check if this span contains the given sequence number.
    #[inline]
    pub fn contains_seq(&self, seq: u32) -> bool {
        return seq >= self.seq && seq < self.seq + self.len;
    }

    /// Get the left origin.
    #[inline]
    pub fn left_origin(&self) -> CompactOpId {
        return CompactOpId::new(UserIdx(self.left_origin_user), self.left_origin_seq);
    }

    /// Set the left origin.
    #[inline]
    pub fn set_left_origin(&mut self, origin: CompactOpId) {
        self.left_origin_user = origin.user_idx.0;
        self.left_origin_seq = origin.seq;
    }

    /// Check if this span has a left origin.
    #[inline]
    pub fn has_left_origin(&self) -> bool {
        return self.left_origin_user != u16::MAX;
    }

    /// Get the right origin.
    #[inline]
    pub fn right_origin(&self) -> CompactOpId {
        return CompactOpId::new(UserIdx(self.right_origin_user), self.right_origin_seq);
    }

    /// Set the right origin.
    #[inline]
    pub fn set_right_origin(&mut self, origin: CompactOpId) {
        self.right_origin_user = origin.user_idx.0;
        self.right_origin_seq = origin.seq;
    }

    /// Check if this span has a right origin.
    #[inline]
    pub fn has_right_origin(&self) -> bool {
        return self.right_origin_user != u16::MAX;
    }

    /// Split this span at the given offset, returning the right part.
    ///
    /// After split:
    /// - `self` contains [0, offset)
    /// - returned span contains [offset, len)
    ///
    /// The right part's left origin is set to the last character of the left part,
    /// which is semantically correct for RGA.
    pub fn split(&mut self, offset: u32) -> CompactSpan {
        debug_assert!(offset > 0 && offset < self.len);

        let right = CompactSpan {
            seq: self.seq + offset,
            len: self.len - offset,
            user_idx: self.user_idx,
            flags: self.flags,
            _pad1: 0,
            content_offset: self.content_offset + offset,
            // Right part's left origin is the last char of left part
            left_origin_seq: self.seq + offset - 1,
            left_origin_user: self.user_idx,
            _pad2: 0,
            // Right origin stays the same
            right_origin_seq: self.right_origin_seq,
            right_origin_user: self.right_origin_user,
            _pad3: 0,
        };

        self.len = offset;
        return right;
    }

    /// Check if this span can be coalesced with another.
    ///
    /// Two spans can be coalesced if:
    /// - Same user
    /// - Consecutive sequence numbers
    /// - Contiguous content offsets
    /// - Same deleted state
    pub fn can_coalesce(&self, next: &CompactSpan) -> bool {
        return self.user_idx == next.user_idx
            && self.seq + self.len == next.seq
            && self.content_offset + self.len == next.content_offset
            && self.is_deleted() == next.is_deleted();
    }

    /// Coalesce with the next span (extend this span).
    pub fn coalesce(&mut self, next: &CompactSpan) {
        debug_assert!(self.can_coalesce(next));
        self.len += next.len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_size() {
        assert_eq!(std::mem::size_of::<CompactSpan>(), 32);
    }

    #[test]
    fn span_deleted() {
        let mut span = CompactSpan::new(0, 0, 10, 0, CompactOpId::none(), CompactOpId::none());

        assert!(!span.is_deleted());
        assert_eq!(span.visible_len(), 10);

        span.set_deleted(true);
        assert!(span.is_deleted());
        assert_eq!(span.visible_len(), 0);

        span.set_deleted(false);
        assert!(!span.is_deleted());
        assert_eq!(span.visible_len(), 10);
    }

    #[test]
    fn span_contains_seq() {
        let span = CompactSpan::new(0, 10, 5, 0, CompactOpId::none(), CompactOpId::none());

        assert!(!span.contains_seq(9));
        assert!(span.contains_seq(10));
        assert!(span.contains_seq(12));
        assert!(span.contains_seq(14));
        assert!(!span.contains_seq(15));
    }

    #[test]
    fn span_origins() {
        let left = CompactOpId::new(UserIdx::new(1), 5);
        let right = CompactOpId::new(UserIdx::new(2), 10);

        let span = CompactSpan::new(0, 0, 5, 0, left, right);

        assert!(span.has_left_origin());
        assert!(span.has_right_origin());
        assert_eq!(span.left_origin(), left);
        assert_eq!(span.right_origin(), right);
    }

    #[test]
    fn span_no_origins() {
        let span = CompactSpan::new(0, 0, 5, 0, CompactOpId::none(), CompactOpId::none());

        assert!(!span.has_left_origin());
        assert!(!span.has_right_origin());
    }

    #[test]
    fn span_split() {
        let left_origin = CompactOpId::new(UserIdx::new(1), 5);
        let right_origin = CompactOpId::new(UserIdx::new(2), 10);
        let mut span = CompactSpan::new(0, 100, 10, 50, left_origin, right_origin);

        let right = span.split(4);

        // Left part
        assert_eq!(span.seq, 100);
        assert_eq!(span.len, 4);
        assert_eq!(span.content_offset, 50);

        // Right part
        assert_eq!(right.seq, 104);
        assert_eq!(right.len, 6);
        assert_eq!(right.content_offset, 54);

        // Right part's left origin should be last char of left part
        assert_eq!(right.left_origin().user_idx, UserIdx::new(0)); // Same user
        assert_eq!(right.left_origin().seq, 103); // seq 100 + offset 4 - 1

        // Right origin preserved
        assert_eq!(right.right_origin(), right_origin);
    }

    #[test]
    fn span_coalesce() {
        let mut a = CompactSpan::new(0, 100, 5, 0, CompactOpId::none(), CompactOpId::none());
        let b = CompactSpan::new(0, 105, 3, 5, CompactOpId::none(), CompactOpId::none());

        assert!(a.can_coalesce(&b));
        a.coalesce(&b);

        assert_eq!(a.len, 8);
    }

    #[test]
    fn span_cannot_coalesce_different_user() {
        let a = CompactSpan::new(0, 100, 5, 0, CompactOpId::none(), CompactOpId::none());
        let b = CompactSpan::new(1, 105, 3, 5, CompactOpId::none(), CompactOpId::none());

        assert!(!a.can_coalesce(&b));
    }

    #[test]
    fn span_cannot_coalesce_gap() {
        let a = CompactSpan::new(0, 100, 5, 0, CompactOpId::none(), CompactOpId::none());
        let b = CompactSpan::new(0, 110, 3, 5, CompactOpId::none(), CompactOpId::none());

        assert!(!a.can_coalesce(&b));
    }

    #[test]
    fn span_cannot_coalesce_different_deleted() {
        let mut a = CompactSpan::new(0, 100, 5, 0, CompactOpId::none(), CompactOpId::none());
        let b = CompactSpan::new(0, 105, 3, 5, CompactOpId::none(), CompactOpId::none());

        a.set_deleted(true);
        assert!(!a.can_coalesce(&b));
    }
}
