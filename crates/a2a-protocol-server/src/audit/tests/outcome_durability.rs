// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A required log does not lose the outcome of a call it admitted
//! (adversarial review A3, 2026-11-12).
//!
//! Required mode gated admission on the `call.started` record and nothing
//! after it: when the store refused the `call` record that says how the call
//! ended, the failure was counted and the outcome was gone, so the chain held
//! a start with no end and nothing said so. A hash chain proves the records it
//! holds are intact; it cannot show that a record never written is missing.
//!
//! The store is made to refuse appends for a while, as a database does during a
//! failover, and heal.

use std::sync::atomic::AtomicBool;

use a2a_protocol_types::audit::kind;
use a2a_protocol_types::error::A2aError;

use super::*;
use crate::audit::interceptor::AuditInterceptor;
use crate::call_context::CallContext;
use crate::interceptor::{CallOutcome, ServerInterceptor};
use crate::metrics::NoopMetrics;

/// A store that refuses every append while `down` is set.
pub struct Flaky {
    pub inner: Arc<InMemoryAuditStore>,
    pub down: AtomicBool,
    /// Appends of records of this kind are refused, and nothing else is.
    pub(super) refuses_kind: std::sync::Mutex<Option<String>>,
    /// How long an append takes before it answers, in milliseconds.
    pub delay_ms: std::sync::atomic::AtomicU64,
    /// This many more appends are refused, then the store takes them again.
    pub(super) refuses_next: std::sync::Mutex<u64>,
    /// Records the store has taken.
    pub stored: std::sync::atomic::AtomicUsize,
}

impl Flaky {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(InMemoryAuditStore::new()),
            down: AtomicBool::new(false),
            refuses_kind: std::sync::Mutex::new(None),
            delay_ms: std::sync::atomic::AtomicU64::new(0),
            refuses_next: std::sync::Mutex::new(0),
            stored: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    pub fn set_down(&self, down: bool) {
        self.down.store(down, Ordering::SeqCst);
    }

    async fn lag(&self) {
        let wait = self.delay_ms.load(Ordering::SeqCst);
        if wait > 0 {
            tokio::time::sleep(Duration::from_millis(wait)).await;
        }
    }

    fn refuse(&self, kind: &str) -> A2aResult<()> {
        let poisoned = self.refuses_kind.lock().unwrap().as_deref() == Some(kind);
        let flapping = !kind.is_empty() && {
            let mut left = self.refuses_next.lock().unwrap();
            let refuse = *left > 0;
            *left = left.saturating_sub(1);
            refuse
        };
        if poisoned || flapping || self.down.load(Ordering::SeqCst) {
            Err(A2aError::internal("the audit store is down"))
        } else {
            Ok(())
        }
    }
}

impl AuditStore for Flaky {
    fn head<'a>(&'a self, c: &'a str) -> BoxFuture<'a, A2aResult<Option<(u64, String)>>> {
        Box::pin(async move {
            self.lag().await;
            self.refuse("")?;
            self.inner.head(c).await
        })
    }
    fn append<'a>(&'a self, r: &'a AuditRecord) -> BoxFuture<'a, A2aResult<Appended>> {
        Box::pin(async move {
            self.lag().await;
            self.refuse(&r.kind)?;
            let appended = self.inner.append(r).await?;
            self.stored.fetch_add(1, Ordering::SeqCst);
            Ok(appended)
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
}

pub(super) fn interceptor(store: &Arc<Flaky>) -> AuditInterceptor {
    let log = AuditLog::new(Arc::clone(store) as Arc<dyn AuditStore>).require_record(true);
    AuditInterceptor {
        log: Arc::new(log),
        metrics: Arc::new(NoopMetrics),
    }
}

/// The kinds a chain holds, in order.
pub(super) async fn kinds(log: &AuditLog) -> Vec<String> {
    log.export("")
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.kind)
        .collect()
}

/// A call admitted while the store is up, ended while it is down, then the
/// store heals and the next call is admitted: the first call's outcome is in
/// the chain, once, before the second call's start.
#[tokio::test]
async fn an_outcome_the_store_refused_is_written_when_it_heals() {
    let store = Flaky::new();
    let audit = interceptor(&store);

    let ctx = CallContext::new("message/send");
    audit.before(&ctx).await.unwrap();
    store.set_down(true);
    audit.on_complete(&ctx, CallOutcome::Succeeded).await;
    store.set_down(false);

    // The next admission is what a live server does next.
    let next = CallContext::new("tasks/get");
    audit.before(&next).await.unwrap();

    assert_eq!(
        kinds(&audit.log).await,
        [kind::CALL_STARTED, kind::CALL, kind::CALL_STARTED],
        "the first call's outcome must be recorded before the second is admitted",
    );
}

/// The chain's `callId`s for records of `kind`, in order.
pub(super) async fn call_ids(log: &AuditLog, of: &str) -> Vec<String> {
    log.export("")
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.kind == of)
        .filter_map(|r| r.detail.get("callId")?.as_str().map(str::to_owned))
        .collect()
}

/// Waits, in paused time, until `done` holds.
async fn until<F: std::future::Future<Output = bool>>(mut done: impl FnMut() -> F) {
    for _ in 0..2_000 {
        if done().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the condition never held");
}

/// No further call is needed: the log retries what the store refused on its
/// own, so an idle server's last outcomes are not left to the next request.
#[tokio::test(start_paused = true)]
async fn the_spool_drains_without_further_traffic() {
    let store = Flaky::new();
    let audit = interceptor(&store);
    let ctx = CallContext::new("message/send");
    audit.before(&ctx).await.unwrap();
    store.set_down(true);
    audit.on_complete(&ctx, CallOutcome::Failed(&crate::error::ServerError::from(A2aError::internal("x")))).await;
    assert_eq!(audit.log.pending_outcomes(), 1);
    assert_eq!(audit.log.failures(), 1, "the refusal is still counted");
    store.set_down(false);

    until(|| async { audit.log.pending_outcomes() == 0 }).await;
    assert_eq!(kinds(&audit.log).await, [kind::CALL_STARTED, kind::CALL]);
    assert_eq!(
        call_ids(&audit.log, kind::CALL).await,
        call_ids(&audit.log, kind::CALL_STARTED).await,
        "the end names the call the start named",
    );
    assert_eq!(audit.log.outcomes_lost(), 0);
}

/// A call dropped while its outcome is being written leaves the record in the
/// spool, and it is written once.
#[tokio::test(start_paused = true)]
async fn a_dropped_hook_does_not_lose_or_repeat_the_outcome() {
    let store = Flaky::new();
    let audit = interceptor(&store);
    let ctx = CallContext::new("message/send");
    audit.before(&ctx).await.unwrap();
    store.set_down(true);
    // Polled once, then dropped, as a client that hangs up does.
    let _ = tokio::time::timeout(
        Duration::ZERO,
        audit.on_complete(&ctx, CallOutcome::Succeeded),
    )
    .await;
    // The write goes on without its caller, and the store refuses it.
    tokio::task::yield_now().await;
    assert_eq!(audit.log.pending_outcomes(), 1);
    store.set_down(false);

    until(|| async { audit.log.pending_outcomes() == 0 }).await;
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert_eq!(call_ids(&audit.log, kind::CALL).await.len(), 1);
}

/// Records come out of the spool in the order they went in.
#[tokio::test(start_paused = true)]
async fn the_spool_is_written_oldest_first() {
    let store = Flaky::new();
    let audit = interceptor(&store);
    let ctxs: Vec<_> = (0..4).map(|_| CallContext::new("tasks/get")).collect();
    for c in &ctxs {
        audit.before(c).await.unwrap();
    }
    store.set_down(true);
    for c in &ctxs {
        audit.on_complete(c, CallOutcome::Succeeded).await;
    }
    store.set_down(false);
    until(|| async { audit.log.pending_outcomes() == 0 }).await;
    let want: Vec<_> = ctxs.iter().map(|c| c.call_id().to_owned()).collect();
    assert_eq!(call_ids(&audit.log, kind::CALL).await, want);
}

/// A spool that fills counts what it cannot keep.
#[tokio::test(start_paused = true)]
async fn a_full_spool_counts_what_it_cannot_keep() {
    let store = Flaky::new();
    let audit = interceptor(&store);
    store.set_down(true);
    let log = Arc::clone(&audit.log);
    let extra = 5;
    for i in 0..super::super::outcomes::MAX_PENDING + extra {
        assert!(
            log.append_required(rec("", &format!("t{i}"))).await.is_err(),
            "the store is down"
        );
    }
    assert_eq!(log.pending_outcomes(), super::super::outcomes::MAX_PENDING);
    assert_eq!(log.outcomes_lost(), extra as u64);
}

/// A record the store keeps refusing while it answers reads does not hold
/// back the records behind it.
#[tokio::test(start_paused = true)]
async fn a_record_the_store_refuses_alone_is_given_up_on() {
    let store = Flaky::new();
    let audit = interceptor(&store);
    *store.refuses_kind.lock().unwrap() = Some("poison".to_owned());
    let log = Arc::clone(&audit.log);
    let mut bad = rec("", "t0");
    bad.kind = "poison".to_owned();
    assert!(log.append_required(bad).await.is_err());
    // Behind it, a record the store would take.
    store.set_down(true);
    assert!(log.append_required(rec("", "t1")).await.is_err());
    store.set_down(false);

    until(|| async { log.pending_outcomes() == 0 }).await;
    assert_eq!(log.outcomes_lost(), 1, "only the refused record is lost");
    assert_eq!(kinds(&log).await, [kind::CALL]);
}

/// A store that is down is not a record that is bad: however long it stays
/// down, what is spooled waits, and is written when it comes back.
#[tokio::test(start_paused = true)]
async fn a_store_that_stays_down_costs_the_spool_nothing() {
    let store = Flaky::new();
    let audit = interceptor(&store);
    store.set_down(true);
    let log = Arc::clone(&audit.log);
    assert!(log.append_required(rec("", "t0")).await.is_err());
    // Far more retries than a record is given before the store is asked
    // whether it is the one at fault.
    tokio::time::sleep(Duration::from_secs(600)).await;
    assert_eq!(log.pending_outcomes(), 1);
    assert_eq!(log.outcomes_lost(), 0);

    store.set_down(false);
    until(|| async { log.pending_outcomes() == 0 }).await;
    assert_eq!(kinds(&log).await, [kind::CALL]);
    assert_eq!(log.outcomes_lost(), 0);
}

/// A hook dropped while the store is still working on its record, which the
/// store then refuses: the record is spooled by the write that outlived its
/// caller.
#[tokio::test(start_paused = true)]
async fn a_hook_dropped_mid_write_still_has_its_record_kept() {
    let store = Flaky::new();
    let audit = interceptor(&store);
    let ctx = CallContext::new("message/send");
    audit.before(&ctx).await.unwrap();
    store.set_down(true);
    store.delay_ms.store(1_000, Ordering::SeqCst);
    let ended = tokio::time::timeout(
        Duration::from_millis(10),
        audit.on_complete(&ctx, CallOutcome::Succeeded),
    )
    .await;
    assert!(ended.is_err(), "the hook was still writing when dropped");
    assert_eq!(audit.log.pending_outcomes(), 0, "not yet refused");

    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(audit.log.pending_outcomes(), 1, "refused after its caller left");
    store.set_down(false);
    store.delay_ms.store(0, Ordering::SeqCst);
    until(|| async { audit.log.pending_outcomes() == 0 }).await;
    assert_eq!(call_ids(&audit.log, kind::CALL).await, [ctx.call_id()]);
}

/// A store that refuses a record a few times while it answers reads, as a
/// database does on a serialisation failure, has not found the record bad: it
/// is written when the store takes it, and is not counted lost.
#[tokio::test(start_paused = true)]
async fn a_record_refused_a_few_times_is_still_written() {
    let store = Flaky::new();
    let audit = interceptor(&store);
    let log = Arc::clone(&audit.log);
    *store.refuses_next.lock().unwrap() = 4;
    assert!(log.append_required(rec("", "t0")).await.is_err());
    until(|| async { log.pending_outcomes() == 0 }).await;
    assert_eq!(kinds(&log).await, [kind::CALL]);
    assert_eq!(log.outcomes_lost(), 0);
}
