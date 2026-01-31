---
model = "claude-opus-4-5"
created = "2026-01-31"
modified = "2026-01-31"
driver = "Isaac Clayton"
---

# Next Optimization Opportunities

## Current State

- 2.3x slower than diamond-types on sveltecomponent (2.64ms vs 1.15ms)
- 4.1x slower on seph-blog1 (27.2ms vs 6.58ms) due to 3x more chunks

## Analysis

### 1. seph-blog1 Performance Gap

seph-blog1 has 10.7k spans vs 3.7k for sveltecomponent, resulting in:
- 168 chunks vs 59 chunks
- Each lookup scans ~84 chunks vs ~29 chunks on average
- 2.9x more work per lookup explains most of the 4.1x/2.3x ratio

**Solution**: O(log n) chunk lookup via Fenwick tree or skip list

### 2. Span Struct Size

Current Span is 112 bytes - doesn't fit in a cache line (64 bytes).

| Layout | Size | Items/cache line |
|--------|------|------------------|
| Current | 112 bytes | 0 |
| Compact1 (origin as index) | 64 bytes | 1 |
| Compact2 (user as index) | 24 bytes | 2 |
| Minimal (origin separate) | 12 bytes | 5 |

Main bloat sources:
- `user: KeyPub` is 32 bytes (could be u16 index into user table)
- `origin: Option<ItemId>` is 48 bytes (could be u32 span index + u32 offset)

### 3. Skip List Adaptation

The existing skip list tracks item counts in `widths[]`. For weighted RGA:
- Change `widths[]` to track cumulative weights instead of item counts
- Add `find_by_weight(weight) -> (index, offset_in_item)`
- Add `update_weight(index, new_weight)`

The skip list already handles:
- O(log n) position lookup via width sums
- Chunked nodes (64 items/node) for cache locality
- Proper width updates on insert/remove

### 4. Fenwick Tree for Chunk Weights

Alternative: Keep chunked list but add Fenwick tree over chunk weights.

```rust
struct WeightedList<T> {
    chunks: Vec<Chunk<T>>,
    chunk_weights: FenwickTree,  // prefix sums of chunk weights
    total_weight: u64,
    len: usize,
}
```

Operations:
- `find_chunk_by_weight`: Binary search using Fenwick prefix queries - O(log chunks)
- On chunk weight change: Fenwick point update - O(log chunks)
- On chunk split: Rebuild Fenwick tree - O(chunks) but rare

Fenwick tree implementation (from unnamed.website research):
```rust
struct FenwickTree {
    tree: Vec<u64>,
}

impl FenwickTree {
    fn new(n: usize) -> Self {
        FenwickTree { tree: vec![0; n + 1] }
    }
    
    // Sum of elements 0..=i
    fn prefix_sum(&self, mut i: usize) -> u64 {
        i += 1;  // 1-indexed
        let mut sum = 0;
        while i > 0 {
            sum += self.tree[i];
            i -= i & i.wrapping_neg();  // i -= lsb(i)
        }
        sum
    }
    
    // Add delta to element i
    fn update(&mut self, mut i: usize, delta: i64) {
        i += 1;  // 1-indexed
        while i < self.tree.len() {
            self.tree[i] = (self.tree[i] as i64 + delta) as u64;
            i += i & i.wrapping_neg();  // i += lsb(i)
        }
    }
    
    // Find first index where prefix_sum > target
    fn find_first_exceeding(&self, target: u64) -> usize {
        // Binary search using prefix_sum
        let mut lo = 0;
        let mut hi = self.tree.len() - 1;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.prefix_sum(mid) <= target {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }
}
```

## Recommended Approach

1. **Compact Span first** (low risk, moderate gain)
   - Change `user: KeyPub` to `user_idx: u16` with lookup table
   - Change `origin: Option<ItemId>` to `origin_idx: u32, origin_offset: u32`
   - Gets Span from 112 bytes to ~24 bytes
   - Better cache utilization

2. **Fenwick tree for chunk weights** (medium risk, high gain on large traces)
   - O(log n) chunk lookup instead of O(n)
   - Especially helps seph-blog1 (168 chunks)
   - Simpler than adapting skip list

3. **Adapt skip list for weights** (higher risk, potentially best performance)
   - Uses existing well-tested skip list code
   - Change widths from item counts to weights
   - Enables O(log n) weighted lookup

## Complexity vs Gain Tradeoff

| Optimization | Complexity | Expected Gain |
|-------------|------------|---------------|
| Compact Span | Low | 1.2-1.5x |
| Fenwick tree | Medium | 1.5-2x on large traces |
| Weighted skip list | High | 2x+ |
