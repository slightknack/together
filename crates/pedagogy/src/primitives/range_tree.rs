// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

//! Range tree for efficient range queries and updates.
//!
//! A range tree supports:
//! - Insert/delete at position
//! - Range queries (sum, count)
//! - Position-to-index and index-to-position conversion
//!
//! This is a building block for CRDT implementations that need
//! both positional and logical indexing.

use std::fmt::Debug;

/// A trait for values that can be aggregated in a range tree.
///
/// The aggregate is used for efficient range queries.
pub trait Aggregate: Clone + Default {
    /// Combine two aggregates.
    fn combine(&self, other: &Self) -> Self;
}

/// Simple count aggregate - just counts items.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Count(pub usize);

impl Aggregate for Count {
    fn combine(&self, other: &Self) -> Self {
        return Count(self.0 + other.0);
    }
}

/// Weight aggregate - sums u64 weights.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Weight(pub u64);

impl Aggregate for Weight {
    fn combine(&self, other: &Self) -> Self {
        return Weight(self.0 + other.0);
    }
}

/// Combined count and weight aggregate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CountWeight {
    pub count: usize,
    pub weight: u64,
}

impl Aggregate for CountWeight {
    fn combine(&self, other: &Self) -> Self {
        return CountWeight {
            count: self.count + other.count,
            weight: self.weight + other.weight,
        };
    }
}

/// A trait for items stored in the range tree.
pub trait RangeItem: Clone {
    /// The aggregate type for this item.
    type Agg: Aggregate;

    /// Compute the aggregate for this item.
    fn aggregate(&self) -> Self::Agg;
}

/// A simple weighted item.
#[derive(Clone, Debug)]
pub struct WeightedItem<T> {
    pub value: T,
    pub weight: u64,
}

impl<T: Clone> RangeItem for WeightedItem<T> {
    type Agg = CountWeight;

    fn aggregate(&self) -> Self::Agg {
        return CountWeight {
            count: 1,
            weight: self.weight,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_aggregate() {
        let a = Count(5);
        let b = Count(3);
        assert_eq!(a.combine(&b), Count(8));
    }

    #[test]
    fn weight_aggregate() {
        let a = Weight(100);
        let b = Weight(50);
        assert_eq!(a.combine(&b), Weight(150));
    }

    #[test]
    fn count_weight_aggregate() {
        let a = CountWeight { count: 2, weight: 100 };
        let b = CountWeight { count: 3, weight: 150 };
        let combined = a.combine(&b);
        assert_eq!(combined.count, 5);
        assert_eq!(combined.weight, 250);
    }
}
