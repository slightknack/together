// model = "claude-opus-4-5"
// created = "2026-01-30"
// modified = "2026-01-30"
// driver = "Isaac Clayton"

//! Span list with O(log n) position lookups via prefix sums.
//!
//! Uses a simple Vec of spans with a parallel prefix sum array that enables
//! binary search for position lookups. Insertions are still O(n) but the
//! constant factor is much lower than rebuilding a HashMap index.
//!
//! This is a stepping stone - a proper skip list or B-tree would give O(log n)
//! insertions, but this simpler structure lets us test the benefit of O(log n)
//! lookups first.

use super::rga::Span;

/// A list of spans with O(log n) position lookup via binary search on prefix sums.
pub struct SpanList {
    /// Spans in document order.
    spans: Vec<Span>,
    /// Prefix sums of visible lengths: prefix[i] = sum of visible_len for spans[0..i].
    /// prefix[0] = 0, prefix[spans.len()] = total visible length.
    prefix: Vec<u64>,
}

impl SpanList {
    /// Create a new empty span list.
    pub fn new() -> SpanList {
        return SpanList {
            spans: Vec::new(),
            prefix: vec![0],
        };
    }

    /// Number of spans.
    pub fn len(&self) -> usize {
        return self.spans.len();
    }

    /// Total visible character count.
    pub fn visible_len(&self) -> u64 {
        return *self.prefix.last().unwrap();
    }

    /// Get a span by index.
    pub fn get(&self, idx: usize) -> Option<&Span> {
        return self.spans.get(idx);
    }

    /// Get a mutable span by index.
    pub fn get_mut(&mut self, idx: usize) -> Option<&mut Span> {
        return self.spans.get_mut(idx);
    }

    /// Find the span containing the given visible position.
    /// Returns (span_index, offset_within_span).
    /// Uses binary search on prefix sums for O(log n) lookup.
    pub fn find_visible_pos(&self, pos: u64) -> Option<(usize, u64)> {
        if pos >= self.visible_len() {
            return None;
        }

        // Binary search for the span containing pos.
        // We want the largest i such that prefix[i] <= pos and span[i] is visible.
        let mut lo = 0;
        let mut hi = self.spans.len();

        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.prefix[mid + 1] <= pos {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }

        // lo is now the index of the span containing pos (if visible)
        // but we might have landed on a deleted span, need to scan forward
        while lo < self.spans.len() {
            let span = &self.spans[lo];
            if !span.deleted {
                let start_pos = self.prefix[lo];
                if pos < start_pos + span.len {
                    return Some((lo, pos - start_pos));
                }
            }
            lo += 1;
        }

        return None;
    }

    /// Insert a span at the given index, updating prefix sums.
    pub fn insert(&mut self, idx: usize, span: Span) {
        let visible = span.visible_len();
        self.spans.insert(idx, span);

        // Update prefix sums: insert new entry and shift all after
        let prev_sum = self.prefix[idx];
        self.prefix.insert(idx + 1, prev_sum + visible);

        // Update all subsequent prefix sums
        for i in (idx + 2)..self.prefix.len() {
            self.prefix[i] += visible;
        }
    }

    /// Push a span at the end.
    pub fn push(&mut self, span: Span) {
        let visible = span.visible_len();
        let prev_sum = *self.prefix.last().unwrap();
        self.spans.push(span);
        self.prefix.push(prev_sum + visible);
    }

    /// Mark a span as deleted and update prefix sums.
    pub fn mark_deleted(&mut self, idx: usize) {
        let span = &mut self.spans[idx];
        if span.deleted {
            return;
        }
        let visible = span.visible_len();
        span.deleted = true;

        // Update all subsequent prefix sums
        for i in (idx + 1)..self.prefix.len() {
            self.prefix[i] -= visible;
        }
    }

    /// Iterate over spans.
    pub fn iter(&self) -> impl Iterator<Item = &Span> {
        return self.spans.iter();
    }

    /// Iterate mutably over spans.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Span> {
        return self.spans.iter_mut();
    }

    /// Get the visible position where a span starts.
    pub fn span_start_pos(&self, idx: usize) -> u64 {
        return self.prefix[idx];
    }

    /// Rebuild prefix sums from scratch (after structural changes).
    pub fn rebuild_prefix(&mut self) {
        self.prefix.clear();
        self.prefix.push(0);
        let mut sum = 0u64;
        for span in &self.spans {
            sum += span.visible_len();
            self.prefix.push(sum);
        }
    }
}

impl Default for SpanList {
    fn default() -> Self {
        return Self::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::KeyPair;

    fn make_span(seq: u64, len: u64, deleted: bool) -> Span {
        let pair = KeyPair::generate();
        return Span {
            user: pair.key_pub,
            seq,
            len,
            origin: None,
            content_offset: 0,
            deleted,
        };
    }

    #[test]
    fn empty_list() {
        let list = SpanList::new();
        assert_eq!(list.len(), 0);
        assert_eq!(list.visible_len(), 0);
    }

    #[test]
    fn push_one() {
        let mut list = SpanList::new();
        list.push(make_span(0, 5, false));
        assert_eq!(list.len(), 1);
        assert_eq!(list.visible_len(), 5);
    }

    #[test]
    fn find_pos_single_span() {
        let mut list = SpanList::new();
        list.push(make_span(0, 10, false));

        for i in 0..10 {
            let result = list.find_visible_pos(i);
            assert_eq!(result, Some((0, i)));
        }
        assert_eq!(list.find_visible_pos(10), None);
    }

    #[test]
    fn find_pos_multiple_spans() {
        let mut list = SpanList::new();
        list.push(make_span(0, 5, false));  // pos 0-4
        list.push(make_span(5, 3, false));  // pos 5-7
        list.push(make_span(8, 7, false));  // pos 8-14

        assert_eq!(list.find_visible_pos(0), Some((0, 0)));
        assert_eq!(list.find_visible_pos(4), Some((0, 4)));
        assert_eq!(list.find_visible_pos(5), Some((1, 0)));
        assert_eq!(list.find_visible_pos(7), Some((1, 2)));
        assert_eq!(list.find_visible_pos(8), Some((2, 0)));
        assert_eq!(list.find_visible_pos(14), Some((2, 6)));
        assert_eq!(list.find_visible_pos(15), None);
    }

    #[test]
    fn find_pos_with_deleted() {
        let mut list = SpanList::new();
        list.push(make_span(0, 5, false));  // visible
        list.push(make_span(5, 3, true));   // deleted
        list.push(make_span(8, 7, false));  // visible

        assert_eq!(list.visible_len(), 12);
        assert_eq!(list.find_visible_pos(5), Some((2, 0)));
        assert_eq!(list.find_visible_pos(11), Some((2, 6)));
    }

    #[test]
    fn insert_middle() {
        let mut list = SpanList::new();
        list.push(make_span(0, 5, false));
        list.push(make_span(10, 5, false));
        list.insert(1, make_span(5, 3, false));

        assert_eq!(list.len(), 3);
        assert_eq!(list.visible_len(), 13);
        assert_eq!(list.find_visible_pos(5), Some((1, 0)));
        assert_eq!(list.find_visible_pos(8), Some((2, 0)));
    }

    #[test]
    fn mark_deleted() {
        let mut list = SpanList::new();
        list.push(make_span(0, 5, false));
        list.push(make_span(5, 3, false));

        assert_eq!(list.visible_len(), 8);
        list.mark_deleted(0);
        assert_eq!(list.visible_len(), 3);
        assert_eq!(list.find_visible_pos(0), Some((1, 0)));
    }
}
