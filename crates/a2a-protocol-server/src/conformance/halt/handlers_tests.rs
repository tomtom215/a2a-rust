// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `halts` and `tenant_scope` graded on states `check_handlers` cannot reach
//! through a store: each check that needs two things must fail when only one
//! holds. The mutation gate found these conjunctions untested: `||` survived
//! in each.

use super::*;
use crate::conformance::Outcome;
use crate::store::InMemoryHaltStore;

fn replicas() -> (RequestHandler, RequestHandler) {
    let store: Arc<dyn HaltStore> = Arc::new(InMemoryHaltStore::new());
    (
        replica(Arc::clone(&store)).expect("builds"),
        replica(store).expect("builds"),
    )
}

fn failed(results: &[CheckResult], name: &str) -> bool {
    results
        .iter()
        .any(|r| r.name == name && r.outcome == Outcome::Fail)
}

/// A task that never ran on this replica fails the start check, however
/// the other replica's went.
#[tokio::test]
async fn a_task_running_on_one_replica_only_fails_the_start_check() {
    let (here, there) = replicas();
    let remote = start(&there, "t1").await.expect("starts");
    let results = halts(&here, &there, "never-started", &remote).await;
    assert!(
        failed(&results, "tasks_start_before_the_halt"),
        "{results:?}"
    );
}

/// A task that ended canceled before the halt was not stopped by it.
#[tokio::test]
async fn a_task_canceled_before_the_halt_was_not_stopped_by_it() {
    let (here, there) = replicas();
    let local = start(&here, "t1").await.expect("starts");
    let remote = start(&there, "t1").await.expect("starts");
    assert!(reaches(&here, "t1", &local, TaskState::Working).await);
    let cancel =
        serde_json::from_value(serde_json::json!({"tenant": "t1", "id": local})).expect("params");
    here.on_cancel_task(cancel, None).await.expect("canceled");
    assert!(reaches(&here, "t1", &local, TaskState::Canceled).await);
    let results = halts(&here, &there, &local, &remote).await;
    assert!(
        failed(&results, "a_halt_stops_this_replicas_tasks"),
        "{results:?}"
    );
}

/// Beta still working on this replica does not spare it when the other
/// refuses its sends: both must hold.
#[tokio::test]
async fn a_tenant_refused_on_the_other_replica_is_not_spared() {
    let here = replica(Arc::new(InMemoryHaltStore::new())).expect("builds");
    let there = replica(Arc::new(InMemoryHaltStore::new())).expect("builds");
    there.halt(HaltScope::All, "test", "everything").await;
    let result = tenant_scope(&here, &there).await;
    assert_eq!(result.outcome, Outcome::Fail, "{result:?}");
    assert!(result.detail.contains("beta spared: false"), "{result:?}");
}
