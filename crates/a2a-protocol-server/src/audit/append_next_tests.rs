// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `AuditStore::append_next` and the time a record is sealed with (audit
//! E-F18). The PostgreSQL override is exercised in
//! `tests/postgres_audit_append.rs`.

use std::sync::Arc;
use std::sync::Mutex;

use a2a_protocol_types::audit::{AuditRecord, kind};
use a2a_protocol_types::error::A2aResult;

use super::{Appended, AuditLog, AuditStore, BoxFuture, InMemoryAuditStore, Sealer};

fn rec() -> AuditRecord {
    AuditRecord::new("c", kind::CALL)
}

type Seen = Mutex<Vec<(Option<(u64, String)>, Option<i64>)>>;

/// Seals a record after `head` the way `AuditLog` does, with a fixed time so a
/// test can tell it from the writer's clock, and notes what it was given.
fn sealing(
    seen: &Seen,
    head: Option<(u64, String)>,
    ms: Option<i64>,
) -> BoxFuture<'_, A2aResult<AuditRecord>> {
    seen.lock().unwrap().push((head.clone(), ms));
    Box::pin(async move {
        let (seq, prev) = match head {
            Some((seq, hash)) => (seq + 1, Some(hash)),
            None => (1, None),
        };
        let mut r = rec();
        r.seal(seq, prev, "2026-01-01T00:00:00.000Z".to_owned())?;
        Ok(r)
    })
}

/// The default reads the head, has the record sealed after it with no clock of
/// the store's, and stores it.
#[tokio::test]
async fn the_default_append_next_reads_the_head_seals_and_stores() {
    let store = InMemoryAuditStore::new();
    let seen = Seen::default();
    let seal = |head, ms| sealing(&seen, head, ms);

    let first = store.append_next("c", &seal).await.unwrap().unwrap();
    let second = store.append_next("c", &seal).await.unwrap().unwrap();

    assert_eq!((first.seq, second.seq), (1, 2));
    assert_eq!(second.prev.as_deref(), Some(first.hash.as_str()));
    assert_eq!(
        store.read("c", 0, 10).await.unwrap(),
        vec![first.clone(), second]
    );
    assert_eq!(
        *seen.lock().unwrap(),
        vec![(None, None), (Some((1, first.hash)), None)],
        "the head each time, and no store clock"
    );
}

/// A position another writer took is `None`, and nothing is stored in its
/// place.
#[tokio::test]
#[allow(clippy::too_many_lines)] // a store that must implement every method
async fn the_default_append_next_says_none_when_the_position_was_taken() {
    /// Reports an empty chain whatever it holds, as a reader that lost the race
    /// to a writer that has since stored would see.
    struct StaleHead(InMemoryAuditStore);
    impl AuditStore for StaleHead {
        fn head<'a>(&'a self, _: &'a str) -> BoxFuture<'a, A2aResult<Option<(u64, String)>>> {
            Box::pin(async { Ok(None) })
        }
        fn append<'a>(&'a self, r: &'a AuditRecord) -> BoxFuture<'a, A2aResult<Appended>> {
            self.0.append(r)
        }
        fn read<'a>(
            &'a self,
            c: &'a str,
            after: u64,
            limit: usize,
        ) -> BoxFuture<'a, A2aResult<Vec<AuditRecord>>> {
            self.0.read(c, after, limit)
        }
        fn chains(&self) -> BoxFuture<'_, A2aResult<Vec<String>>> {
            self.0.chains()
        }
        fn put_checkpoint<'a>(
            &'a self,
            c: &'a a2a_protocol_types::audit::Checkpoint,
        ) -> BoxFuture<'a, A2aResult<()>> {
            self.0.put_checkpoint(c)
        }
        fn checkpoints<'a>(
            &'a self,
            c: &'a str,
        ) -> BoxFuture<'a, A2aResult<Vec<a2a_protocol_types::audit::Checkpoint>>> {
            self.0.checkpoints(c)
        }
        fn last_before<'a>(
            &'a self,
            c: &'a str,
            cutoff: i64,
        ) -> BoxFuture<'a, A2aResult<Option<(u64, String)>>> {
            self.0.last_before(c, cutoff)
        }
        fn delete_through<'a>(&'a self, c: &'a str, seq: u64) -> BoxFuture<'a, A2aResult<u64>> {
            self.0.delete_through(c, seq)
        }
        fn place_hold<'a>(&'a self, h: &'a super::LegalHold) -> BoxFuture<'a, A2aResult<()>> {
            self.0.place_hold(h)
        }
        fn release_hold<'a>(&'a self, c: &'a str) -> BoxFuture<'a, A2aResult<bool>> {
            self.0.release_hold(c)
        }
        fn holds(&self) -> BoxFuture<'_, A2aResult<Vec<super::LegalHold>>> {
            self.0.holds()
        }
    }

    let store = StaleHead(InMemoryAuditStore::new());
    let seen = Seen::default();
    let seal = |head, ms| sealing(&seen, head, ms);
    assert!(store.append_next("c", &seal).await.unwrap().is_some());
    assert_eq!(
        store.append_next("c", &seal).await.unwrap(),
        None,
        "position 1 is taken"
    );
    assert_eq!(store.0.read("c", 0, 10).await.unwrap().len(), 1);
}

/// A store that gives its own clock times the record by it, and one that does
/// not leaves the writer's: the log seals with whichever it is handed.
#[tokio::test]
#[allow(clippy::too_many_lines)] // a store that must implement every method
async fn a_record_is_timed_by_the_stores_clock_when_it_gives_one() {
    /// Calls the sealer with a fixed clock, then stores what it returns.
    struct Clocked {
        inner: InMemoryAuditStore,
        clock_ms: Option<i64>,
    }
    impl AuditStore for Clocked {
        fn head<'a>(&'a self, c: &'a str) -> BoxFuture<'a, A2aResult<Option<(u64, String)>>> {
            self.inner.head(c)
        }
        fn append<'a>(&'a self, r: &'a AuditRecord) -> BoxFuture<'a, A2aResult<Appended>> {
            self.inner.append(r)
        }
        fn append_next<'a>(
            &'a self,
            chain: &'a str,
            seal: &'a Sealer<'a>,
        ) -> BoxFuture<'a, A2aResult<Option<AuditRecord>>> {
            Box::pin(async move {
                let head = self.inner.head(chain).await?;
                let record = seal(head, self.clock_ms).await?;
                Ok((self.inner.append(&record).await? == Appended::Stored).then_some(record))
            })
        }
        fn read<'a>(
            &'a self,
            c: &'a str,
            after: u64,
            limit: usize,
        ) -> BoxFuture<'a, A2aResult<Vec<AuditRecord>>> {
            self.inner.read(c, after, limit)
        }
        fn chains(&self) -> BoxFuture<'_, A2aResult<Vec<String>>> {
            self.inner.chains()
        }
        fn put_checkpoint<'a>(
            &'a self,
            c: &'a a2a_protocol_types::audit::Checkpoint,
        ) -> BoxFuture<'a, A2aResult<()>> {
            self.inner.put_checkpoint(c)
        }
        fn checkpoints<'a>(
            &'a self,
            c: &'a str,
        ) -> BoxFuture<'a, A2aResult<Vec<a2a_protocol_types::audit::Checkpoint>>> {
            self.inner.checkpoints(c)
        }
        fn last_before<'a>(
            &'a self,
            c: &'a str,
            cutoff: i64,
        ) -> BoxFuture<'a, A2aResult<Option<(u64, String)>>> {
            self.inner.last_before(c, cutoff)
        }
        fn delete_through<'a>(&'a self, c: &'a str, seq: u64) -> BoxFuture<'a, A2aResult<u64>> {
            self.inner.delete_through(c, seq)
        }
        fn place_hold<'a>(&'a self, h: &'a super::LegalHold) -> BoxFuture<'a, A2aResult<()>> {
            self.inner.place_hold(h)
        }
        fn release_hold<'a>(&'a self, c: &'a str) -> BoxFuture<'a, A2aResult<bool>> {
            self.inner.release_hold(c)
        }
        fn holds(&self) -> BoxFuture<'_, A2aResult<Vec<super::LegalHold>>> {
            self.inner.holds()
        }
    }

    let log = AuditLog::new(Arc::new(Clocked {
        inner: InMemoryAuditStore::new(),
        clock_ms: Some(1_700_000_000_123),
    }));
    let sealed = log.append(rec()).await.unwrap();
    assert_eq!(sealed.time, "2023-11-14T22:13:20.123Z");
    assert!(log.verify("c").await.unwrap().is_intact());

    let log = AuditLog::new(Arc::new(Clocked {
        inner: InMemoryAuditStore::new(),
        clock_ms: None,
    }));
    let before = a2a_protocol_types::utc_now_iso8601();
    let sealed = log.append(rec()).await.unwrap();
    let after = a2a_protocol_types::utc_now_iso8601();
    // Fixed-width ISO 8601 strings order as the instants do.
    assert!(
        before <= sealed.time && sealed.time <= after,
        "{before} <= {} <= {after}",
        sealed.time
    );
}
