// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Act 3 — two replicas of the same agent, and what "shared" means.
//!
//! A "replica" here is a [`RequestHandler`] with its own executor, its own
//! event queues, its own port and — the variable under test — either its own
//! store or a shared one. That is the sharing a two-pod deployment has: the
//! database is common, process memory is not.
//!
//! Three checks:
//!
//! 1. **In-memory stores.** A task created on A is not visible on B. Shown,
//!    not assumed: the default store is per-process, and a round-robin
//!    balancer in front of two of them loses every second `GetTask`.
//! 2. **Shared PostgreSQL store.** The same task *is* visible on B, with its
//!    history and artifacts. And the limit of that: B's subscriber to a task
//!    running on A sees the stream *end* (the SDK polls the store for a
//!    terminal state) but not the frames along the way, because event queues
//!    are per-process. Both numbers are printed.
//! 3. **Shared rate-limit counter.** Two limiters configured for 5 requests
//!    per window admit 10 between them by default and 5 with a shared
//!    [`PostgresRateLimitCounter`] — both measured, side by side.
//!
//! Checks 2 and 3 need a server this example cannot start. They report
//! `[NOT RUN]` naming `A2A_TEST_POSTGRES_URL` when it is unset — the same
//! variable, and the same convention, as `incident-response`.
//!
//! [`RequestHandler`]: a2a_protocol_server::RequestHandler
//! [`PostgresRateLimitCounter`]: a2a_protocol_server::PostgresRateLimitCounter

use std::sync::Arc;

use a2a_protocol_client::ClientError;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_types::ErrorCode;
use a2a_protocol_types::params::TaskQueryParams;

use crate::Check;
use crate::support::executors::{FlakyExecutor, FlakyTally};
use crate::support::{client, expect_task, send_params, serve};

pub async fn run() -> Vec<Check> {
    vec![
        in_memory_isolation().await,
        shared_store().await,
        shared_rate_limit().await,
    ]
}

/// The environment variable naming a database to run the shared checks
/// against. Same name `ci.yml`'s `test-postgres` job sets.
pub const POSTGRES_URL_ENV: &str = "A2A_TEST_POSTGRES_URL";

fn query(id: &str) -> TaskQueryParams {
    TaskQueryParams {
        tenant: None,
        id: id.to_owned(),
        history_length: None,
    }
}

// ── 1. In-memory: not shared ────────────────────────────────────────────────

const IN_MEMORY_LABEL: &str = "In-memory stores: a task created on replica A is not on replica B";

async fn in_memory_isolation() -> Check {
    Check::from_result(IN_MEMORY_LABEL, in_memory_isolation_inner().await)
}

async fn in_memory_isolation_inner() -> Result<String, String> {
    let mut urls = Vec::new();
    for _ in 0..2 {
        let handler =
            RequestHandlerBuilder::new(FlakyExecutor(Arc::new(FlakyTally::failing_first(0))))
                .build()
                .map_err(|e| format!("building a replica: {e}"))?;
        let (_handler, url) = serve(handler).await?;
        // The handler `Arc` is dropped here; the acceptor task keeps the
        // server alive. See `support::serve`.
        urls.push(url);
    }
    let a = client(&urls[0])?;
    let b = client(&urls[1])?;

    let task_id = expect_task(
        a.send_message(send_params("created on A"))
            .await
            .map_err(|e| format!("A: SendMessage: {e}"))?,
    )?
    .id
    .0;
    a.get_task(query(&task_id))
        .await
        .map_err(|e| format!("A cannot read back its own task: {e}"))?;

    match b.get_task(query(&task_id)).await {
        Err(ClientError::Protocol(e)) if e.code == ErrorCode::TaskNotFound => Ok(format!(
            "task {task_id}: GetTask on A -> found; GetTask on B -> {:?} — the default store is \
             per-process, so a balancer in front of two replicas needs a shared store",
            e.code
        )),
        Err(e) => Err(format!(
            "B refused for the wrong reason (expected TaskNotFound): {e}"
        )),
        Ok(_) => Err(format!(
            "replica B returned task {task_id} from its own in-memory store — the two replicas \
             are sharing memory, which two processes cannot"
        )),
    }
}

// ── 2 and 3. PostgreSQL: shared ─────────────────────────────────────────────

const STORE_LABEL: &str =
    "PostgreSQL shared store: B reads A's task; B's subscriber sees the end only";
const LIMIT_LABEL: &str = "PostgreSQL shared rate-limit counter: one limit across two replicas";

#[cfg(feature = "postgres")]
async fn shared_store() -> Check {
    let Ok(url) = std::env::var(POSTGRES_URL_ENV) else {
        return Check::unavailable(
            STORE_LABEL,
            format!("set {POSTGRES_URL_ENV} to a PostgreSQL URL to exercise this"),
        );
    };
    Check::from_result(STORE_LABEL, postgres::shared_store(&url).await)
}

#[cfg(feature = "postgres")]
async fn shared_rate_limit() -> Check {
    let Ok(url) = std::env::var(POSTGRES_URL_ENV) else {
        return Check::unavailable(
            LIMIT_LABEL,
            format!("set {POSTGRES_URL_ENV} to a PostgreSQL URL to exercise this"),
        );
    };
    Check::from_result(LIMIT_LABEL, postgres::shared_rate_limit(&url).await)
}

#[cfg(not(feature = "postgres"))]
async fn shared_store() -> Check {
    Check::skipped(STORE_LABEL, "postgres")
}

#[cfg(not(feature = "postgres"))]
async fn shared_rate_limit() -> Check {
    Check::skipped(LIMIT_LABEL, "postgres")
}

#[cfg(feature = "postgres")]
mod postgres {
    use std::sync::Arc;
    use std::time::Duration;

    use a2a_protocol_client::A2aClient;
    use a2a_protocol_server::builder::RequestHandlerBuilder;
    use a2a_protocol_server::rate_limit::{RateLimitConfig, RateLimitInterceptor};
    use a2a_protocol_server::store::PostgresTaskStore;
    use a2a_protocol_server::{PostgresRateLimitCounter, RequestHandler};
    use a2a_protocol_types::events::StreamResponse;
    use a2a_protocol_types::task::TaskState;

    use super::query;
    use crate::support::executors::{FlakyExecutor, FlakyTally, StreamingExecutor};
    use crate::support::{FixedIdentity, client, is_refusal, send_params, serve};

    /// Delay between frames on replica A — the window in which B can
    /// subscribe to a task that exists, is non-terminal, and is producing
    /// events somewhere else.
    const STEP: Duration = Duration::from_millis(300);

    async fn replica(url: &str) -> Result<RequestHandler, String> {
        let store = PostgresTaskStore::with_migrations(url)
            .await
            .map_err(|e| format!("connecting to {url}: {e}"))?;
        RequestHandlerBuilder::new(StreamingExecutor {
            step: STEP,
            die_after_first_chunk: false,
        })
        .with_task_store(store)
        .build()
        .map_err(|e| format!("building a replica: {e}"))
    }

    fn task_id_of(event: &StreamResponse) -> Option<String> {
        match event {
            StreamResponse::Task(t) => Some(t.id.0.clone()),
            StreamResponse::StatusUpdate(e) => Some(e.task_id.0.clone()),
            StreamResponse::ArtifactUpdate(e) => Some(e.task_id.0.clone()),
            _ => None,
        }
    }

    pub(super) async fn shared_store(url: &str) -> Result<String, String> {
        let (_a, url_a) = serve(replica(url).await?).await?;
        let (_b, url_b) = serve(replica(url).await?).await?;
        let a = client(&url_a)?;
        let b = client(&url_b)?;

        // Start a task streaming on A and learn its id from the first frame.
        let mut stream_a = a
            .stream_message(send_params("created on A"))
            .await
            .map_err(|e| format!("A: SendStreamingMessage: {e}"))?;
        let first = stream_a
            .next()
            .await
            .ok_or("A: the stream produced nothing")?
            .map_err(|e| format!("A: first frame: {e}"))?;
        let task_id = task_id_of(&first).ok_or("A: the first frame named no task")?;

        // Subscribe on B to a task whose executor is running on A.
        let mut stream_b = b
            .subscribe_to_task(task_id.clone())
            .await
            .map_err(|e| format!("B: SubscribeToTask {task_id}: {e}"))?;

        // Drain A so the task actually completes, counting artifact frames.
        let drain_a = tokio::spawn(async move {
            let mut artifacts = 0_usize;
            while let Some(Ok(event)) = stream_a.next().await {
                if matches!(event, StreamResponse::ArtifactUpdate(_)) {
                    artifacts += 1;
                }
            }
            artifacts
        });

        let mut b_artifacts = 0_usize;
        let mut b_last_state = None;
        let ended = tokio::time::timeout(Duration::from_secs(20), async {
            while let Some(Ok(event)) = stream_b.next().await {
                match &event {
                    StreamResponse::ArtifactUpdate(_) => b_artifacts += 1,
                    StreamResponse::StatusUpdate(e) if e.status.state.is_terminal() => {
                        b_last_state = Some(e.status.state);
                    }
                    StreamResponse::Task(t) if t.status.state.is_terminal() => {
                        b_last_state = Some(t.status.state);
                    }
                    _ => {}
                }
            }
        })
        .await;
        let a_artifacts = drain_a
            .await
            .map_err(|e| format!("A's drain panicked: {e}"))?;
        if ended.is_err() {
            return Err(format!(
                "B's subscription to task {task_id} never ended — a task completing on A must \
                 still terminate a subscription on B"
            ));
        }
        if b_last_state != Some(TaskState::Completed) {
            return Err(format!(
                "B's subscription ended without reporting Completed (last terminal state: {b_last_state:?})"
            ));
        }

        // The store is shared: B reads what A wrote, in full.
        let on_a = a
            .get_task(query(&task_id))
            .await
            .map_err(|e| format!("A: GetTask: {e}"))?;
        let on_b = b.get_task(query(&task_id)).await.map_err(|e| {
            format!("B cannot read task {task_id} that A created in the shared store: {e}")
        })?;
        let a_json = serde_json::to_value(&on_a).map_err(|e| e.to_string())?;
        let b_json = serde_json::to_value(&on_b).map_err(|e| e.to_string())?;
        if a_json != b_json {
            return Err(format!(
                "A and B read different tasks from the same store:\n  A: {a_json}\n  B: {b_json}"
            ));
        }
        let parts: usize = on_b.artifacts.iter().flatten().map(|a| a.parts.len()).sum();

        Ok(format!(
            "task {task_id}: GetTask on B -> {:?}, {parts} artifact part(s), identical to A's; \
             A's stream carried {a_artifacts} artifact frame(s), B's subscription saw {b_artifacts} \
             and ended with Completed — the store is shared, the event queues are not",
            on_b.status.state
        ))
    }

    /// Configured limit per caller per window.
    const LIMIT: u64 = 5;
    /// Requests sent across both replicas, alternating.
    const SENDS: u64 = LIMIT * 2 + 2;

    /// A replica with a limiter keyed by `caller`, sharing `counter` if given.
    async fn limited_replica(
        caller: &str,
        counter: Option<Arc<PostgresRateLimitCounter>>,
    ) -> Result<String, String> {
        let config = RateLimitConfig::default()
            .with_requests_per_window(LIMIT)
            // Wide enough that the window cannot roll mid-check.
            .with_window_secs(300);
        let mut limiter = RateLimitInterceptor::new(config).map_err(|e| format!("limiter: {e}"))?;
        if let Some(counter) = counter {
            limiter = limiter.with_shared_counter(counter);
        }
        let handler =
            RequestHandlerBuilder::new(FlakyExecutor(Arc::new(FlakyTally::failing_first(0))))
                // Identity first, so the limiter sees it.
                .with_interceptor(FixedIdentity(caller.to_owned()))
                .with_interceptor(limiter)
                .build()
                .map_err(|e| format!("building a limited replica: {e}"))?;
        let (_handler, url) = serve(handler).await?;
        Ok(url)
    }

    /// Sends `SENDS` requests alternating between the two replicas; returns
    /// `(admitted by A, admitted by B)`.
    async fn admit_alternating(a: &A2aClient, b: &A2aClient) -> Result<(u64, u64), String> {
        let mut admitted = (0, 0);
        for n in 0..SENDS {
            let (client, slot) = if n % 2 == 0 {
                (a, &mut admitted.0)
            } else {
                (b, &mut admitted.1)
            };
            match client.send_message(send_params("count me")).await {
                Ok(_) => *slot += 1,
                Err(e) if is_refusal(&e) => {}
                Err(e) => return Err(format!("send {} never reached a replica: {e}", n + 1)),
            }
        }
        Ok(admitted)
    }

    pub(super) async fn shared_rate_limit(url: &str) -> Result<String, String> {
        // A per-run caller, because the counter table outlives the process
        // and a 300-second window would otherwise remember the last run.
        let run = uuid::Uuid::new_v4().to_string();

        // Default: each replica counts in its own map.
        let local_a = client(&limited_replica(&format!("local-{run}"), None).await?)?;
        let local_b = client(&limited_replica(&format!("local-{run}"), None).await?)?;
        let (la, lb) = admit_alternating(&local_a, &local_b).await?;
        if la + lb != LIMIT * 2 {
            return Err(format!(
                "two independent limiters at {LIMIT}/window should admit {} of {SENDS} between \
                 them (the per-replica multiplication the shared counter removes); admitted {} \
                 (A {la}, B {lb})",
                LIMIT * 2,
                la + lb
            ));
        }

        // Shared: each replica has its own counter *instance* — its own pool,
        // as a separate process would — over the same table.
        let counter = |name: &'static str| async move {
            PostgresRateLimitCounter::new(url)
                .await
                .map(Arc::new)
                .map_err(|e| format!("{name}: rate-limit counter on {url}: {e}"))
        };
        let shared_caller = format!("shared-{run}");
        let shared_a = client(&limited_replica(&shared_caller, Some(counter("A").await?)).await?)?;
        let shared_b = client(&limited_replica(&shared_caller, Some(counter("B").await?)).await?)?;
        let (sa, sb) = admit_alternating(&shared_a, &shared_b).await?;
        if sa + sb != LIMIT {
            return Err(format!(
                "two limiters sharing PostgresRateLimitCounter at {LIMIT}/window should admit \
                 {LIMIT} of {SENDS} between them; admitted {} (A {sa}, B {sb})",
                sa + sb
            ));
        }

        Ok(format!(
            "limit {LIMIT}/300s per caller, {SENDS} sends alternating A/B: independent limiters \
             admitted {} (A {la}, B {lb}); limiters sharing PostgresRateLimitCounter admitted {} \
             (A {sa}, B {sb})",
            la + lb,
            sa + sb
        ))
    }
}
