// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The kill switch end to end: handlers built on the store under grade,
//! standing for replicas.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use a2a_protocol_types::message::Message;
use a2a_protocol_types::params::{MessageSendParams, TaskQueryParams};
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::TaskState;

use super::{BOUND, Shared};
use crate::builder::RequestHandlerBuilder;
use crate::conformance::{CheckResult, Report};
use crate::error::ServerError;
use crate::handler::HaltScope;
use crate::store::{HaltStore, TenantAwareInMemoryTaskStore};
use crate::{EventEmitter, RequestHandler, SendMessageResult, agent_executor};

/// How often the harness's handlers read the store.
const POLL: Duration = Duration::from_millis(50);

/// Works until cancelled.
struct Worker;
agent_executor!(Worker, |ctx, queue| async {
    EventEmitter::new(ctx, queue)
        .status(TaskState::Working)
        .await?;
    ctx.cancellation_token.cancelled().await;
    Ok(())
});

fn replica(store: Arc<dyn HaltStore>) -> Result<RequestHandler, String> {
    RequestHandlerBuilder::new(Worker)
        .without_agent_card()
        .with_task_store(TenantAwareInMemoryTaskStore::new())
        .with_halt_store(Shared(store))
        .with_halt_poll_interval(POLL)
        .build()
        .map_err(|e| format!("a handler on the store did not build: {e}"))
}

/// A report of one failure, for a run that could not get started.
fn stopped_at(name: &'static str, why: impl Into<String>) -> Report {
    Report {
        subject: "kill switch",
        results: vec![CheckResult::fail(name, why)],
    }
}

fn params(tenant: &str) -> MessageSendParams {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = format!("halt-conformance-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let mut p = MessageSendParams::new(Message::user_text(id, "work"));
    p.tenant = Some(tenant.to_owned());
    p.configuration = serde_json::from_value(serde_json::json!({ "returnImmediately": true })).ok();
    p
}

/// Starts a task in `tenant`; its id, or why it was not started.
async fn start(h: &RequestHandler, tenant: &str) -> Result<String, String> {
    match h.on_send_message(params(tenant), false, None).await {
        Ok(SendMessageResult::Response(SendMessageResponse::Task(t))) => Ok(t.id.0),
        Ok(_) => Err("the send answered without a task".to_owned()),
        Err(e) => Err(e.to_string()),
    }
}

/// Whether a send to `tenant` is refused as halted.
async fn refused(h: &RequestHandler, tenant: &str) -> bool {
    matches!(
        h.on_send_message(params(tenant), false, None).await,
        Err(ServerError::Halted(_))
    )
}

async fn state(h: &RequestHandler, tenant: &str, id: &str) -> Option<TaskState> {
    let q = TaskQueryParams {
        tenant: Some(tenant.to_owned()),
        id: id.to_owned(),
        history_length: None,
    };
    h.on_get_task(q, None).await.ok().map(|t| t.status.state)
}

/// Waits up to [`BOUND`] for `done` to hold.
async fn within<F, Fut>(done: F) -> bool
where
    F: Fn() -> Fut + Send + Sync,
    Fut: Future<Output = bool> + Send,
{
    tokio::time::timeout(BOUND, async {
        while !done().await {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok()
}

async fn reaches(h: &RequestHandler, tenant: &str, id: &str, want: TaskState) -> bool {
    within(|| async { state(h, tenant, id).await == Some(want) }).await
}

fn grade(name: &'static str, ok: bool, pass: &str, fail: impl Into<String>) -> CheckResult {
    if ok {
        CheckResult::pass(name, pass)
    } else {
        CheckResult::fail(name, fail)
    }
}

/// Grades the kill switch on handlers built on the store `open` opens: each
/// call must open the same backing store, starting empty, as
/// [`check_store`](super::check_store) requires. The store is left empty.
///
/// The checks run in order, each on the state the last left: a halt that
/// never reaches the other replica also fails the checks after it that need
/// it to have.
pub async fn check_handlers<F, Fut>(open: &F) -> Report
where
    F: Fn() -> Fut + Sync,
    Fut: Future<Output = Arc<dyn HaltStore>> + Send,
{
    let (here, there) = match (replica(open().await), replica(open().await)) {
        (Ok(here), Ok(there)) => (here, there),
        (Err(e), _) | (_, Err(e)) => return stopped_at("replicas_build", e),
    };
    let (Ok(local), Ok(remote)) = (start(&here, "t1").await, start(&there, "t1").await) else {
        return stopped_at(
            "tasks_start_before_the_halt",
            "the harness could not start a task on a fresh replica",
        );
    };
    let mut results = halts(&here, &there, &local, &remote).await;
    let restarted = match replica(open().await) {
        Ok(h) => h,
        Err(e) => {
            results.push(CheckResult::fail(
                "a_restarted_replica_comes_back_halted",
                e,
            ));
            return Report {
                subject: "kill switch",
                results,
            };
        }
    };
    results.extend(restart_and_resume(&here, &there, &restarted).await);
    results.push(tenant_scope(&here, &there).await);
    Report {
        subject: "kill switch",
        results,
    }
}

/// Halts everything on `here` while `local` runs there and `remote` on
/// `there`, and grades what each replica does.
async fn halts(
    here: &RequestHandler,
    there: &RequestHandler,
    local: &str,
    remote: &str,
) -> Vec<CheckResult> {
    let ran = reaches(here, "t1", local, TaskState::Working).await
        && reaches(there, "t1", remote, TaskState::Working).await;
    let mut results = vec![grade(
        "tasks_start_before_the_halt",
        ran,
        "a task runs on each replica",
        "a task never reached working",
    )];
    let report = here
        .halt(HaltScope::All, "conformance", "kill switch")
        .await;
    results.push(grade(
        "a_halt_is_written_to_the_store",
        report.persisted == Some(true),
        "the halt was stored",
        format!("persisted = {:?}", report.persisted),
    ));
    results.push(grade(
        "a_halt_stops_this_replicas_tasks",
        report.stopped.iter().any(|t| t == local)
            && reaches(here, "t1", local, TaskState::Canceled).await,
        "the running task was stopped and ended canceled",
        format!(
            "stopped {:?}; the task did not end canceled",
            report.stopped
        ),
    ));
    results.push(grade(
        "a_halt_refuses_new_work_here",
        refused(here, "t1").await,
        "a new send was refused as halted",
        "a new send was admitted",
    ));
    results.push(grade(
        "a_halt_stops_the_other_replicas_tasks",
        reaches(there, "t1", remote, TaskState::Canceled).await,
        "the other replica's task ended canceled",
        "the other replica's task was still not canceled after 10 s",
    ));
    results.push(grade(
        "a_halt_refuses_new_work_on_the_other_replica",
        within(|| refused(there, "t1")).await,
        "the other replica refused a new send",
        "the other replica still admitted sends after 10 s",
    ));
    results
}

/// A replica built after the halt starts halted; a resume on another lifts
/// it on every replica.
async fn restart_and_resume(
    here: &RequestHandler,
    there: &RequestHandler,
    restarted: &RequestHandler,
) -> Vec<CheckResult> {
    let came_back = grade(
        "a_restarted_replica_comes_back_halted",
        refused(restarted, "t1").await,
        "the first send to a new replica was refused",
        "a replica built after the halt admitted its first send",
    );
    let resumed = there.resume(HaltScope::All, "conformance").await;
    let lifted = matches!(resumed, Ok(true))
        && within(|| async { start(here, "t1").await.is_ok() }).await
        && within(|| async { start(restarted, "t1").await.is_ok() }).await;
    let resume = grade(
        "a_resume_on_any_replica_lifts_the_halt_everywhere",
        lifted,
        "after a resume on one replica, the others admitted sends",
        format!("resume answered {resumed:?}, or a replica still refused after 10 s"),
    );
    vec![came_back, resume]
}

/// A tenant's halt stops that tenant's work and only that.
async fn tenant_scope(here: &RequestHandler, there: &RequestHandler) -> CheckResult {
    const NAME: &str = "a_tenant_halt_spares_other_tenants";
    let (Ok(acme), Ok(beta)) = (start(here, "acme").await, start(here, "beta").await) else {
        return CheckResult::fail(NAME, "tasks could not be started after the resume");
    };
    let scope = HaltScope::Tenant("acme".into());
    here.halt(scope.clone(), "conformance", "one tenant").await;
    let stopped = reaches(here, "acme", &acme, TaskState::Canceled).await;
    // Once the other replica refuses acme it has read the halt, so beta is
    // judged against what the store holds, not against a replica that has
    // not looked yet: a store that widened the halt to every tenant passed
    // when beta was judged first.
    let refused_there = within(|| refused(there, "acme")).await;
    let spared = state(here, "beta", &beta).await == Some(TaskState::Working)
        && start(there, "beta").await.is_ok();
    let _ = here.resume(scope, "conformance").await;
    grade(
        NAME,
        stopped && spared && refused_there,
        "acme stopped and refused on both replicas; beta kept working and admitted sends",
        format!(
            "acme stopped: {stopped}; beta spared: {spared}; acme refused on the other replica: {refused_there}"
        ),
    )
}

#[cfg(test)]
#[path = "handlers_tests.rs"]
mod tests;
