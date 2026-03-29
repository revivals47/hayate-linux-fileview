//! Simple LRU cache backed by a HashMap + VecDeque.
//!
//! Evicts the least-recently-used entry one at a time when capacity is
//! exceeded, instead of clearing the entire cache.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

pub(crate) struct LruCache<K, V> {
    map: HashMap<K, V>,
    order: VecDeque<K>,
    capacity: usize,
}

impl<K: Eq + Hash + Clone, V> LruCache<K, V> {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Get a reference to the value, promoting the key to most-recently-used.
    #[cfg(test)]
    pub(crate) fn get(&mut self, key: &K) -> Option<&V> {
        if self.map.contains_key(key) {
            self.touch(key);
            self.map.get(key)
        } else {
            None
        }
    }

    /// Insert a key-value pair. If the key exists, it is promoted and the
    /// value is updated. If at capacity, the LRU entry is evicted first.
    pub(crate) fn insert(&mut self, key: K, value: V) {
        if self.map.contains_key(&key) {
            self.touch(&key);
            self.map.insert(key, value);
            return;
        }
        // Evict LRU entries until we're under capacity
        while self.map.len() >= self.capacity {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            } else {
                break;
            }
        }
        self.order.push_back(key.clone());
        self.map.insert(key, value);
    }

    /// Get existing value or insert a new one computed by `f`.
    /// Returns a mutable reference to the value.
    pub(crate) fn get_or_insert_with(&mut self, key: K, f: impl FnOnce() -> V) -> &mut V {
        if self.map.contains_key(&key) {
            self.touch(&key);
        } else {
            let value = f();
            self.insert(key.clone(), value);
        }
        self.map.get_mut(&key).unwrap()
    }

    /// Remove all entries.
    pub(crate) fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }

    /// Number of entries currently cached.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.map.len()
    }

    /// Move `key` to the back (most recently used) of the order queue.
    fn touch(&mut self, key: &K) {
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            self.order.remove(pos);
        }
        self.order.push_back(key.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_insert_and_get() {
        let mut c = LruCache::new(3);
        c.insert("a", 1);
        c.insert("b", 2);
        c.insert("c", 3);
        assert_eq!(c.get(&"a"), Some(&1));
        assert_eq!(c.get(&"b"), Some(&2));
        assert_eq!(c.get(&"c"), Some(&3));
        assert_eq!(c.len(), 3);
    }

    #[test]
    fn evicts_lru_on_overflow() {
        let mut c = LruCache::new(2);
        c.insert("a", 1);
        c.insert("b", 2);
        // "a" is LRU
        c.insert("c", 3);
        assert_eq!(c.get(&"a"), None); // evicted
        assert_eq!(c.get(&"b"), Some(&2));
        assert_eq!(c.get(&"c"), Some(&3));
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn access_promotes_entry() {
        let mut c = LruCache::new(2);
        c.insert("a", 1);
        c.insert("b", 2);
        // Access "a" → now "b" is LRU
        c.get(&"a");
        c.insert("c", 3);
        assert_eq!(c.get(&"b"), None); // "b" was LRU, evicted
        assert_eq!(c.get(&"a"), Some(&1)); // promoted, survives
        assert_eq!(c.get(&"c"), Some(&3));
    }

    #[test]
    fn get_or_insert_with_creates() {
        let mut c: LruCache<&str, i32> = LruCache::new(3);
        let v = c.get_or_insert_with("x", || 42);
        assert_eq!(*v, 42);
        // Second call returns cached value
        let v2 = c.get_or_insert_with("x", || 99);
        assert_eq!(*v2, 42);
    }

    #[test]
    fn clear_empties_cache() {
        let mut c = LruCache::new(3);
        c.insert("a", 1);
        c.insert("b", 2);
        c.clear();
        assert_eq!(c.len(), 0);
        assert_eq!(c.get(&"a"), None);
    }

    #[test]
    fn update_existing_key() {
        let mut c = LruCache::new(3);
        c.insert("a", 1);
        c.insert("a", 10);
        assert_eq!(c.get(&"a"), Some(&10));
        assert_eq!(c.len(), 1);
    }
}
