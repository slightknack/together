// model = "claude-opus-4-5"
// created = 2026-02-01
// modified = 2026-02-01
// driver = "Isaac Clayton"

//! User table for mapping user IDs to compact indices.
//!
//! In CRDT implementations, each user/replica is identified by a unique ID
//! (typically a public key). Storing full IDs in every span would be expensive
//! (32 bytes per span), so we use a user table to map IDs to 16-bit indices.
//!
//! The table supports:
//! - Get or insert: O(1) average case (hash map)
//! - Index to ID: O(1) (array lookup)
//! - Maximum 65,534 users (u16::MAX - 1 reserved for sentinel)

use std::hash::Hash;

use rustc_hash::FxHashMap;

use super::id::UserIdx;

/// A table mapping user IDs to compact indices.
///
/// Generic over the user ID type to support different key types
/// (e.g., KeyPub, String, u64).
#[derive(Clone, Debug)]
pub struct UserTable<U: Clone + Eq + Hash> {
    /// Map from user ID to index.
    id_to_idx: FxHashMap<U, UserIdx>,
    /// Map from index to user ID.
    idx_to_id: Vec<U>,
}

impl<U: Clone + Eq + Hash> Default for UserTable<U> {
    fn default() -> Self {
        return Self::new();
    }
}

impl<U: Clone + Eq + Hash> UserTable<U> {
    /// Create a new empty user table.
    pub fn new() -> UserTable<U> {
        return UserTable {
            id_to_idx: FxHashMap::default(),
            idx_to_id: Vec::new(),
        };
    }

    /// Get or insert a user, returning their index.
    ///
    /// If the user already exists, returns their existing index.
    /// Otherwise, assigns a new index and returns it.
    ///
    /// Panics if trying to add more than 65,534 users.
    pub fn get_or_insert(&mut self, user: &U) -> UserIdx {
        if let Some(&idx) = self.id_to_idx.get(user) {
            return idx;
        }
        
        let idx = self.idx_to_id.len();
        assert!(idx < (u16::MAX - 1) as usize, "too many users (max 65534)");
        
        let user_idx = UserIdx::new(idx as u16);
        self.idx_to_id.push(user.clone());
        self.id_to_idx.insert(user.clone(), user_idx);
        
        return user_idx;
    }

    /// Get the index for a user, if they exist.
    #[inline]
    pub fn get(&self, user: &U) -> Option<UserIdx> {
        return self.id_to_idx.get(user).copied();
    }

    /// Get the user ID for an index, if it exists.
    #[inline]
    pub fn get_id(&self, idx: UserIdx) -> Option<&U> {
        if idx.is_none() {
            return None;
        }
        return self.idx_to_id.get(idx.0 as usize);
    }

    /// Get the number of users in the table.
    #[inline]
    pub fn len(&self) -> usize {
        return self.idx_to_id.len();
    }

    /// Check if the table is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        return self.idx_to_id.is_empty();
    }

    /// Iterate over all (index, user) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (UserIdx, &U)> {
        return self.idx_to_id
            .iter()
            .enumerate()
            .map(|(i, u)| (UserIdx::new(i as u16), u));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_table() {
        let table: UserTable<String> = UserTable::new();
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn insert_and_get() {
        let mut table: UserTable<String> = UserTable::new();
        
        let idx = table.get_or_insert(&"alice".to_string());
        assert_eq!(idx, UserIdx::new(0));
        assert_eq!(table.len(), 1);
        
        // Get same user again - should return same index
        let idx2 = table.get_or_insert(&"alice".to_string());
        assert_eq!(idx2, idx);
        assert_eq!(table.len(), 1); // Still just 1 user
    }

    #[test]
    fn multiple_users() {
        let mut table: UserTable<String> = UserTable::new();
        
        let alice = table.get_or_insert(&"alice".to_string());
        let bob = table.get_or_insert(&"bob".to_string());
        let charlie = table.get_or_insert(&"charlie".to_string());
        
        assert_eq!(alice, UserIdx::new(0));
        assert_eq!(bob, UserIdx::new(1));
        assert_eq!(charlie, UserIdx::new(2));
        assert_eq!(table.len(), 3);
    }

    #[test]
    fn get_existing() {
        let mut table: UserTable<String> = UserTable::new();
        table.get_or_insert(&"alice".to_string());
        
        assert_eq!(table.get(&"alice".to_string()), Some(UserIdx::new(0)));
        assert_eq!(table.get(&"bob".to_string()), None);
    }

    #[test]
    fn get_id() {
        let mut table: UserTable<String> = UserTable::new();
        table.get_or_insert(&"alice".to_string());
        table.get_or_insert(&"bob".to_string());
        
        assert_eq!(table.get_id(UserIdx::new(0)), Some(&"alice".to_string()));
        assert_eq!(table.get_id(UserIdx::new(1)), Some(&"bob".to_string()));
        assert_eq!(table.get_id(UserIdx::new(2)), None);
        assert_eq!(table.get_id(UserIdx::NONE), None);
    }

    #[test]
    fn iterate() {
        let mut table: UserTable<String> = UserTable::new();
        table.get_or_insert(&"alice".to_string());
        table.get_or_insert(&"bob".to_string());
        
        let pairs: Vec<_> = table.iter().collect();
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0], (UserIdx::new(0), &"alice".to_string()));
        assert_eq!(pairs[1], (UserIdx::new(1), &"bob".to_string()));
    }

    #[test]
    fn works_with_integers() {
        let mut table: UserTable<u64> = UserTable::new();
        
        let idx1 = table.get_or_insert(&12345);
        let idx2 = table.get_or_insert(&67890);
        
        assert_eq!(idx1, UserIdx::new(0));
        assert_eq!(idx2, UserIdx::new(1));
        assert_eq!(table.get_id(idx1), Some(&12345));
    }
}
