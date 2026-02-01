// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

//! Cursor caching for amortizing sequential lookups.
//!
//! Text editing has strong locality: sequential typing inserts at pos+1,
//! backspace deletes at pos-1. By caching the last lookup result, we can
//! scan from the cached position instead of doing a full O(log n) lookup.
//!
//! For sequential typing, this turns O(log n) per insert into O(1) amortized.
//!
//! # Usage Patterns
//!
//! ## Sequential Forward (Typing)
//! Inserts at positions P, P+1, P+2, ...
//! Cache hit: scan forward 1 position
//!
//! ## Sequential Backward (Backspace)
//! Deletes at positions P, P-1, P-2, ...
//! Cache hit: scan backward 1 position
//!
//! ## Random Access
//! Cache miss: full lookup required

/// A generic cursor cache that stores the result of a position lookup.
///
/// The cache stores:
/// - The visible position that was looked up
/// - Implementation-specific location data (e.g., span index, offset)
/// - Whether the cache is valid
#[derive(Clone, Debug)]
pub struct CursorCache<L: Clone> {
    /// The visible position that was looked up.
    pos: u64,
    /// Implementation-specific location data.
    location: L,
    /// Whether the cache is valid.
    valid: bool,
}

impl<L: Clone + Default> Default for CursorCache<L> {
    fn default() -> Self {
        return Self::new();
    }
}

impl<L: Clone + Default> CursorCache<L> {
    /// Create a new invalid cache.
    pub fn new() -> CursorCache<L> {
        return CursorCache {
            pos: 0,
            location: L::default(),
            valid: false,
        };
    }

    /// Check if the cache is valid.
    #[inline]
    pub fn is_valid(&self) -> bool {
        return self.valid;
    }

    /// Get the cached position (only valid if is_valid() returns true).
    #[inline]
    pub fn pos(&self) -> u64 {
        return self.pos;
    }

    /// Get the cached location (only valid if is_valid() returns true).
    #[inline]
    pub fn location(&self) -> &L {
        return &self.location;
    }

    /// Update the cache with a new lookup result.
    #[inline]
    pub fn update(&mut self, pos: u64, location: L) {
        self.pos = pos;
        self.location = location;
        self.valid = true;
    }

    /// Invalidate the cache.
    #[inline]
    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    /// Check if the cache can be used for a sequential forward lookup.
    ///
    /// Returns true if `target_pos == cached_pos + 1`.
    #[inline]
    pub fn is_sequential_forward(&self, target_pos: u64) -> bool {
        return self.valid && target_pos == self.pos + 1;
    }

    /// Check if the cache can be used for a sequential backward lookup.
    ///
    /// Returns true if `target_pos + 1 == cached_pos`.
    #[inline]
    pub fn is_sequential_backward(&self, target_pos: u64) -> bool {
        return self.valid && self.pos > 0 && target_pos == self.pos - 1;
    }

    /// Check if the cache is an exact hit.
    ///
    /// Returns true if `target_pos == cached_pos`.
    #[inline]
    pub fn is_exact_hit(&self, target_pos: u64) -> bool {
        return self.valid && target_pos == self.pos;
    }

    /// Adjust the cache after a delete operation.
    ///
    /// If the delete is entirely after the cached position, the cache
    /// remains valid. Otherwise, the cache is invalidated.
    #[inline]
    pub fn adjust_after_delete(&mut self, delete_start: u64, _delete_len: u64) {
        if !self.valid {
            return;
        }
        // If delete is after our cached position, cache remains valid
        if delete_start > self.pos {
            return;
        }
        // Delete touches or precedes cached position - must invalidate
        self.invalidate();
    }

    /// Adjust the cache after an insert operation.
    ///
    /// If the insert is after the cached position, the cache remains valid.
    /// If the insert is at or before the cached position, we adjust the
    /// cached position by the insert length.
    #[inline]
    pub fn adjust_after_insert(&mut self, insert_pos: u64, insert_len: u64) {
        if !self.valid {
            return;
        }
        if insert_pos <= self.pos {
            // Insert before or at cached position - adjust position
            self.pos += insert_len;
        }
        // Insert after cached position - no adjustment needed
    }
}

/// Location data for a simple span-based structure.
#[derive(Clone, Debug, Default)]
pub struct SpanLocation {
    /// Index of the span.
    pub span_idx: usize,
    /// Offset within the span.
    pub offset: u64,
}

/// Location data for a B-tree based structure with chunk caching.
#[derive(Clone, Debug, Default)]
pub struct BTreeLocation {
    /// Index of the span in the overall list.
    pub span_idx: usize,
    /// Offset within the span.
    pub offset: u64,
    /// Chunk/leaf index for avoiding tree traversal.
    pub chunk_idx: usize,
    /// Index within the chunk.
    pub idx_in_chunk: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_initially_invalid() {
        let cache: CursorCache<SpanLocation> = CursorCache::new();
        assert!(!cache.is_valid());
    }

    #[test]
    fn cache_update_and_access() {
        let mut cache: CursorCache<SpanLocation> = CursorCache::new();
        
        cache.update(10, SpanLocation { span_idx: 5, offset: 3 });
        
        assert!(cache.is_valid());
        assert_eq!(cache.pos(), 10);
        assert_eq!(cache.location().span_idx, 5);
        assert_eq!(cache.location().offset, 3);
    }

    #[test]
    fn cache_invalidate() {
        let mut cache: CursorCache<SpanLocation> = CursorCache::new();
        cache.update(10, SpanLocation { span_idx: 5, offset: 3 });
        
        cache.invalidate();
        
        assert!(!cache.is_valid());
    }

    #[test]
    fn sequential_forward() {
        let mut cache: CursorCache<SpanLocation> = CursorCache::new();
        cache.update(10, SpanLocation { span_idx: 5, offset: 3 });
        
        assert!(cache.is_sequential_forward(11));
        assert!(!cache.is_sequential_forward(10));
        assert!(!cache.is_sequential_forward(12));
        assert!(!cache.is_sequential_forward(9));
    }

    #[test]
    fn sequential_backward() {
        let mut cache: CursorCache<SpanLocation> = CursorCache::new();
        cache.update(10, SpanLocation { span_idx: 5, offset: 3 });
        
        assert!(cache.is_sequential_backward(9));
        assert!(!cache.is_sequential_backward(10));
        assert!(!cache.is_sequential_backward(8));
        assert!(!cache.is_sequential_backward(11));
    }

    #[test]
    fn exact_hit() {
        let mut cache: CursorCache<SpanLocation> = CursorCache::new();
        cache.update(10, SpanLocation { span_idx: 5, offset: 3 });
        
        assert!(cache.is_exact_hit(10));
        assert!(!cache.is_exact_hit(9));
        assert!(!cache.is_exact_hit(11));
    }

    #[test]
    fn adjust_after_delete_after() {
        let mut cache: CursorCache<SpanLocation> = CursorCache::new();
        cache.update(10, SpanLocation { span_idx: 5, offset: 3 });
        
        cache.adjust_after_delete(15, 5); // Delete after cache
        
        assert!(cache.is_valid());
        assert_eq!(cache.pos(), 10);
    }

    #[test]
    fn adjust_after_delete_before() {
        let mut cache: CursorCache<SpanLocation> = CursorCache::new();
        cache.update(10, SpanLocation { span_idx: 5, offset: 3 });
        
        cache.adjust_after_delete(5, 3); // Delete before cache
        
        assert!(!cache.is_valid());
    }

    #[test]
    fn adjust_after_insert_before() {
        let mut cache: CursorCache<SpanLocation> = CursorCache::new();
        cache.update(10, SpanLocation { span_idx: 5, offset: 3 });
        
        cache.adjust_after_insert(5, 3); // Insert before cache
        
        assert!(cache.is_valid());
        assert_eq!(cache.pos(), 13); // Position shifted by insert length
    }

    #[test]
    fn adjust_after_insert_after() {
        let mut cache: CursorCache<SpanLocation> = CursorCache::new();
        cache.update(10, SpanLocation { span_idx: 5, offset: 3 });
        
        cache.adjust_after_insert(15, 5); // Insert after cache
        
        assert!(cache.is_valid());
        assert_eq!(cache.pos(), 10); // Position unchanged
    }
}
