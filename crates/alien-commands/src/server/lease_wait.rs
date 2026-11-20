//! Wake-ups for lease requests that wait for a command to be queued.
//!
//! A lease request with `waitSeconds > 0` parks in the manager until a command
//! for its (deployment, target resource) is queued. The command server signals
//! the key after the pending index write succeeds. The signal reaches waiters
//! in the same process only; a command queued through another manager replica
//! is seen by the waiter's probe of the target's wake key in the command KV.
//! Either way the signal is only a hint: a woken waiter repeats the scan and
//! the atomic lease write that every lease request does.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::sync::Notify;
use tokio::time::Instant;

/// Open waiting lease requests one manager process holds by default. Over the
/// cap a request is answered at once, as if it had not asked to wait.
pub const DEFAULT_MAX_OPEN_LEASE_WAITS: usize = 5_000;

/// Open waiting lease requests one manager process holds for a single
/// (deployment, target) by default. A receiver needs one; the rest leave room
/// for a restart that overlaps its old connection. Over the cap a request is
/// answered at once. The target id is whatever the caller sends, so this cap
/// alone does not bound a deployment: see the per-deployment cap.
pub const DEFAULT_MAX_OPEN_LEASE_WAITS_PER_TARGET: usize = 8;

/// Open waiting lease requests one manager process holds for a single
/// deployment, over all its targets, by default. A deployment has a few
/// receivers, each needing one wait (a few more while a restart overlaps its
/// old connection), so this is well under the process cap and bounds what one
/// deployment's credentials can take, whatever target ids they send. Over the
/// cap a request is answered at once.
pub const DEFAULT_MAX_OPEN_LEASE_WAITS_PER_DEPLOYMENT: usize = 64;

/// How often the waiters of one target read its wake key, all together. Covers
/// commands queued through another manager replica, whose in-process signal
/// never reaches this process.
pub const DEFAULT_LEASE_WAIT_PROBE_INTERVAL: Duration = Duration::from_secs(1);

type Key = (String, String);

/// The caps a registration is checked against.
#[derive(Clone, Copy)]
pub(crate) struct WaitLimits {
    pub process: usize,
    pub deployment: usize,
    pub target: usize,
}

/// What the waiters of one target share.
pub(crate) struct Shared {
    notify: Notify,
    /// Held while a waiter scans after a wake-up, so waiters woken by one
    /// command scan one after the other, not all at once.
    scan: tokio::sync::Mutex<ScanMemo>,
    /// The last wake key read, so waiters on one target read it once per
    /// interval together.
    wake_read: tokio::sync::Mutex<Option<WakeRead>>,
    /// When the last in-process signal was sent.
    last_signal: Mutex<Option<Instant>>,
}

#[derive(Default)]
pub(crate) struct ScanMemo {
    /// Start of the last scan that found nothing queued.
    empty_since: Option<Instant>,
}

struct WakeRead {
    started: Instant,
    value: Option<Vec<u8>>,
}

struct Slot {
    shared: Arc<Shared>,
    waiters: usize,
}

#[derive(Default)]
struct State {
    slots: HashMap<Key, Slot>,
    /// Open waits per deployment.
    by_deployment: HashMap<String, usize>,
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

    /// Register a waiter, or `None` when `limits.process` waits are already
    /// open in this process, `limits.deployment` for this deployment or
    /// `limits.target` on this key. Nothing but counters is touched.
    pub(crate) fn register(
        self: &Arc<Self>,
        deployment_id: &str,
        target_resource_id: &str,
        limits: WaitLimits,
    ) -> Option<LeaseWait> {
        let WaitLimits {
            process: max_open,
            deployment: max_per_deployment,
            target: max_per_target,
        } = limits;
        let mut state = self.lock();
        if state.open >= max_open {
            return None;
        }
        if state
            .by_deployment
            .get(deployment_id)
            .is_some_and(|open| *open >= max_per_deployment)
            || max_per_deployment == 0
        {
            return None;
        }
        let key = (deployment_id.to_string(), target_resource_id.to_string());
        if state
            .slots
            .get(&key)
            .is_some_and(|slot| slot.waiters >= max_per_target)
        {
            return None;
        }
        if max_per_target == 0 {
            return None;
        }
        state.open += 1;
        *state
            .by_deployment
            .entry(deployment_id.to_string())
            .or_insert(0) += 1;
        let slot = state.slots.entry(key.clone()).or_insert_with(|| Slot {
            shared: Arc::new(Shared {
                notify: Notify::new(),
                scan: tokio::sync::Mutex::new(ScanMemo::default()),
                wake_read: tokio::sync::Mutex::new(None),
                last_signal: Mutex::new(None),
            }),
            waiters: 0,
        });
        slot.waiters += 1;
        Some(LeaseWait {
            registry: self.clone(),
            key,
            shared: slot.shared.clone(),
        })
    }

    /// Wake every waiter on this key. No-op when nobody waits.
    pub(crate) fn notify(&self, deployment_id: &str, target_resource_id: &str) {
        let key = (deployment_id.to_string(), target_resource_id.to_string());
        if let Some(slot) = self.lock().slots.get(&key) {
            *slot
                .shared
                .last_signal
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
            slot.shared.notify.notify_waiters();
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
    shared: Arc<Shared>,
}

impl LeaseWait {
    pub(crate) fn notified(&self) -> tokio::sync::futures::Notified<'_> {
        self.shared.notify.notified()
    }

    /// When the last in-process signal for this target was sent: what a waiter
    /// woken by a signal is woken "at". Later than the queued command's index
    /// write, so a scan that starts after it sees the command.
    pub(crate) fn last_signal(&self) -> Instant {
        self.shared
            .last_signal
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .unwrap_or_else(Instant::now)
    }

    /// The wake key's value as read by a read that started at or after
    /// `not_before`, and when that read started. Waiters on one target share the
    /// read: one that finds a recent enough value uses it, so N waiters cost
    /// about one read per probe interval, not N.
    pub(crate) async fn wake_value<F, Fut>(
        &self,
        not_before: Instant,
        read: F,
    ) -> (Option<Vec<u8>>, Instant)
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Option<Vec<u8>>>,
    {
        let mut cached = self.shared.wake_read.lock().await;
        if let Some(entry) = cached.as_ref() {
            if entry.started >= not_before {
                return (entry.value.clone(), entry.started);
            }
        }
        let started = Instant::now();
        let value = read().await;
        *cached = Some(WakeRead {
            started,
            value: value.clone(),
        });
        (value, started)
    }

    /// Run `scan` while holding the target's scan turn. `woken_at` is when this
    /// waiter was woken; when a scan that began after it already found the
    /// queue empty, this waiter's wake-up is spent and `None` is returned
    /// without scanning. A scan that finds nothing is remembered for the
    /// waiters behind it.
    pub(crate) async fn scan_in_turn<T, F, Fut>(
        &self,
        woken_at: Option<Instant>,
        scan: F,
    ) -> Option<T>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = (T, bool)>,
    {
        let mut memo = self.shared.scan.lock().await;
        if let (Some(woken), Some(empty)) = (woken_at, memo.empty_since) {
            if empty >= woken {
                return None;
            }
        }
        let started = Instant::now();
        let (result, found_nothing) = scan().await;
        if found_nothing {
            memo.empty_since = Some(started);
        }
        Some(result)
    }
}

impl Drop for LeaseWait {
    fn drop(&mut self) {
        let mut state = self.registry.lock();
        state.open -= 1;
        if let Some(open) = state.by_deployment.get_mut(&self.key.0) {
            *open -= 1;
            if *open == 0 {
                state.by_deployment.remove(&self.key.0);
            }
        }
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

    /// Limits with the deployment cap out of the way.
    fn limits(process: usize, target: usize) -> WaitLimits {
        WaitLimits {
            process,
            deployment: usize::MAX,
            target,
        }
    }

    #[test]
    fn entry_is_removed_when_the_last_waiter_leaves() {
        let waiters = Arc::new(LeaseWaiters::default());
        let a = waiters.register("dep", "queue", limits(10, 10)).unwrap();
        let b = waiters.register("dep", "queue", limits(10, 10)).unwrap();
        let other = waiters.register("dep", "other", limits(10, 10)).unwrap();
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
        let first = waiters.register("dep", "queue", limits(1, 1)).unwrap();
        assert!(waiters.register("dep", "other", limits(1, 1)).is_none());
        drop(first);
        assert!(waiters.register("dep", "other", limits(1, 1)).is_some());
    }

    #[test]
    fn registration_is_refused_over_the_per_target_cap() {
        let waiters = Arc::new(LeaseWaiters::default());
        let held: Vec<_> = (0..2)
            .map(|_| waiters.register("dep", "queue", limits(100, 2)).unwrap())
            .collect();
        assert!(waiters.register("dep", "queue", limits(100, 2)).is_none());
        // Another target and another deployment are unaffected.
        assert!(waiters.register("dep", "other", limits(100, 2)).is_some());
        assert!(waiters.register("dep2", "queue", limits(100, 2)).is_some());
        drop(held);
        assert!(waiters.register("dep", "queue", limits(100, 2)).is_some());
    }

    #[test]
    fn invented_targets_cannot_exceed_the_deployment_cap() {
        let waiters = Arc::new(LeaseWaiters::default());
        let limits = WaitLimits {
            process: 1_000,
            deployment: 3,
            target: 8,
        };
        let held: Vec<_> = (0..3)
            .map(|i| {
                waiters
                    .register("dep", &format!("invented-{i}"), limits)
                    .unwrap()
            })
            .collect();
        assert!(waiters.register("dep", "invented-3", limits).is_none());
        assert!(waiters.register("dep", "invented-0", limits).is_none());
        // Another deployment still waits.
        let other = waiters.register("dep2", "invented-0", limits).unwrap();
        assert_eq!(waiters.open_waits(), 4);
        // A slot freed by one wait leaving is reusable, and the count returns
        // to nothing when all are gone.
        drop(held);
        assert!(waiters.register("dep", "invented-9", limits).is_some());
        drop(other);
        assert_eq!(waiters.open_waits(), 0);
        assert!(waiters.lock().by_deployment.is_empty());
    }
}
