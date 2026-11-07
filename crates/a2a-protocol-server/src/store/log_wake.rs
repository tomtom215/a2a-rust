// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Waking a stream that waits on a task's event log (audit E-F25 (b), ADR
//! 0057).
//!
//! A `SubscribeToTask` stream on a replica that does not run the task learns
//! of its events by reading the store, and until this existed it read on a
//! timer. [`TaskStore::watch_event_log`](crate::store::TaskStore::watch_event_log)
//! lets a store say "something changed" instead, so the timer becomes the
//! fallback it should have been. The types here are what a store hands back
//! ([`LogWatch`], resolving to a [`LogWake`]); [`LogWakers`] is the registry
//! the stores in this crate that can push keep their waiters in.

use std::future::Future;
use std::pin::Pin;

/// Why a wait on a task's event log ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LogWake {
    /// A notification named this task: an event was appended to its log.
    Notified,
    /// The store lost the source of its notifications and has it again.
    /// Whatever was sent in between is gone, so every waiter is woken to look
    /// for itself.
    Resynced,
}

impl LogWake {
    /// The [`subscribe_wake`](crate::metrics::subscribe_wake) label this wake
    /// is reported under.
    pub(crate) const fn cause(self) -> &'static str {
        match self {
            Self::Notified => crate::metrics::subscribe_wake::NOTIFICATION,
            Self::Resynced => crate::metrics::subscribe_wake::RESYNC,
        }
    }
}

/// A registered interest in one task's event log, from
/// [`TaskStore::watch_event_log`](crate::store::TaskStore::watch_event_log).
///
/// It resolves when the log may have changed since it was created, and ends
/// the interest when it is dropped. The interest begins when the watch is
/// created, not when it is first polled: a stream creates one, reads the
/// store, and only then waits on it, so a change made between the read and
/// the wait is not lost.
pub type LogWatch = Pin<Box<dyn Future<Output = LogWake> + Send>>;

#[cfg(any(feature = "postgres", test))]
pub use registry::{LogWakers, key};

#[cfg(any(feature = "postgres", test))]
mod registry {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

    use tokio::sync::watch;

    use super::{LogWake, LogWatch};

    /// The most bytes a task's key may take, which is the payload of its
    /// notification. `PostgreSQL` refuses one of 8,000 bytes or more; this is far
    /// below it, because a task id is chosen by a client (a continuation names
    /// it) and every replica reads every notification. A task whose key does
    /// not fit is never announced and never watched: it is polled, as it was.
    pub const MAX_KEY_BYTES: usize = 512;

    /// The key a task is announced under, `{partition length}:{partition}{task}`,
    /// or `None` when it is longer than [`MAX_KEY_BYTES`].
    ///
    /// The partition is the tenant (and caller) the task is stored under, `""`
    /// for the plain stores. The length comes first so that two different pairs
    /// never share a key, whatever characters either holds, and so that a
    /// receiver needs no parser: the key is looked up as it arrives.
    pub fn key(partition: &str, task: &str) -> Option<String> {
        let key = format!("{}:{partition}{task}", partition.len());
        (key.len() <= MAX_KEY_BYTES).then_some(key)
    }

    /// One watched task: a counter its waiters watch, and how many there are.
    #[derive(Debug)]
    struct Entry {
        tx: watch::Sender<u64>,
        waiters: usize,
    }

    #[derive(Debug)]
    struct Inner {
        /// Only keys with a waiter: an entry is made by the first and removed
        /// by the last, so the map is as big as the streams waiting and a
        /// notification for any other task is one failed lookup.
        tasks: Mutex<HashMap<String, Entry>>,
        /// Counts the times every waiter was told to look.
        resync: watch::Sender<u64>,
    }

    impl Inner {
        fn tasks(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
            // Nothing that holds the lock can panic part-way through a change.
            self.tasks.lock().unwrap_or_else(PoisonError::into_inner)
        }
    }

    /// The streams waiting on each task, and the means to wake them.
    #[derive(Debug, Clone)]
    pub struct LogWakers {
        inner: Arc<Inner>,
    }

    /// Ends one waiter's interest when its [`LogWatch`] is dropped, finished
    /// or not.
    struct Interest {
        inner: Arc<Inner>,
        key: String,
    }

    impl Drop for Interest {
        fn drop(&mut self) {
            let mut tasks = self.inner.tasks();
            if let Some(entry) = tasks.get_mut(&self.key) {
                entry.waiters -= 1;
                if entry.waiters == 0 {
                    tasks.remove(&self.key);
                }
            }
        }
    }

    impl LogWakers {
        pub(crate) fn new() -> Self {
            Self {
                inner: Arc::new(Inner {
                    tasks: Mutex::new(HashMap::new()),
                    resync: watch::channel(0).0,
                }),
            }
        }

        /// Registers a waiter on `key` (see [`key`]) now, and returns the
        /// future that resolves when it is woken.
        pub(crate) fn watch(&self, key: String) -> LogWatch {
            let mut task = {
                let mut tasks = self.inner.tasks();
                let entry = tasks.entry(key.clone()).or_insert_with(|| Entry {
                    tx: watch::channel(0).0,
                    waiters: 0,
                });
                entry.waiters += 1;
                let receiver = entry.tx.subscribe();
                drop(tasks);
                receiver
            };
            let mut resync = self.inner.resync.subscribe();
            let interest = Interest {
                inner: Arc::clone(&self.inner),
                key,
            };
            Box::pin(async move {
                let _interest = interest;
                // Notified first when both are ready: it names the task.
                tokio::select! {
                    biased;
                    _ = task.changed() => LogWake::Notified,
                    _ = resync.changed() => LogWake::Resynced,
                }
            })
        }

        /// Wakes the waiters on `key`, if any.
        pub(crate) fn notify(&self, key: &str) {
            if let Some(entry) = self.inner.tasks().get(key) {
                entry.tx.send_modify(|count| *count = count.wrapping_add(1));
            }
        }

        /// Wakes every waiter, for a loss of notifications.
        pub(crate) fn resync(&self) {
            self.inner
                .resync
                .send_modify(|count| *count = count.wrapping_add(1));
        }

        /// How many keys have a waiter, for a test of cleanup.
        #[cfg(test)]
        pub(crate) fn watched(&self) -> usize {
            self.inner.tasks().len()
        }
    }
}

#[cfg(test)]
pub mod testing;
#[cfg(test)]
mod tests;
