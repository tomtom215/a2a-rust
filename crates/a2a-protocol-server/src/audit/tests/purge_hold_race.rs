// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A hold that has been placed stops a purge that began before it
//! (adversarial review A1, 2026-11-12).
//!
//! `purge` used to read the holds once, and delete from the chains it had
//! found unheld however long afterwards; `place_hold` took the chain's lock,
//! which `purge` did not. A hold that *completed* in between therefore did not
//! stop the deletion of the records it was placed to keep. The invariant these
//! tests hold the code to: if a hold completes before a deletion starts, the
//! deletion removes nothing from the held chain.
//!
//! The store's deletion is made to wait, so the purge is paused after it has
//! read the holds and decided what to delete, and a hold is placed then.

use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

use super::*;

/// A store whose deletions wait until the test lets one go.
pub(super) struct Gated {
    inner: Arc<dyn AuditStore>,
    /// A deletion has arrived and is waiting.
    pub(super) entered: Notify,
    /// Lets the waiting deletion go.
    pub(super) release: Notify,
    pub(super) deletions: AtomicUsize,
}

impl Gated {
    pub(super) fn new(inner: Arc<dyn AuditStore>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            entered: Notify::new(),
            release: Notify::new(),
            deletions: AtomicUsize::new(0),
        })
    }

    /// Waits at the deletion until released.
    async fn pause(&self) {
        self.deletions.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        self.release.notified().await;
    }
}

impl AuditStore for Gated {
    fn head<'a>(&'a self, c: &'a str) -> BoxFuture<'a, A2aResult<Option<(u64, String)>>> {
        self.inner.head(c)
    }
    fn append<'a>(&'a self, r: &'a AuditRecord) -> BoxFuture<'a, A2aResult<Appended>> {
        self.inner.append(r)
    }
    fn read<'a>(
        &'a self,
        c: &'a str,
        a: u64,
        l: usize,
    ) -> BoxFuture<'a, A2aResult<Vec<AuditRecord>>> {
        self.inner.read(c, a, l)
    }
    fn chains(&self) -> BoxFuture<'_, A2aResult<Vec<String>>> {
        self.inner.chains()
    }
    fn put_checkpoint<'a>(&'a self, c: &'a Checkpoint) -> BoxFuture<'a, A2aResult<()>> {
        self.inner.put_checkpoint(c)
    }
    fn checkpoints<'a>(&'a self, c: &'a str) -> BoxFuture<'a, A2aResult<Vec<Checkpoint>>> {
        self.inner.checkpoints(c)
    }
    fn last_before<'a>(
        &'a self,
        c: &'a str,
        ms: i64,
    ) -> BoxFuture<'a, A2aResult<Option<(u64, String)>>> {
        self.inner.last_before(c, ms)
    }
    fn delete_through<'a>(&'a self, c: &'a str, s: u64) -> BoxFuture<'a, A2aResult<u64>> {
        Box::pin(async move {
            self.pause().await;
            self.inner.delete_through(c, s).await
        })
    }
    fn delete_through_unless_held<'a>(
        &'a self,
        c: &'a str,
        s: u64,
    ) -> BoxFuture<'a, A2aResult<Option<u64>>> {
        Box::pin(async move {
            self.pause().await;
            self.inner.delete_through_unless_held(c, s).await
        })
    }
    fn place_hold<'a>(&'a self, h: &'a LegalHold) -> BoxFuture<'a, A2aResult<()>> {
        self.inner.place_hold(h)
    }
    fn release_hold<'a>(&'a self, c: &'a str) -> BoxFuture<'a, A2aResult<bool>> {
        self.inner.release_hold(c)
    }
    fn holds(&self) -> BoxFuture<'_, A2aResult<Vec<LegalHold>>> {
        self.inner.holds()
    }
}

/// Seeds chain `c` of `store` with records sealed on days 0, 1, 2 and 300.
async fn seed(store: &dyn AuditStore) {
    let mut prev = None;
    for (i, day) in [0_i64, 1, 2, 300].iter().enumerate() {
        let time = a2a_protocol_types::unix_millis_to_iso8601(Y2026 + day * DAY_MS);
        let r = sealed("c", i as u64 + 1, prev.clone(), &time);
        prev = Some(r.hash.clone());
        store.append(&r).await.unwrap();
    }
}

/// Pauses a purge at its deletion, completes a hold if it can (from `holder`),
/// lets the purge go, and returns whether the hold completed first and what
/// of the four original records is left.
async fn hold_during_purge(
    store: Arc<dyn AuditStore>,
    holder_shares_the_log: bool,
) -> (bool, Vec<u64>) {
    seed(&*store).await;
    let gated = Gated::new(Arc::clone(&store));
    let purger =
        Arc::new(AuditLog::new(Arc::clone(&gated) as Arc<dyn AuditStore>).with_signer(signer(), 0));
    let holder = if holder_shares_the_log {
        Arc::clone(&purger)
    } else {
        // Another replica: its own log, its own chain lock, the same store.
        Arc::new(AuditLog::new(Arc::clone(&store)))
    };
    let retention = AuditRetention::allowing_shorter_than_six_months(Duration::from_hours(24));
    let purge = tokio::spawn({
        let purger = Arc::clone(&purger);
        async move { purger.purge(retention, Y2026 + 400 * DAY_MS).await }
    });
    // The purge has read the holds and is about to delete.
    tokio::time::timeout(Duration::from_secs(10), gated.entered.notified())
        .await
        .expect("the purge reached its deletion");

    let mut hold = tokio::spawn(async move {
        holder
            .place_hold("c", "litigation 2026-17", "alice")
            .await
            .unwrap();
    });
    let timed = tokio::time::timeout(Duration::from_millis(300), &mut hold).await;
    let completed_first = timed.is_ok();
    if let Ok(joined) = timed {
        joined.unwrap();
    }

    gated.release.notify_one();
    purge.await.unwrap().unwrap();
    if !completed_first {
        hold.await.unwrap();
    }

    let left = store
        .read("c", 0, 100)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.seq)
        .filter(|seq| *seq <= 4)
        .collect();
    (completed_first, left)
}

async fn assert_the_hold_won(store: Arc<dyn AuditStore>, shared: bool) {
    let (completed_first, left) = hold_during_purge(store, shared).await;
    if completed_first {
        assert_eq!(
            left,
            [1, 2, 3, 4],
            "a hold completed before the purge deleted, and the purge deleted from the held chain"
        );
    }
}

#[tokio::test]
async fn a_hold_completed_during_a_purge_of_one_log_stops_its_deletion_in_memory() {
    assert_the_hold_won(Arc::new(InMemoryAuditStore::new()), true).await;
}

#[tokio::test]
async fn a_hold_completed_during_a_purge_by_another_log_stops_its_deletion_in_memory() {
    assert_the_hold_won(Arc::new(InMemoryAuditStore::new()), false).await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn a_hold_completed_during_a_purge_of_one_log_stops_its_deletion_in_sqlite() {
    let store = super::super::SqliteAuditStore::new("sqlite::memory:")
        .await
        .unwrap();
    assert_the_hold_won(Arc::new(store), true).await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn a_hold_completed_during_a_purge_by_another_log_stops_its_deletion_in_sqlite() {
    let store = super::super::SqliteAuditStore::new("sqlite::memory:")
        .await
        .unwrap();
    assert_the_hold_won(Arc::new(store), false).await;
}
