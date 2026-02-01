//! Simple profiling counters for understanding hot paths.

use std::sync::atomic::{AtomicU64, Ordering};

pub static CURSOR_HITS: AtomicU64 = AtomicU64::new(0);
pub static CURSOR_MISSES: AtomicU64 = AtomicU64::new(0);
pub static COALESCE_COUNT: AtomicU64 = AtomicU64::new(0);
pub static YATA_SCAN_COUNT: AtomicU64 = AtomicU64::new(0);
pub static YATA_FAST_EXIT: AtomicU64 = AtomicU64::new(0);

#[inline]
pub fn cursor_hit() {
    CURSOR_HITS.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn cursor_miss() {
    CURSOR_MISSES.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn coalesce() {
    COALESCE_COUNT.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn yata_scan() {
    YATA_SCAN_COUNT.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn yata_fast_exit() {
    YATA_FAST_EXIT.fetch_add(1, Ordering::Relaxed);
}

pub fn reset() {
    CURSOR_HITS.store(0, Ordering::Relaxed);
    CURSOR_MISSES.store(0, Ordering::Relaxed);
    COALESCE_COUNT.store(0, Ordering::Relaxed);
    YATA_SCAN_COUNT.store(0, Ordering::Relaxed);
    YATA_FAST_EXIT.store(0, Ordering::Relaxed);
}

pub fn report() -> String {
    let hits = CURSOR_HITS.load(Ordering::Relaxed);
    let misses = CURSOR_MISSES.load(Ordering::Relaxed);
    let total = hits + misses;
    let hit_rate = if total > 0 { hits as f64 / total as f64 * 100.0 } else { 0.0 };
    
    let coalesce = COALESCE_COUNT.load(Ordering::Relaxed);
    let yata = YATA_SCAN_COUNT.load(Ordering::Relaxed);
    let fast = YATA_FAST_EXIT.load(Ordering::Relaxed);
    
    format!(
        "Cursor: {}/{} ({:.1}% hit), Coalesce: {}, YATA: {} (fast exit: {})",
        hits, total, hit_rate, coalesce, yata, fast
    )
}
