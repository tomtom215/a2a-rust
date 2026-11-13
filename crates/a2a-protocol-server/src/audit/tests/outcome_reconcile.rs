// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A start with no end is found and closed (adversarial review A3,
//! 2026-11-12).
//!
//! The spool is memory. A process that dies with outcomes in it leaves
//! `call.started` records whose calls have no `call` record, and nothing in
//! the chain says so. Reconciliation writes `call.unresolved` for each, once.

use super::outcome_durability::{Flaky, call_ids, interceptor, kinds};
use super::*;
use crate::call_context::CallContext;
use crate::interceptor::{CallOutcome, ServerInterceptor};

/// A process that admitted a call and died before it could record the end.
async fn after_a_crash() -> (Arc<Flaky>, CallContext, AuditLog) {
    let store = Flaky::new();
    let before_the_crash = interceptor(&store);
    let ctx = CallContext::new("message/send");
    before_the_crash.before(&ctx).await.unwrap();
    store.set_down(true);
    before_the_crash
        .on_complete(&ctx, CallOutcome::Succeeded)
        .await;
    // The spool dies with the process: a new log over the same store.
    store.set_down(false);
    let restarted = AuditLog::new(Arc::clone(&store) as Arc<dyn AuditStore>)
        .with_signer(signer(), 0)
        .require_record(true);
    (store, ctx, restarted)
}

#[tokio::test(start_paused = true)]
async fn a_call_the_crash_left_unended_is_found_and_closed_once() {
    let (_store, ctx, log) = after_a_crash().await;

    let found = log.unresolved_calls("", 0, Duration::ZERO).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].call_id, ctx.call_id());
    assert_eq!(found[0].started_seq, 1);
    assert_eq!(found[0].method.as_deref(), Some("message/send"));

    let written = log
        .reconcile_unresolved("", 0, Duration::ZERO)
        .await
        .unwrap();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].kind, kind::CALL_UNRESOLVED);
    assert_eq!(call_ids(&log, kind::CALL_UNRESOLVED).await, [ctx.call_id()]);
    assert_eq!(written[0].detail["startedSeq"], 1);

    // Closed: neither found nor written again.
    assert_eq!(
        log.unresolved_calls("", 0, Duration::ZERO)
            .await
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        log.reconcile_unresolved("", 0, Duration::ZERO)
            .await
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        kinds(&log).await,
        [kind::CALL_STARTED, kind::CALL_UNRESOLVED]
    );
    assert!(
        log.verify("").await.unwrap().is_intact(),
        "the chain still verifies"
    );
}

/// What this process is still serving, or holds in its spool, is not lost.
#[tokio::test(start_paused = true)]
async fn a_call_still_running_or_still_spooled_is_not_unresolved() {
    let store = Flaky::new();
    let audit = interceptor(&store);
    let running = CallContext::new("tasks/get");
    audit.before(&running).await.unwrap();
    let spooled = CallContext::new("tasks/list");
    audit.before(&spooled).await.unwrap();
    store.set_down(true);
    audit.on_complete(&spooled, CallOutcome::Succeeded).await;
    store.set_down(false);

    let found = audit
        .log
        .unresolved_calls("", 0, Duration::ZERO)
        .await
        .unwrap();
    assert_eq!(found.len(), 0, "found {found:?}");

    // Ended: nothing is owed for it.
    audit.on_complete(&running, CallOutcome::Succeeded).await;
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert_eq!(
        audit
            .log
            .unresolved_calls("", 0, Duration::ZERO)
            .await
            .unwrap()
            .len(),
        0
    );
    assert!(
        audit.log.live_call_ids().is_empty(),
        "an ended call is not remembered as running"
    );
}

/// A start written moments ago may belong to a call another replica is
/// serving; only one older than `older_than` is reported.
#[tokio::test]
async fn a_recent_start_is_left_alone() {
    let (_store, _ctx, log) = after_a_crash().await;
    let grace = Duration::from_hours(1);
    assert_eq!(log.unresolved_calls("", 0, grace).await.unwrap().len(), 0);
    assert_eq!(
        log.reconcile_unresolved("", 0, grace).await.unwrap().len(),
        0
    );
    assert_eq!(
        log.unresolved_calls("", 0, Duration::ZERO)
            .await
            .unwrap()
            .len(),
        1
    );
}

/// Reading starts after `after`: calls before it are not looked at.
#[tokio::test(start_paused = true)]
async fn reconciliation_starts_after_the_position_given() {
    let (_store, _ctx, log) = after_a_crash().await;
    assert_eq!(
        log.unresolved_calls("", 1, Duration::ZERO)
            .await
            .unwrap()
            .len(),
        0
    );
}

/// A `call.started` that names no call id, written before ids existed, is
/// not reported: it cannot be matched to anything.
#[tokio::test]
async fn a_start_with_no_call_id_is_not_reported() {
    let store = Flaky::new();
    let log = AuditLog::new(Arc::clone(&store) as Arc<dyn AuditStore>);
    let mut r = AuditRecord::new("", kind::CALL_STARTED);
    r.method = Some("message/send".to_owned());
    log.append(r).await.unwrap();
    assert_eq!(
        log.unresolved_calls("", 0, Duration::ZERO)
            .await
            .unwrap()
            .len(),
        0
    );
}
