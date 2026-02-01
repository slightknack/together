// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

//! Clock primitives for tracking causality and ordering.
//!
//! # Lamport Clock
//!
//! A simple monotonic counter that provides a partial ordering of events.
//! When two events have different Lamport times, the one with lower time
//! happened before. When times are equal, events are concurrent.
//!
//! Complexity:
//! - tick: O(1)
//! - update: O(1)
//! - compare: O(1)
//!
//! # Vector Clock
//!
//! Tracks causality across multiple replicas. Each replica maintains
//! a counter, and the vector represents knowledge of all replicas.
//!
//! Complexity:
//! - tick: O(1)
//! - merge: O(n) where n is number of replicas
//! - compare: O(n)

use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::Hash;

/// A Lamport clock for partial ordering of events.
///
/// The clock is a simple counter that:
/// - Increments on local events (tick)
/// - Updates to max(local, remote) + 1 on receiving messages
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LamportClock {
    time: u64,
}

impl LamportClock {
    /// Create a new clock starting at 0.
    pub fn new() -> LamportClock {
        return LamportClock { time: 0 };
    }

    /// Create a clock with a specific starting time.
    pub fn with_time(time: u64) -> LamportClock {
        return LamportClock { time };
    }

    /// Get the current time.
    #[inline]
    pub fn time(&self) -> u64 {
        return self.time;
    }

    /// Increment the clock for a local event.
    /// Returns the new time.
    #[inline]
    pub fn tick(&mut self) -> u64 {
        self.time += 1;
        return self.time;
    }

    /// Update the clock upon receiving a message with the given timestamp.
    /// Sets local time to max(local, remote) + 1.
    /// Returns the new time.
    #[inline]
    pub fn update(&mut self, remote_time: u64) -> u64 {
        self.time = self.time.max(remote_time) + 1;
        return self.time;
    }

    /// Merge with another clock (for sync).
    /// Sets local time to max(local, other).
    #[inline]
    pub fn merge(&mut self, other: &LamportClock) {
        self.time = self.time.max(other.time);
    }
}

impl PartialOrd for LamportClock {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        return Some(self.cmp(other));
    }
}

impl Ord for LamportClock {
    fn cmp(&self, other: &Self) -> Ordering {
        return self.time.cmp(&other.time);
    }
}

/// A vector clock for tracking causality across replicas.
///
/// Each replica has an entry in the vector. The clock captures
/// the "happens-before" relationship between events.
#[derive(Clone, Debug, Default)]
pub struct VectorClock<K: Clone + Eq + Hash> {
    /// Map from replica ID to that replica's logical time.
    entries: HashMap<K, u64>,
}

impl<K: Clone + Eq + Hash> VectorClock<K> {
    /// Create an empty vector clock.
    pub fn new() -> VectorClock<K> {
        return VectorClock {
            entries: HashMap::new(),
        };
    }

    /// Get the time for a specific replica.
    pub fn get(&self, replica: &K) -> u64 {
        return *self.entries.get(replica).unwrap_or(&0);
    }

    /// Increment the clock for the given replica.
    /// Returns the new time for that replica.
    pub fn tick(&mut self, replica: K) -> u64 {
        let entry = self.entries.entry(replica).or_insert(0);
        *entry += 1;
        return *entry;
    }

    /// Update a specific entry (for receiving an operation).
    pub fn update(&mut self, replica: K, time: u64) {
        let entry = self.entries.entry(replica).or_insert(0);
        *entry = (*entry).max(time);
    }

    /// Merge with another vector clock.
    /// Takes the pointwise maximum of all entries.
    pub fn merge(&mut self, other: &VectorClock<K>) {
        for (k, v) in &other.entries {
            let entry = self.entries.entry(k.clone()).or_insert(0);
            *entry = (*entry).max(*v);
        }
    }

    /// Check if this clock causally precedes another.
    /// Returns true if all entries in self are <= corresponding entries in other,
    /// and at least one is strictly less.
    pub fn happens_before(&self, other: &VectorClock<K>) -> bool {
        let mut dominated = true;
        let mut strictly_less = false;

        for (k, v) in &self.entries {
            let other_v = other.get(k);
            if *v > other_v {
                dominated = false;
                break;
            }
            if *v < other_v {
                strictly_less = true;
            }
        }

        if !dominated {
            return false;
        }

        // Check for entries in other not in self
        for (k, v) in &other.entries {
            if !self.entries.contains_key(k) && *v > 0 {
                strictly_less = true;
            }
        }

        return strictly_less;
    }

    /// Check if two clocks are concurrent (neither happens-before the other).
    pub fn concurrent_with(&self, other: &VectorClock<K>) -> bool {
        return !self.happens_before(other) && !other.happens_before(self) && self != other;
    }
}

impl<K: Clone + Eq + Hash> PartialEq for VectorClock<K> {
    fn eq(&self, other: &Self) -> bool {
        // Two clocks are equal if they have the same entries
        if self.entries.len() != other.entries.len() {
            return false;
        }
        for (k, v) in &self.entries {
            if other.get(k) != *v {
                return false;
            }
        }
        return true;
    }
}

impl<K: Clone + Eq + Hash> Eq for VectorClock<K> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lamport_tick() {
        let mut clock = LamportClock::new();
        assert_eq!(clock.time(), 0);
        
        assert_eq!(clock.tick(), 1);
        assert_eq!(clock.tick(), 2);
        assert_eq!(clock.time(), 2);
    }

    #[test]
    fn lamport_update() {
        let mut clock = LamportClock::new();
        clock.tick(); // time = 1
        
        // Receive message with time 5
        assert_eq!(clock.update(5), 6);
        assert_eq!(clock.time(), 6);
        
        // Receive message with time 3 (less than current)
        assert_eq!(clock.update(3), 7);
        assert_eq!(clock.time(), 7);
    }

    #[test]
    fn lamport_merge() {
        let mut a = LamportClock::with_time(5);
        let b = LamportClock::with_time(10);
        
        a.merge(&b);
        assert_eq!(a.time(), 10);
    }

    #[test]
    fn lamport_ordering() {
        let a = LamportClock::with_time(5);
        let b = LamportClock::with_time(10);
        
        assert!(a < b);
        assert!(b > a);
        assert_eq!(a, LamportClock::with_time(5));
    }

    #[test]
    fn vector_clock_basic() {
        let mut clock: VectorClock<&str> = VectorClock::new();
        
        assert_eq!(clock.get(&"alice"), 0);
        
        clock.tick("alice");
        assert_eq!(clock.get(&"alice"), 1);
        
        clock.tick("bob");
        assert_eq!(clock.get(&"bob"), 1);
    }

    #[test]
    fn vector_clock_merge() {
        let mut a: VectorClock<&str> = VectorClock::new();
        let mut b: VectorClock<&str> = VectorClock::new();
        
        a.tick("alice");
        a.tick("alice");
        b.tick("bob");
        b.tick("bob");
        b.tick("bob");
        
        a.merge(&b);
        
        assert_eq!(a.get(&"alice"), 2);
        assert_eq!(a.get(&"bob"), 3);
    }

    #[test]
    fn vector_clock_happens_before() {
        let mut a: VectorClock<&str> = VectorClock::new();
        let mut b: VectorClock<&str> = VectorClock::new();
        
        a.tick("alice");
        
        b.tick("alice");
        b.tick("bob");
        
        assert!(a.happens_before(&b));
        assert!(!b.happens_before(&a));
    }

    #[test]
    fn vector_clock_concurrent() {
        let mut a: VectorClock<&str> = VectorClock::new();
        let mut b: VectorClock<&str> = VectorClock::new();
        
        a.tick("alice");
        b.tick("bob");
        
        assert!(a.concurrent_with(&b));
        assert!(b.concurrent_with(&a));
        assert!(!a.happens_before(&b));
        assert!(!b.happens_before(&a));
    }

    #[test]
    fn vector_clock_equality() {
        let mut a: VectorClock<&str> = VectorClock::new();
        let mut b: VectorClock<&str> = VectorClock::new();
        
        a.tick("alice");
        b.tick("alice");
        
        assert_eq!(a, b);
        assert!(!a.concurrent_with(&b));
    }
}
