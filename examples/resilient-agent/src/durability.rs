// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Act 1 — a task outlives the handler that created it.
//!
//! "Restart" here is the real thing as far as the store is concerned: a
//! second [`RequestHandler`] with its own `SqliteTaskStore`, its own
//! `SqlitePushConfigStore` and its own port is opened over the **same
//! database file**, and every `Arc` to the first handler is dropped. Nothing
//! in process memory is shared between the two. The in-memory store would
//! pass any check that reused one handler, so the second handler is the whole
//! point.
//!
//! Two checks, because a restart has two cases and they have different
//! answers:
//!
//! 1. The task had **finished** before the restart. Everything — status,
//!    history, artifacts, push config — must come back identical.
//! 2. The task was **mid-stream** when the process died. What had been
//!    persisted comes back; what had not, does not; and the SDK does **not**
//!    resume the executor on the new handler. The task stays `Working`. That
//!    is stated here rather than hidden, because it is the first thing an
//!    operator needs to know about restarting an agent with tasks in flight.
//!
//! [`RequestHandler`]: a2a_protocol_server::RequestHandler

use crate::Check;

const COMPLETED_LABEL: &str = "SQLite: a completed task survives a handler restart";
const INTERRUPTED_LABEL: &str = "SQLite: a task cut off mid-stream is persisted as far as it got";

#[cfg(feature = "sqlite")]
pub async fn run() -> Vec<Check> {
    vec![completed_task().await, interrupted_task().await]
}

#[cfg(not(feature = "sqlite"))]
pub async fn run() -> Vec<Check> {
    vec![
        Check::skipped(COMPLETED_LABEL, "sqlite"),
        Check::skipped(INTERRUPTED_LABEL, "sqlite"),
    ]
}

#[cfg(feature = "sqlite")]
mod sqlite {
    use std::sync::Arc;
    use std::time::Duration;

    use a2a_protocol_client::A2aClient;
    use a2a_protocol_server::builder::RequestHandlerBuilder;
    use a2a_protocol_server::push::{HttpPushSender, PushRetryPolicy};
    use a2a_protocol_server::store::SqliteTaskStore;
    use a2a_protocol_server::{RequestHandler, SqlitePushConfigStore};
    use a2a_protocol_types::events::StreamResponse;
    use a2a_protocol_types::params::{ListPushConfigsParams, TaskQueryParams};
    use a2a_protocol_types::task::{Task, TaskState};
    use hyper::StatusCode;

    use super::{COMPLETED_LABEL, INTERRUPTED_LABEL};
    use crate::Check;
    use crate::support::executors::{CHUNK_ONE, StreamingExecutor};
    use crate::support::injectors::webhook_sink;
    use crate::support::metrics::RecordingMetrics;
    use crate::support::{client, scratch_dir, send_params_with_push, serve, text_of};

    /// Delay between the executor's frames. Long enough that "cut off after
    /// the first chunk" is a state a client can observe, short enough that
    /// the act stays quick.
    const STEP: Duration = Duration::from_millis(50);

    /// Opens a handler over `db_url`: task store, push-config store, a push
    /// sender so push configs are accepted, and a recorder for the one
    /// signal that says a write was lost — `Metrics::on_persistence_error`.
    ///
    /// Each call opens fresh pools. Two handlers from two calls share
    /// nothing but the file — which is what two processes would share.
    async fn open(
        db_url: &str,
        executor: StreamingExecutor,
    ) -> Result<(RequestHandler, Arc<RecordingMetrics>), String> {
        let metrics = Arc::new(RecordingMetrics::default());
        let tasks = SqliteTaskStore::with_migrations(db_url)
            .await
            .map_err(|e| format!("opening the task store at {db_url}: {e}"))?;
        let push_configs = SqlitePushConfigStore::new(db_url)
            .await
            .map_err(|e| format!("opening the push-config store at {db_url}: {e}"))?;
        let handler = RequestHandlerBuilder::new(executor)
            .with_task_store(tasks)
            .with_push_config_store(push_configs)
            .with_push_sender(
                HttpPushSender::with_timeout(Duration::from_secs(1))
                    .with_retry_policy(PushRetryPolicy::default().with_max_attempts(1))
                    .allow_private_urls(),
            )
            .with_metrics(Arc::clone(&metrics))
            .build()
            .map_err(|e| format!("building the handler: {e}"))?;
        Ok((handler, metrics))
    }

    /// The streaming path persists each event in the background, so a
    /// client can see an artifact the store rejected. This is the check that
    /// it did not: a restart can only bring back what was actually written.
    fn require_no_lost_writes(name: &str, metrics: &RecordingMetrics) -> Result<(), String> {
        let errors = metrics.persistence_errors();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "{name} reported {} persistence error(s) — writes the client never saw fail: {errors:?}",
                errors.len()
            ))
        }
    }

    async fn get(client: &A2aClient, id: &str) -> Result<Task, String> {
        client
            .get_task(TaskQueryParams {
                tenant: None,
                id: id.to_owned(),
                history_length: None,
            })
            .await
            .map_err(|e| format!("GetTask {id}: {e}"))
    }

    fn task_id_of(event: &StreamResponse) -> Option<String> {
        match event {
            StreamResponse::Task(t) => Some(t.id.0.clone()),
            StreamResponse::StatusUpdate(e) => Some(e.task_id.0.clone()),
            StreamResponse::ArtifactUpdate(e) => Some(e.task_id.0.clone()),
            _ => None,
        }
    }

    fn describe(task: &Task) -> String {
        let history = task.history.as_ref().map_or(0, Vec::len);
        let artifacts = task.artifacts.as_ref().map_or(0, Vec::len);
        let parts: usize = task.artifacts.iter().flatten().map(|a| a.parts.len()).sum();
        format!(
            "{:?}, {history} history message(s), {artifacts} artifact(s) with {parts} part(s)",
            task.status.state
        )
    }

    // ── Check 1: finished before the restart ────────────────────────────────

    pub(super) async fn completed_task() -> Check {
        let dir = match scratch_dir("durable") {
            Ok(dir) => dir,
            Err(e) => return Check::fail(COMPLETED_LABEL, e),
        };
        let db_url = format!("sqlite://{}/agent.db?mode=rwc", dir.display());
        let outcome = completed_round_trip(&db_url).await;
        let _ = std::fs::remove_dir_all(&dir);
        Check::from_result(COMPLETED_LABEL, outcome)
    }

    async fn completed_round_trip(db_url: &str) -> Result<String, String> {
        let (webhook, sink) = webhook_sink(0, StatusCode::SERVICE_UNAVAILABLE).await;

        // Handler A: create the task, stream it to completion.
        let (handler_a, metrics_a) = open(
            db_url,
            StreamingExecutor {
                step: STEP,
                die_after_first_chunk: false,
            },
        )
        .await?;
        let (handler_a, url_a) = serve(handler_a).await?;
        let client_a = client(&url_a)?;

        let mut stream = client_a
            .stream_message(send_params_with_push("make me durable", &webhook))
            .await
            .map_err(|e| format!("A: SendStreamingMessage: {e}"))?;
        let mut task_id = None;
        let mut frames = 0_usize;
        while let Some(event) = stream.next().await {
            let event = event.map_err(|e| format!("A: stream error: {e}"))?;
            frames += 1;
            if task_id.is_none() {
                task_id = task_id_of(&event);
            }
        }
        let task_id = task_id.ok_or("A: the stream named no task")?;

        // The stream ending is not the store being written. The streaming
        // reader and the persister are separate subscribers to the event
        // queue, so the client can see `Completed` a few milliseconds before
        // `GetTask` does. Measured rather than assumed: the first version of
        // this check read the task straight after the stream and found it
        // `Working`.
        let started = std::time::Instant::now();
        let before = loop {
            let task = get(&client_a, &task_id).await?;
            if task.status.state == TaskState::Completed {
                break task;
            }
            if started.elapsed() > Duration::from_secs(3) {
                return Err(format!(
                    "A: the stream ended but the store still says {:?} after 3s",
                    task.status.state
                ));
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        let store_lag = started.elapsed();

        require_no_lost_writes("A", &metrics_a)?;

        // "Restart": drop everything this example holds on handler A. Its
        // acceptor task keeps its own clone, so the port is simply never
        // used again — see `support::serve`.
        drop(client_a);
        drop(handler_a);

        // Handler B: the same file, nothing else.
        let (handler_b, _) = open(
            db_url,
            StreamingExecutor {
                step: STEP,
                die_after_first_chunk: false,
            },
        )
        .await?;
        let (_handler_b, url_b) = serve(handler_b).await?;
        let client_b = client(&url_b)?;
        let after = get(&client_b, &task_id)
            .await
            .map_err(|e| format!("task {task_id} did not survive the restart: {e}"))?;

        // Whole-value equality, not a field or two: any column the store
        // drops or rewrites shows up here as a diff, named.
        let before_json = serde_json::to_value(&before).map_err(|e| e.to_string())?;
        let after_json = serde_json::to_value(&after).map_err(|e| e.to_string())?;
        if before_json != after_json {
            return Err(format!(
                "task {task_id} came back different after the restart:\n  before: {before_json}\n  after:  {after_json}"
            ));
        }
        let text: String = after
            .artifacts
            .iter()
            .flatten()
            .map(|a| text_of(&a.parts))
            .collect();
        if !text.contains(CHUNK_ONE) {
            return Err(format!(
                "task {task_id} lost its artifact text across the restart: {text:?}"
            ));
        }

        let configs = client_b
            .list_push_configs(ListPushConfigsParams {
                tenant: None,
                task_id: task_id.clone(),
                page_size: None,
                page_token: None,
            })
            .await
            .map_err(|e| format!("B: ListTaskPushNotificationConfigs: {e}"))?;
        let urls: Vec<&str> = configs.configs.iter().map(|c| c.url.as_str()).collect();
        if urls != [webhook.as_str()] {
            return Err(format!(
                "the push config did not survive the restart: expected [{webhook}], B lists {urls:?}"
            ));
        }

        Ok(format!(
            "task {task_id}: {} — {frames} frames streamed, {} push delivery(ies) received, \
             0 persistence errors, store Completed {store_lag:?} after the stream ended; read back \
             byte-identical by a fresh handler, push config intact",
            describe(&after),
            sink.accepted()
        ))
    }

    // ── Check 2: cut off mid-stream ─────────────────────────────────────────

    pub(super) async fn interrupted_task() -> Check {
        let dir = match scratch_dir("interrupted") {
            Ok(dir) => dir,
            Err(e) => return Check::fail(INTERRUPTED_LABEL, e),
        };
        let db_url = format!("sqlite://{}/agent.db?mode=rwc", dir.display());
        let outcome = interrupted_round_trip(&db_url).await;
        let _ = std::fs::remove_dir_all(&dir);
        Check::from_result(INTERRUPTED_LABEL, outcome)
    }

    async fn interrupted_round_trip(db_url: &str) -> Result<String, String> {
        let (webhook, _sink) = webhook_sink(0, StatusCode::SERVICE_UNAVAILABLE).await;

        // Handler A's executor emits the first chunk and then dies.
        let (handler_a, metrics_a) = open(
            db_url,
            StreamingExecutor {
                step: STEP,
                die_after_first_chunk: true,
            },
        )
        .await?;
        let (handler_a, url_a) = serve(handler_a).await?;
        let client_a = client(&url_a)?;

        let mut stream = client_a
            .stream_message(send_params_with_push("cut me off", &webhook))
            .await
            .map_err(|e| format!("A: SendStreamingMessage: {e}"))?;
        let mut task_id = None;
        let mut frames_seen = 0_usize;
        // Read until the first artifact chunk has arrived — "streamed
        // part-way" — then stop, as a client whose server just died would.
        let read = tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(event) = stream.next().await {
                let event = event.map_err(|e| format!("A: stream error: {e}"))?;
                frames_seen += 1;
                if task_id.is_none() {
                    task_id = task_id_of(&event);
                }
                if matches!(event, StreamResponse::ArtifactUpdate(_)) {
                    return Ok::<_, String>(());
                }
            }
            Err("A: the stream ended before the first chunk".to_owned())
        })
        .await;
        match read {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err("A: no artifact chunk within 5s".to_owned()),
        }
        let task_id = task_id.ok_or("A: the stream named no task")?;

        // As in check 1: the stream reader and the persister are separate
        // subscribers, so the chunk the client just saw may not be in the
        // store yet. The crash this scene stages happens after that write
        // lands — the executor is parked, so nothing else will follow it.
        // Measured, not assumed: on a macOS runner the first version of this
        // check dropped handler A straight away and handler B read back an
        // empty artifact.
        let started = std::time::Instant::now();
        loop {
            let task = get(&client_a, &task_id).await?;
            let text: String = task
                .artifacts
                .iter()
                .flatten()
                .map(|a| text_of(&a.parts))
                .collect();
            if text == CHUNK_ONE {
                break;
            }
            if started.elapsed() > Duration::from_secs(3) {
                return Err(format!(
                    "A: the client saw the first chunk but the store holds {text:?} after 3s"
                ));
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        require_no_lost_writes("A", &metrics_a)?;
        drop(stream);
        drop(client_a);
        drop(handler_a);

        // Handler B over the same file.
        let (handler_b, _) = open(
            db_url,
            StreamingExecutor {
                step: STEP,
                die_after_first_chunk: false,
            },
        )
        .await?;
        let (_handler_b, url_b) = serve(handler_b).await?;
        let client_b = client(&url_b)?;
        let after = get(&client_b, &task_id)
            .await
            .map_err(|e| format!("task {task_id} did not survive the restart: {e}"))?;

        let text: String = after
            .artifacts
            .iter()
            .flatten()
            .map(|a| text_of(&a.parts))
            .collect();
        if text != CHUNK_ONE {
            return Err(format!(
                "expected exactly the first chunk {CHUNK_ONE:?} to have been persisted, found {text:?}"
            ));
        }
        if after.status.state != TaskState::Working {
            return Err(format!(
                "a task cut off mid-stream should read back Working, got {:?}",
                after.status.state
            ));
        }

        // Give a hypothetical resume mechanism far longer than the executor
        // needs to finish, then look again. If this ever comes back
        // Completed, the SDK has started resuming executors and this act's
        // wording is wrong.
        let wait = STEP * 10;
        tokio::time::sleep(wait).await;
        let later = get(&client_b, &task_id).await?;
        if later.status.state != TaskState::Working {
            return Err(format!(
                "the task changed state to {:?} on the new handler with no one driving it — \
                 something resumed the executor, and the README says nothing does",
                later.status.state
            ));
        }

        Ok(format!(
            "task {task_id}: {frames_seen} frames seen before the cut; the new handler reads \
             {} — chunk one persisted, chunk two never written; still Working after {wait:?}: \
             the SDK does NOT resume an executor after a restart",
            describe(&later)
        ))
    }
}

#[cfg(feature = "sqlite")]
use sqlite::{completed_task, interrupted_task};
