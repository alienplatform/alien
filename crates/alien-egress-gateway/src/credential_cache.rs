//! Small bounded LRU caches. Expired credentials are never served after a vault failure.
use std::{collections::VecDeque, sync::Mutex, time::Duration};
use tokio::time::Instant;

const CAPACITY: usize = 256;

pub(crate) struct Cache<K, V> {
    entries: Mutex<VecDeque<(K, Instant, V)>>,
    ttl: Duration,
}
impl<K: PartialEq, V: Clone> Cache<K, V> {
    pub fn new(ttl: Duration) -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            ttl,
        }
    }
    pub fn get(&self, key: &K) -> Option<V> {
        let mut entries = self.entries.lock().ok()?;
        let index = entries.iter().position(|(stored, _, _)| stored == key)?;
        let entry = entries.remove(index)?;
        if entry.1.elapsed() >= self.ttl {
            return None;
        }
        let value = entry.2.clone();
        entries.push_back(entry);
        Some(value)
    }
    pub fn insert(&self, key: K, value: V) {
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        entries.retain(|(stored, loaded, _)| *stored != key && loaded.elapsed() < self.ttl);
        if entries.len() >= CAPACITY {
            entries.pop_front();
        }
        entries.push_back((key, Instant::now(), value));
    }
}
