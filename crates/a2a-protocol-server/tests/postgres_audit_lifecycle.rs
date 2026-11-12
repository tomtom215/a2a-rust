// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Two audit lifecycle guarantees over a live PostgreSQL database (adversarial
//! review A1 and A2, 2026-11-12).
//!
//! * **A2.** An append holds one connection for its transaction and must not
//!   wait for a second while it does. With a pool of two and two first appends
//!   to two empty chains, both transactions are open at once; sealing used to
//!   read the chain's latest anchor from the same pool, so each waited for a
//!   connection the other held, and both failed when the pool's acquire
//!   timeout ran out. The pool is not enlarged to make them pass.
//! * **A1.** A legal hold that completed on one replica stops the purge another
//!   replica had already begun: the deletion checks the hold in the same
//!   transaction that deletes.
//!
//! Every test is `#[ignore]`d for want of a live server and creates and drops
//! a scratch database of its own:
//!
//! ```bash
//! A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres \
//!   cargo test -p a2a-protocol-server --features postgres,audit \
//!   --test postgres_audit_lifecycle -- --ignored
//! ```

#![cfg(all(feature = "postgres", feature = "audit"))]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use a2a_protocol_server::audit::record::{AuditRecord, Checkpoint, kind};
use a2a_protocol_server::audit::{
    Appended, AuditLog, AuditRetention, AuditStore, BoxFuture, LegalHold, PostgresAuditStore,
    Sealer, SubjectKey,
};
use a2a_protocol_types::audit::{CheckpointSigner, SigningAlg};
use a2a_protocol_types::error::A2aResult;
use ring::rand::SystemRandom;
use ring::signature::Ed25519KeyPair;
use sqlx::postgres::{PgPool, PgPoolOptions};
use tokio::sync::{Barrier, Notify};

const URL_ENV: &str = "A2A_TEST_POSTGRES_URL";
const DAY_MS: i64 = 86_400_000;
/// 2026-01-01T00:00:00Z.
const Y2026: i64 = 1_767_225_600_000;

/// A scratch database for one test.
struct Scratch {
    admin_url: String,
    name: String,
    url: String,
}

impl Scratch {
    async fn create(tag: &str) -> Self {
        let admin_url = std::env::var(URL_ENV)
            .unwrap_or_else(|_| panic!("{URL_ENV} must point at a live PostgreSQL server"));
        let (base, _) = admin_url.rsplit_once('/').expect("a database in the URL");
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let name = format!("a2a_test_audit_{tag}_{nanos}");
        let admin = PgPool::connect(&admin_url).await.expect("admin");
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE \"{name}\"")))
            .execute(&admin)
            .await
            .expect("create database");
        admin.close().await;
        let url = format!("{base}/{name}");
        Self {
            admin_url,
            name,
            url,
        }
    }

    /// A store over a pool of `connections`, whose connections wait at most
    /// three seconds for one another.
    async fn store(&self, connections: u32) -> PostgresAuditStore {
        let pool = PgPoolOptions::new()
            .max_connections(connections)
            .acquire_timeout(Duration::from_secs(3))
            .connect(&self.url)
            .await
            .expect("connect");
        PostgresAuditStore::from_pool(pool).await.expect("schema")
    }

    async fn drop_db(self) {
        let admin = PgPool::connect(&self.admin_url).await.expect("admin");
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
            self.name
        )))
        .execute(&admin)
        .await
        .expect("drop database");
        admin.close().await;
    }
}

fn record(chain: &str, task: &str) -> AuditRecord {
    let mut r = AuditRecord::new(chain, kind::CALL);
    r.task_id = Some(task.to_owned());
    r
}

fn signer() -> CheckpointSigner {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    CheckpointSigner::from_pkcs8(SigningAlg::EdDsa, "audit-1", pkcs8.as_ref()).unwrap()
}

/// A store that makes every `append_next` wait, once its sealer has been
/// called (so its transaction holds a connection), until `parties` appends are
/// sealing at once.
struct Meeting {
    inner: PostgresAuditStore,
    barrier: Arc<Barrier>,
}

impl AuditStore for Meeting {
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
        let barrier = Arc::clone(&self.barrier);
        Box::pin(async move {
            let meet = move |head: Option<(u64, String)>, ms: Option<i64>| {
                let barrier = Arc::clone(&barrier);
                Box::pin(async move {
                    // Each appender is inside its transaction, holding its
                    // connection, when it gets here.
                    let _ = tokio::time::timeout(Duration::from_secs(20), barrier.wait()).await;
                    seal(head, ms).await
                }) as BoxFuture<'_, _>
            };
            self.inner.append_next(chain, &meet).await
        })
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
        self.inner.delete_through(c, s)
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
    fn supports_subject_keys(&self) -> bool {
        self.inner.supports_subject_keys()
    }
    fn subject_key<'a>(
        &'a self,
        c: &'a str,
        l: &'a str,
    ) -> BoxFuture<'a, A2aResult<Option<SubjectKey>>> {
        self.inner.subject_key(c, l)
    }
    fn subject_key_by_sid<'a>(
        &'a self,
        c: &'a str,
        s: &'a str,
    ) -> BoxFuture<'a, A2aResult<Option<SubjectKey>>> {
        self.inner.subject_key_by_sid(c, s)
    }
    fn subject_keys<'a>(&'a self, c: &'a str) -> BoxFuture<'a, A2aResult<Vec<SubjectKey>>> {
        self.inner.subject_keys(c)
    }
    fn put_subject_key<'a>(
        &'a self,
        c: &'a str,
        k: &'a SubjectKey,
    ) -> BoxFuture<'a, A2aResult<bool>> {
        self.inner.put_subject_key(c, k)
    }
    fn delete_subject_key<'a>(
        &'a self,
        c: &'a str,
        l: &'a str,
    ) -> BoxFuture<'a, A2aResult<Option<String>>> {
        self.inner.delete_subject_key(c, l)
    }
}

/// The review's A2 sequence, with the standard `AuditLog` sealing: one pool of
/// two connections, two empty chains, both appends inside their transactions
/// before either seals. Both must complete without the pool being enlarged.
#[tokio::test]
#[ignore = "requires a live PostgreSQL server (set A2A_TEST_POSTGRES_URL)"]
async fn two_first_appends_to_two_empty_chains_complete_on_a_pool_of_two() {
    let db = Scratch::create("pool2").await;
    let store = Meeting {
        inner: db.store(2).await,
        barrier: Arc::new(Barrier::new(2)),
    };
    let log = Arc::new(AuditLog::new(Arc::new(store)));

    let (a, b) = {
        let (la, lb) = (Arc::clone(&log), Arc::clone(&log));
        tokio::join!(
            tokio::spawn(async move { la.append(record("chain-a", "t")).await }),
            tokio::spawn(async move { lb.append(record("chain-b", "t")).await }),
        )
    };
    let (a, b) = (a.unwrap(), b.unwrap());
    assert!(
        a.is_ok() && b.is_ok(),
        "appends starved the pool of the connection each needed to seal: {a:?} {b:?}"
    );
    for chain in ["chain-a", "chain-b"] {
        assert_eq!(log.export(chain).await.unwrap().len(), 1);
        assert!(log.verify(chain).await.unwrap().is_intact());
    }
    db.drop_db().await;
}

/// The same, after a purge left an anchor: an empty chain continues from its
/// latest anchor, which sealing used to read from the pool.
#[tokio::test]
#[ignore = "requires a live PostgreSQL server (set A2A_TEST_POSTGRES_URL)"]
async fn first_appends_after_an_anchor_complete_on_a_pool_of_two() {
    let db = Scratch::create("pool2anchor").await;
    // Two chains, each with one anchor and nothing else left of it is not
    // reachable (the newest record is never purged), so only the empty-chain
    // read is exercised: the chains are new.
    let store = Meeting {
        inner: db.store(2).await,
        barrier: Arc::new(Barrier::new(2)),
    };
    let log = Arc::new(AuditLog::new(Arc::new(store)).with_signer(signer(), 1));
    let (la, lb) = (Arc::clone(&log), Arc::clone(&log));
    let (a, b) = tokio::join!(
        tokio::spawn(async move { la.append(record("x", "t")).await }),
        tokio::spawn(async move { lb.append(record("y", "t")).await }),
    );
    assert!(a.unwrap().is_ok() && b.unwrap().is_ok());
    db.drop_db().await;
}

/// The same with `AuditLog::with_privacy`: each record names an actor, whose
/// key sealing found (or made) in the store while the append held its
/// connection. Both appends must complete on a pool of two.
#[cfg(feature = "encryption")]
#[tokio::test]
#[ignore = "requires a live PostgreSQL server (set A2A_TEST_POSTGRES_URL)"]
async fn two_private_first_appends_complete_on_a_pool_of_two() {
    use a2a_protocol_server::audit::{AuditPrivacy, HMAC_KEY_LEN};
    use a2a_protocol_server::store::encrypted::{AES_256_KEY_LEN, Aes256GcmCipher};
    use a2a_protocol_types::audit::Actor;

    let db = Scratch::create("pool2priv").await;
    let kek = Arc::new(Aes256GcmCipher::new("kek-1", [5; AES_256_KEY_LEN]).unwrap());
    let privacy = AuditPrivacy::new(kek, "h1", [6; HMAC_KEY_LEN]).unwrap();
    let store = Meeting {
        inner: db.store(2).await,
        barrier: Arc::new(Barrier::new(2)),
    };
    let log = Arc::new(
        AuditLog::new(Arc::new(store))
            .with_privacy(privacy)
            .expect("the store keeps subject keys"),
    );
    let named = |chain: &str, who: &str| {
        let mut r = record(chain, "t");
        r.actor = Some(Actor::new(who, Some("bearer".to_owned())));
        r
    };
    let (la, lb) = (Arc::clone(&log), Arc::clone(&log));
    let (ra, rb) = (named("p-a", "alice"), named("p-b", "bob"));
    let (a, b) = tokio::join!(
        tokio::spawn(async move { la.append(ra).await }),
        tokio::spawn(async move { lb.append(rb).await }),
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert!(a.is_ok() && b.is_ok(), "{a:?} {b:?}");
    for chain in ["p-a", "p-b"] {
        assert!(log.verify(chain).await.unwrap().is_intact());
        let exported = log.export(chain).await.unwrap();
        let subject = &exported[0].actor.as_ref().expect("an actor").subject;
        assert!(
            subject.starts_with("a2a-sealed:v1:"),
            "the actor is sealed, not in the clear: {subject}"
        );
    }
    db.drop_db().await;
}

/// A store that waits at its hold-checked deletion until released.
struct Gated {
    inner: PostgresAuditStore,
    entered: Notify,
    release: Notify,
    deletions: AtomicUsize,
}

impl AuditStore for Gated {
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
        self.inner.append_next(chain, seal)
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

impl Gated {
    async fn pause(&self) {
        self.deletions.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        self.release.notified().await;
    }
}

/// A1 across replicas on PostgreSQL: replica A's purge has read "no hold" and
/// is about to delete; replica B (its own log, pool and chain lock) places a
/// hold, which completes; A's deletion then runs. It must remove nothing.
#[tokio::test]
#[ignore = "requires a live PostgreSQL server (set A2A_TEST_POSTGRES_URL)"]
async fn a_hold_completed_on_another_replica_stops_a_purge_that_had_begun() {
    let db = Scratch::create("hold").await;
    let seeder = AuditLog::new(Arc::new(db.store(3).await));
    // Records sealed now, so every one is older than the cutoff chosen below.
    for i in 0..4 {
        seeder.append(record("c", &format!("t{i}"))).await.unwrap();
    }
    let gated = Arc::new(Gated {
        inner: db.store(3).await,
        entered: Notify::new(),
        release: Notify::new(),
        deletions: AtomicUsize::new(0),
    });
    let purger =
        Arc::new(AuditLog::new(Arc::clone(&gated) as Arc<dyn AuditStore>).with_signer(signer(), 0));
    let holder = AuditLog::new(Arc::new(db.store(3).await));
    let retention = AuditRetention::allowing_shorter_than_six_months(Duration::from_secs(1));
    let far_future = Y2026 + 3_650 * DAY_MS;

    let purge = tokio::spawn({
        let purger = Arc::clone(&purger);
        async move { purger.purge(retention, far_future).await }
    });
    tokio::time::timeout(Duration::from_secs(20), gated.entered.notified())
        .await
        .expect("the purge reached its deletion");

    holder
        .place_hold("c", "litigation 2026-17", "alice")
        .await
        .expect("the hold completes while the purge waits");

    gated.release.notify_one();
    purge.await.unwrap().unwrap();

    let reader = AuditLog::new(Arc::new(db.store(3).await));
    let left: Vec<u64> = reader
        .export("c")
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.seq)
        .filter(|seq| *seq <= 4)
        .collect();
    assert_eq!(
        left,
        [1, 2, 3, 4],
        "the purge deleted from a chain whose hold had completed"
    );
    db.drop_db().await;
}
