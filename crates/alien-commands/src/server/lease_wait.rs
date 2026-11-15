//! Wake-ups for lease requests that wait for a command to be queued.
//!
//! A lease request with `waitSeconds > 0` parks in the manager until a command
//! for its (deployment, target resource) is queued. The command server signals
//! the key after the pending index write succeeds. The signal is only a hint:
//! a woken waiter repeats the normal scan and the normal atomic lease write, so
//! a lost wake-up (another manager replica queued the command, or the signal
//! raced the scan) costs at most the re-scan interval.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::sync::Notify;

/// Open waiting lease requests one manager process holds by default. Over the
/// cap a request is answered at once, as if it had not asked to wait.
pub const DEFAULT_MAX_OPEN_LEASE_WAITS: usize = 5_000;

/// How often a waiting request scans again without a wake-up. Covers commands
/// queued through another manager replica and lost wake-ups.
pub const DEFAULT_LEASE_WAIT_RESCAN_INTERVAL: Duration = Duration::from_secs(3);

type Key = (String, String);

struct Slot {
    notify: Arc<Notify>,
    waiters: usize,
}

#[derive(Default)]
struct State {
    slots: HashMap<Key, Slot>,
    open: usize,
}

/// Registry of open waits, keyed by (deployment id, target resource id): the
/// same key the pending index is scanned by.
#[derive(Default)]
pub(crate) struct LeaseWaiters {
    state: Mutex<State>,
}

impl LeaseWaiters {
    fn lock(&self) -> MutexGuard<'_, State> {
        // The state holds only counters and notifiers; a panic elsewhere
        // cannot leave it half-updated.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Register a waiter, or `None` when `max_open` waits are already open.
    pub(crate) fn register(
        self: &Arc<Self>,
        deployment_id: &str,
        target_resource_id: &str,
        max_open: usize,
    ) -> Option<LeaseWait> {
        let mut state = self.lock();
        if state.open >= max_open {
            return None;
        }
        state.open += 1;
        let key = (deployment_id.to_string(), target_resource_id.to_string());
        let slot = state.slots.entry(key.clone()).or_insert_with(|| Slot {
            notify: Arc::new(Notify::new()),
            waiters: 0,
        });
        slot.waiters += 1;
        Some(LeaseWait {
            registry: self.clone(),
            key,
            notify: slot.notify.clone(),
        })
    }

    /// Wake every waiter on this key. No-op when nobody waits.
    pub(crate) fn notify(&self, deployment_id: &str, target_resource_id: &str) {
        let key = (deployment_id.to_string(), target_resource_id.to_string());
        if let Some(slot) = self.lock().slots.get(&key) {
            slot.notify.notify_waiters();
        }
    }

    pub(crate) fn open_waits(&self) -> usize {
        self.lock().open
    }

    #[cfg(test)]
    pub(crate) fn open_keys(&self) -> usize {
        self.lock().slots.len()
    }
}

/// One open wait. Dropping it removes the key's entry when it was the last.
pub(crate) struct LeaseWait {
    registry: Arc<LeaseWaiters>,
    key: Key,
    notify: Arc<Notify>,
}

impl LeaseWait {
    pub(crate) fn notified(&self) -> tokio::sync::futures::Notified<'_> {
        self.notify.notified()
    }
}

impl Drop for LeaseWait {
    fn drop(&mut self) {
        let mut state = self.registry.lock();
        state.open -= 1;
        if let Some(slot) = state.slots.get_mut(&self.key) {
            slot.waiters -= 1;
            if slot.waiters == 0 {
                state.slots.remove(&self.key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_is_removed_when_the_last_waiter_leaves() {
        let waiters = Arc::new(LeaseWaiters::default());
        let a = waiters.register("dep", "queue", 10).unwrap();
        let b = waiters.register("dep", "queue", 10).unwrap();
        let other = waiters.register("dep", "other", 10).unwrap();
        assert_eq!(waiters.open_keys(), 2);
        drop(a);
        assert_eq!(waiters.open_keys(), 2);
        drop(b);
        assert_eq!(waiters.open_keys(), 1);
        drop(other);
        assert_eq!((waiters.open_keys(), waiters.open_waits()), (0, 0));
    }

    #[test]
    fn registration_is_refused_over_the_cap() {
        let waiters = Arc::new(LeaseWaiters::default());
        let first = waiters.register("dep", "queue", 1).unwrap();
        assert!(waiters.register("dep", "other", 1).is_none());
        drop(first);
        assert!(waiters.register("dep", "other", 1).is_some());
    }
}
