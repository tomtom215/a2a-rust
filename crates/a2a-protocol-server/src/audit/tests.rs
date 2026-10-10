// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use a2a_protocol_types::audit::{
    AuditRecord, Checkpoint, CheckpointSigner, SigningAlg, kind, verify_chain,
};
use a2a_protocol_types::error::A2aResult;
use ring::rand::SystemRandom;
use ring::signature::Ed25519KeyPair;

use super::store::{Appended, AuditStore, BoxFuture, InMemoryAuditStore, LegalHold};
use super::{AuditLog, AuditRetention, SIX_MONTHS};

fn signer() -> CheckpointSigner {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    CheckpointSigner::from_pkcs8(SigningAlg::EdDsa, "audit-1", pkcs8.as_ref()).unwrap()
}

fn rec(chain: &str, task: &str) -> AuditRecord {
    let mut r = AuditRecord::new(chain, kind::CALL);
    r.task_id = Some(task.to_owned());
    r
}

/// A sealed record placed at `seq` after `prev`, with a fixed time.
fn sealed(chain: &str, seq: u64, prev: Option<String>, time: &str) -> AuditRecord {
    let mut r = rec(chain, "t");
    r.seal(seq, prev, time.to_owned()).unwrap();
    r
}

// ── The store contract, run against every store ──────────────────────────────

/// What every [`AuditStore`] must do. A store that fails any of this would
/// fork chains, lose records, or return records whose hashes no longer
/// verify.
async fn store_contract(store: &dyn AuditStore) {
    assert_eq!(store.head("c").await.unwrap(), None);
    let r1 = sealed("c", 1, None, "2026-01-01T00:00:00.000Z");
    let r2 = sealed("c", 2, Some(r1.hash.clone()), "2026-03-01T00:00:00.000Z");
    let r3 = sealed("c", 3, Some(r2.hash.clone()), "2026-06-01T00:00:00.000Z");
    for r in [&r1, &r2, &r3] {
        assert_eq!(store.append(r).await.unwrap(), Appended::Stored);
    }
    // A taken position is refused, not overwritten.
    let rival = sealed("c", 3, Some(r2.hash.clone()), "2026-06-02T00:00:00.000Z");
    assert_eq!(store.append(&rival).await.unwrap(), Appended::Conflict);
    assert_eq!(store.head("c").await.unwrap(), Some((3, r3.hash.clone())));

    // Read back exactly as written, so the hashes still verify.
    let all = store.read("c", 0, 100).await.unwrap();
    assert_eq!(all, vec![r1.clone(), r2.clone(), r3.clone()]);
    assert!(verify_chain(&all, &[], &[]).is_intact());
    assert_eq!(store.read("c", 1, 1).await.unwrap(), vec![r2.clone()]);
    assert_eq!(store.read("other", 0, 10).await.unwrap(), Vec::new());

    // Chains are separate.
    let o1 = sealed("other", 1, None, "2026-01-01T00:00:00.000Z");
    store.append(&o1).await.unwrap();
    assert_eq!(
        store.chains().await.unwrap(),
        vec!["c".to_owned(), "other".to_owned()]
    );

    // Checkpoints: stored, ordered, replaced by (seq, kind).
    let cp = Checkpoint::new("checkpoint", "c", 2, r2.hash.clone(), "t1");
    store.put_checkpoint(&cp).await.unwrap();
    let cp2 = Checkpoint::new("checkpoint", "c", 2, r2.hash.clone(), "t2");
    store.put_checkpoint(&cp2).await.unwrap();
    let anchor = Checkpoint::new("anchor", "c", 2, r2.hash.clone(), "t3");
    store.put_checkpoint(&anchor).await.unwrap();
    let cps = store.checkpoints("c").await.unwrap();
    assert_eq!(cps.len(), 2, "{cps:?}");
    assert!(cps.contains(&cp2) && cps.contains(&anchor));

    // last_before: the last of the leading run of records older than the cutoff.
    let april =
        a2a_protocol_types::parse_iso8601_to_unix_millis("2026-04-01T00:00:00.000Z").unwrap();
    assert_eq!(
        store.last_before("c", april).await.unwrap(),
        Some((2, r2.hash.clone()))
    );
    let jan = a2a_protocol_types::parse_iso8601_to_unix_millis("2026-01-01T00:00:00.000Z").unwrap();
    assert_eq!(store.last_before("c", jan).await.unwrap(), None);

    // delete_through removes records and older checkpoints, keeps an anchor at seq.
    let old = Checkpoint::new("checkpoint", "c", 1, r1.hash.clone(), "t0");
    store.put_checkpoint(&old).await.unwrap();
    assert_eq!(store.delete_through("c", 2).await.unwrap(), 2);
    assert_eq!(store.read("c", 0, 10).await.unwrap(), vec![r3.clone()]);
    let left = store.checkpoints("c").await.unwrap();
    assert!(!left.contains(&old), "{left:?}");
    assert!(left.contains(&anchor), "{left:?}");

    // Holds.
    assert_eq!(store.holds().await.unwrap(), Vec::new());
    let hold = LegalHold {
        chain: "c".to_owned(),
        reason: "case 7".to_owned(),
        placed_at: "now".to_owned(),
    };
    store.place_hold(&hold).await.unwrap();
    assert_eq!(store.holds().await.unwrap(), vec![hold]);
    assert!(store.release_hold("c").await.unwrap());
    assert!(!store.release_hold("c").await.unwrap());
}

#[tokio::test]
async fn the_in_memory_store_keeps_the_contract() {
    store_contract(&InMemoryAuditStore::new()).await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn the_sqlite_store_keeps_the_contract() {
    let store = super::SqliteAuditStore::new("sqlite::memory:")
        .await
        .unwrap();
    store_contract(&store).await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "needs A2A_TEST_POSTGRES_URL"]
async fn the_postgres_store_keeps_the_contract() {
    let url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");
    let store = super::PostgresAuditStore::new(&url).await.unwrap();
    for t in [
        "a2a_audit_records",
        "a2a_audit_checkpoints",
        "a2a_audit_holds",
    ] {
        sqlx::query(&format!("DELETE FROM {t}"))
            .execute(
                &crate::store::postgres_store::pool::pg_pool(&url)
                    .await
                    .unwrap(),
            )
            .await
            .unwrap();
    }
    store_contract(&store).await;
}

// ── The log ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn appends_form_a_verifiable_chain_per_tenant() {
    let log = AuditLog::new(Arc::new(InMemoryAuditStore::new()));
    for i in 0..5 {
        log.append(rec("acme", &format!("t{i}"))).await.unwrap();
        log.append(rec("", &format!("u{i}"))).await.unwrap();
    }
    for chain in ["acme", ""] {
        let records = log.export(chain).await.unwrap();
        assert_eq!(
            records.iter().map(|r| r.seq).collect::<Vec<_>>(),
            [1, 2, 3, 4, 5]
        );
        assert!(log.verify(chain).await.unwrap().is_intact());
    }
}

#[tokio::test]
async fn concurrent_appends_to_one_chain_never_fork_it() {
    let log = Arc::new(AuditLog::new(Arc::new(InMemoryAuditStore::new())));
    let mut tasks = Vec::new();
    for i in 0..64 {
        let log = Arc::clone(&log);
        tasks.push(tokio::spawn(async move {
            log.append(rec("c", &format!("t{i}"))).await.unwrap()
        }));
    }
    for t in tasks {
        t.await.unwrap();
    }
    let r = log.verify("c").await.unwrap();
    assert!(r.is_intact(), "{r:?}");
    assert_eq!(r.range, Some((1, 64)));
}

/// A store that refuses the first `conflicts` appends, as a store shared with
/// another replica that keeps winning the race would.
struct Contended {
    inner: InMemoryAuditStore,
    conflicts: AtomicUsize,
}

impl AuditStore for Contended {
    fn head<'a>(&'a self, c: &'a str) -> BoxFuture<'a, A2aResult<Option<(u64, String)>>> {
        self.inner.head(c)
    }
    fn append<'a>(&'a self, r: &'a AuditRecord) -> BoxFuture<'a, A2aResult<Appended>> {
        if self.conflicts.load(Ordering::SeqCst) > 0 {
            self.conflicts.fetch_sub(1, Ordering::SeqCst);
            return Box::pin(async { Ok(Appended::Conflict) });
        }
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
}

#[tokio::test]
async fn a_lost_race_is_retried_and_a_lost_war_is_an_error() {
    let store = Arc::new(Contended {
        inner: InMemoryAuditStore::new(),
        conflicts: AtomicUsize::new(7),
    });
    let log = AuditLog::new(Arc::clone(&store) as Arc<dyn AuditStore>);
    assert_eq!(log.append(rec("c", "t")).await.unwrap().seq, 1);
    store.conflicts.store(8, Ordering::SeqCst);
    assert!(log.append(rec("c", "t")).await.is_err());
}

#[tokio::test]
async fn checkpoints_are_signed_every_n_records_and_verify() {
    let log = AuditLog::new(Arc::new(InMemoryAuditStore::new())).with_signer(signer(), 4);
    for i in 0..10 {
        log.append(rec("c", &format!("t{i}"))).await.unwrap();
    }
    let r = log.verify("c").await.unwrap();
    assert!(r.is_intact(), "{r:?}");
    assert_eq!(r.checkpoints_verified, 2);
    assert_eq!(r.signed_through, Some(8));
    assert_eq!(r.unsigned_tail(), 2);
    let cp = log.checkpoint("c").await.unwrap();
    assert_eq!(cp.seq, 10);
    assert_eq!(log.verify("c").await.unwrap().unsigned_tail(), 0);
}

#[tokio::test]
async fn an_explicit_checkpoint_needs_a_signer_and_a_record() {
    let unsigned = AuditLog::new(Arc::new(InMemoryAuditStore::new()));
    assert!(unsigned.checkpoint("c").await.is_err());
    let signed = AuditLog::new(Arc::new(InMemoryAuditStore::new())).with_signer(signer(), 0);
    assert!(signed.checkpoint("c").await.is_err());
}

#[tokio::test]
async fn the_run_registry_forgets_the_oldest_past_its_bound() {
    let log = AuditLog::new(Arc::new(InMemoryAuditStore::new()));
    for i in 0..=65_536_u64 {
        log.begin_run("c", &format!("t{i}"), i + 1);
    }
    assert_eq!(log.tracked_runs(), 65_536);
    assert_eq!(log.run_of("c", "t0"), None);
    assert_eq!(log.run_of("c", "t65536"), Some(65_537));
    log.end_run("c", "t65536");
    assert_eq!(log.run_of("c", "t65536"), None);
}

// ── Retention and holds ──────────────────────────────────────────────────────

#[test]
fn six_months_is_the_longest_six_calendar_months() {
    assert_eq!(SIX_MONTHS, Duration::from_secs(184 * 86_400));
    assert_eq!(AuditRetention::six_months().keep_for, SIX_MONTHS);
    assert!(AuditRetention::new(Duration::from_secs(183 * 86_400)).is_err());
    assert!(AuditRetention::new(SIX_MONTHS).is_ok());
}

/// A log whose chain "c" holds records sealed at the given days of 2026.
async fn aged_log(days: &[u32]) -> (AuditLog, Arc<InMemoryAuditStore>) {
    let store = Arc::new(InMemoryAuditStore::new());
    let mut prev = None;
    for (i, day) in days.iter().enumerate() {
        let time = a2a_protocol_types::unix_millis_to_iso8601(
            1_767_225_600_000 + i64::from(*day) * 86_400_000,
        );
        let r = sealed("c", i as u64 + 1, prev.clone(), &time);
        prev = Some(r.hash.clone());
        store.append(&r).await.unwrap();
    }
    let log = AuditLog::new(Arc::clone(&store) as Arc<dyn AuditStore>).with_signer(signer(), 0);
    (log, store)
}

const DAY_MS: i64 = 86_400_000;
/// 2026-01-01T00:00:00Z.
const Y2026: i64 = 1_767_225_600_000;

#[tokio::test]
async fn purge_deletes_only_what_is_past_the_floor_and_the_rest_still_verifies() {
    let (log, _store) = aged_log(&[0, 10, 20, 200, 210]).await;
    let retention =
        AuditRetention::allowing_shorter_than_six_months(Duration::from_secs(30 * 86_400));
    // On day 60 the floor is day 30: days 0, 10 and 20 are past it.
    let report = log.purge(retention, Y2026 + 60 * DAY_MS).await.unwrap();
    assert_eq!(report.purged, vec![("c".to_owned(), 3, 3)]);
    let r = log.verify("c").await.unwrap();
    assert!(r.is_intact(), "{r:?}");
    assert_eq!(r.anchored_at, Some(3));
    // Appending after a purge links to what is left.
    log.append(rec("c", "later")).await.unwrap();
    assert!(log.verify("c").await.unwrap().is_intact());
}

#[tokio::test]
async fn purge_never_deletes_the_newest_record() {
    let (log, _store) = aged_log(&[0, 1, 2]).await;
    let retention = AuditRetention::allowing_shorter_than_six_months(Duration::from_secs(86_400));
    let report = log.purge(retention, Y2026 + 400 * DAY_MS).await.unwrap();
    assert_eq!(report.purged, vec![("c".to_owned(), 2, 2)]);
    assert_eq!(log.export("c").await.unwrap().len(), 1);
    assert!(log.verify("c").await.unwrap().is_intact());
}

#[tokio::test]
async fn a_legal_hold_stops_purge_until_released() {
    let (log, _store) = aged_log(&[0, 1, 2, 300]).await;
    let retention = AuditRetention::allowing_shorter_than_six_months(Duration::from_secs(86_400));
    log.place_hold("c", "litigation 2026-17").await.unwrap();
    let held = log.purge(retention, Y2026 + 400 * DAY_MS).await.unwrap();
    assert_eq!(held.held, vec!["c".to_owned()]);
    assert_eq!(held.purged, Vec::new());
    assert_eq!(log.export("c").await.unwrap().len(), 4);
    assert!(log.release_hold("c").await.unwrap());
    let report = log.purge(retention, Y2026 + 400 * DAY_MS).await.unwrap();
    assert_eq!(report.purged.len(), 1);
}

#[tokio::test]
async fn purge_without_a_signer_is_refused() {
    let log = AuditLog::new(Arc::new(InMemoryAuditStore::new()));
    let err = log
        .purge(AuditRetention::six_months(), Y2026)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("signer"), "{err}");
}

// ── The store wrapper forwards everything ────────────────────────────────────

/// Every `TaskStore` method is overridden in `AuditedTaskStore`, so a method
/// added to the trait later with a default cannot silently no-op through the
/// wrapper (CONTRIBUTING.md, "A default is not free").
#[test]
fn the_audited_task_store_overrides_every_task_store_method() {
    // A Windows checkout may have CRLF line endings.
    let trait_src = include_str!("../store/task_store/mod.rs").replace("\r\n", "\n");
    let wrapper_src = include_str!("task_store.rs").replace("\r\n", "\n");
    let methods = |src: &str, start: &str| -> Vec<String> {
        let body = &src[src.find(start).unwrap()..];
        let end = body.find("\n}\n").unwrap();
        body[..end]
            .lines()
            .filter_map(|l| l.trim_start().strip_prefix("fn "))
            .map(|l| l.split(['<', '(']).next().unwrap().to_owned())
            .collect()
    };
    let wanted = methods(&trait_src, "pub trait TaskStore");
    let have = methods(&wrapper_src, "impl TaskStore for AuditedTaskStore");
    assert!(
        wanted.len() >= 19,
        "parsed {} trait methods: {wanted:?}",
        wanted.len()
    );
    let missing: Vec<&String> = wanted.iter().filter(|m| !have.contains(m)).collect();
    assert!(
        missing.is_empty(),
        "AuditedTaskStore does not forward {missing:?}"
    );
}

/// What an operator sees in a `{:?}` of the log: its settings and its
/// failure count, and never the signing key.
#[tokio::test]
async fn the_debug_form_shows_the_settings_and_hides_the_key() {
    let log = AuditLog::new(Arc::new(InMemoryAuditStore::new()))
        .with_signer(signer(), 5)
        .require_record(true);
    let shown = format!("{log:?}");
    for part in [
        "AuditLog",
        "kid: \"audit-1\"",
        "<redacted>",
        "checkpoint_every: 5",
        "require_record: true",
        "failures: 0",
    ] {
        assert!(shown.contains(part), "{part} missing from {shown}");
    }
}

#[test]
fn every_failure_is_counted() {
    let log = AuditLog::new(Arc::new(InMemoryAuditStore::new()));
    assert_eq!(log.failures(), 0);
    log.note_failure();
    log.note_failure();
    assert_eq!(log.failures(), 2);
}

/// A chain whose every stored record is gone — pruned through the store,
/// behind this log's back — continues after its anchor, not from 1, so what
/// is written next still links to what was deleted.
#[tokio::test]
async fn a_chain_emptied_behind_its_anchor_continues_after_it() {
    let store = Arc::new(InMemoryAuditStore::new());
    let log = AuditLog::new(Arc::clone(&store) as Arc<dyn AuditStore>).with_signer(signer(), 0);
    let mut last = None;
    for i in 0..5 {
        last = Some(log.append(rec("c", &format!("t{i}"))).await.unwrap());
    }
    let last = last.unwrap();
    log.sign_checkpoint("anchor", "c", last.seq, &last.hash)
        .await
        .unwrap();
    assert_eq!(store.delete_through("c", last.seq).await.unwrap(), 5);
    assert_eq!(store.head("c").await.unwrap(), None);

    let next = log.append(rec("c", "t5")).await.unwrap();
    assert_eq!(next.seq, 6);
    assert_eq!(next.prev.as_deref(), Some(last.hash.as_str()));
    assert!(log.verify("c").await.unwrap().is_intact());
}

/// An export reads page after page: a chain longer than one read, and one
/// that ends exactly on a page boundary, come back whole.
#[tokio::test]
async fn an_export_returns_every_record_across_pages() {
    for n in [1_000_u64, 2_500] {
        let log = AuditLog::new(Arc::new(InMemoryAuditStore::new()));
        for i in 0..n {
            log.append(rec("c", &format!("t{i}"))).await.unwrap();
        }
        let all = log.export("c").await.unwrap();
        assert_eq!(all.len() as u64, n);
        assert!(all.iter().zip(1..).all(|(r, seq)| r.seq == seq));
        assert!(log.verify("c").await.unwrap().is_intact());
    }
}

/// Ending a run forgets it in the eviction order too, so the registry stays
/// at its bound: the run still tracked is the one evicted when it fills.
#[test]
fn an_ended_run_leaves_the_registry_exactly_at_its_bound() {
    let log = AuditLog::new(Arc::new(InMemoryAuditStore::new()));
    log.begin_run("c", "ended", 1);
    log.begin_run("c", "kept", 2);
    log.end_run("c", "ended");
    for i in 0..65_536_u64 {
        log.begin_run("c", &format!("t{i}"), i + 3);
    }
    assert_eq!(log.tracked_runs(), 65_536);
    assert_eq!(
        log.run_of("c", "kept"),
        None,
        "the oldest live run is evicted"
    );
}

#[test]
fn the_in_memory_store_debug_form_names_it() {
    assert!(format!("{:?}", InMemoryAuditStore::new()).starts_with("InMemoryAuditStore"));
}
