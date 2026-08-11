// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Drives every A2A method over every binding this agent serves, recording
//! each success into the coverage matrix.
//!
//! One function runs the whole method set against one client, so all four
//! bindings are exercised by identical code. That is deliberate: a
//! per-binding copy is how a transport ends up with a quietly reduced set of
//! calls, which is the shape of gap this example previously had (JSON-RPC and
//! REST drove different subsets, and neither drove more than four methods).

use a2a_protocol_client::A2aClient;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::method::Method;
use a2a_protocol_types::params::{
    ListPushConfigsParams, ListTasksParams, MessageSendParams, TaskQueryParams,
};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::responses::SendMessageResponse;

use crate::{Binding, Matrix};

/// How long to keep draining one stream before moving on.
///
/// Not a timeout on the *method* — by the time this expires the call has
/// already succeeded and been recorded. It bounds how long a stream that
/// legitimately stays open (a task parked in `INPUT_REQUIRED` never reaches a
/// terminal event) can hold up the rest of the sweep.
const STREAM_DRAIN_BUDGET: std::time::Duration = std::time::Duration::from_secs(5);

/// Cap on events read from one stream, so a chatty agent cannot stall the run.
const MAX_STREAM_EVENTS: usize = 50;

/// Builds a single-text-part send.
pub fn make_send_params(text: &str) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        message: Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
            role: MessageRole::User,
            parts: vec![Part::text(text)],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    }
}

/// What one binding's sweep found, for the caller to print.
pub struct SweepOutcome {
    /// Human-readable lines describing each call.
    pub lines: Vec<String>,
    /// Calls that failed. Non-empty means the run must not report success.
    pub failures: Vec<String>,
}

/// Drives all eleven methods against `client`, recording into `matrix`.
///
/// `webhook_url` is where push configs point. It must be an address something
/// is listening on, otherwise `CreateTaskPushNotificationConfig` may be
/// accepted while delivery silently fails — and a config that is stored but
/// never delivered is not evidence that push works.
///
/// `slow_prefix` is the message-text marker that makes the agent under test
/// pause mid-task. `SubscribeToTask` needs a task that is still running: the
/// server answers `UnsupportedOperation` for a terminal one, correctly, so
/// without a slow turn the success path is unreachable and only the refusal
/// is ever observed. Each example supplies its own marker because each has its
/// own executor.
pub async fn sweep(
    client: &A2aClient,
    binding: Binding,
    webhook_url: &str,
    slow_prefix: &str,
    matrix: &mut Matrix,
) -> SweepOutcome {
    let mut lines = Vec::new();
    let mut failures = Vec::new();

    macro_rules! ok {
        ($method:expr, $detail:expr) => {{
            matrix.record($method, binding);
            lines.push(format!("  [ok]   {:<34} {}", $method.wire_name(), $detail));
        }};
    }
    macro_rules! bad {
        ($method:expr, $err:expr) => {{
            let msg = format!("{} over {}: {}", $method.wire_name(), binding.label(), $err);
            lines.push(format!("  [FAIL] {:<34} {}", $method.wire_name(), $err));
            failures.push(msg);
        }};
    }

    // ── SendMessage ──────────────────────────────────────────────────────
    let task_id = match client.send_message(make_send_params("hello")).await {
        Ok(SendMessageResponse::Task(task)) => {
            ok!(Method::SendMessage, format!("task {}", task.id));
            Some(task.id.to_string())
        }
        Ok(other) => {
            bad!(
                Method::SendMessage,
                format!("expected a Task, got {other:?}")
            );
            None
        }
        Err(e) => {
            bad!(Method::SendMessage, e);
            None
        }
    };

    // ── SendStreamingMessage ─────────────────────────────────────────────
    // Also the source of the task id used by SubscribeToTask below, since a
    // streamed task is still running when the id is first seen.
    let mut streamed_task_id: Option<String> = None;
    match client.stream_message(make_send_params("stream me")).await {
        Ok(mut stream) => {
            let mut events = 0_usize;
            let mut lagged = 0_usize;
            let mut fatal: Option<String> = None;

            // Bounded. A task that parks in INPUT_REQUIRED is not terminal, so
            // its stream stays open indefinitely and an unbounded drain hangs
            // the whole sweep — which is exactly what happened the first time
            // this ran against an agent that asks clarifying questions. The
            // method under test has demonstrably worked once an event arrives;
            // reading to the end of time proves nothing further.
            let drained = tokio::time::timeout(STREAM_DRAIN_BUDGET, async {
                while let Some(ev) = stream.next().await {
                    match ev {
                        Ok(resp) => {
                            events += 1;
                            if streamed_task_id.is_none() {
                                streamed_task_id = task_id_of(&resp);
                            }
                        }
                        // Recoverable: the server says events were dropped for
                        // this consumer, not that the stream ended.
                        Err(e) if e.is_stream_lagged() => lagged += 1,
                        Err(e) => {
                            fatal = Some(format!("{e}"));
                            return;
                        }
                    }
                    if events >= MAX_STREAM_EVENTS {
                        return;
                    }
                }
            })
            .await;

            match (fatal, events) {
                (Some(e), _) => bad!(Method::SendStreamingMessage, e),
                (None, 0) => bad!(
                    Method::SendStreamingMessage,
                    if drained.is_err() {
                        "stream produced no events before the drain budget expired"
                    } else {
                        "stream produced no events"
                    }
                ),
                (None, n) => ok!(
                    Method::SendStreamingMessage,
                    format!(
                        "{n} event(s), {lagged} lag signal(s){}",
                        if drained.is_err() {
                            " (still open)"
                        } else {
                            ""
                        }
                    )
                ),
            }
        }
        Err(e) => bad!(Method::SendStreamingMessage, e),
    }

    // ── GetTask ──────────────────────────────────────────────────────────
    if let Some(id) = &task_id {
        match client
            .get_task(TaskQueryParams {
                tenant: None,
                id: id.clone(),
                history_length: None,
            })
            .await
        {
            Ok(t) => ok!(Method::GetTask, format!("state {:?}", t.status.state)),
            Err(e) => bad!(Method::GetTask, e),
        }
    } else {
        bad!(Method::GetTask, "no task id from SendMessage");
    }

    // ── ListTasks ────────────────────────────────────────────────────────
    match client.list_tasks(ListTasksParams::default()).await {
        Ok(resp) => ok!(Method::ListTasks, format!("{} task(s)", resp.tasks.len())),
        Err(e) => bad!(Method::ListTasks, e),
    }

    // ── CancelTask ───────────────────────────────────────────────────────
    // The echo task completes immediately, so cancelling it is expected to be
    // refused with TaskNotCancelable. That refusal *is* the method working:
    // the server looked the task up, evaluated its state, and applied the
    // rule. Recording it as coverage — while a transport error is still a
    // failure — is the honest reading.
    match client.cancel_task(new_task_for_cancel(client).await).await {
        Ok(t) => ok!(Method::CancelTask, format!("state {:?}", t.status.state)),
        Err(e) if is_protocol_refusal(&e) => {
            ok!(Method::CancelTask, format!("refused as expected: {e}"));
        }
        Err(e) => bad!(Method::CancelTask, e),
    }

    // ── SubscribeToTask ──────────────────────────────────────────────────
    //
    // Re-attaching requires a task that is still running: the server answers
    // `UnsupportedOperation` for a terminal one, correctly, because it will
    // never produce another event. Every ordinary echo task is terminal by
    // the time its id is known, so this starts a deliberately slow one and
    // subscribes while it is mid-flight — otherwise the only observable
    // outcome is the refusal, and the success path stays uncovered.
    //
    // `streamed_task_id` from the sweep above is *not* reused for that
    // reason: it names a task that has already finished.
    let _ = &streamed_task_id;
    match start_slow_task(client, slow_prefix).await {
        Ok(slow_id) => match client.subscribe_to_task(slow_id.clone()).await {
            Ok(mut s) => {
                let mut events = 0_usize;
                let mut stream_err: Option<String> = None;
                // Bounded for the same reason as the send stream above.
                let _ = tokio::time::timeout(STREAM_DRAIN_BUDGET, async {
                    while let Some(ev) = s.next().await {
                        match ev {
                            Ok(_) => events += 1,
                            Err(e) if e.is_stream_lagged() => {}
                            Err(e) => {
                                // Never swallow this. Some bindings deliver a
                                // refusal *inside* the stream rather than as an
                                // immediate `Err`, so `break`ing quietly here
                                // turns a failed subscribe into "0 events" and
                                // records coverage for a call that did not
                                // work. That is exactly what this sweep did on
                                // its first run.
                                stream_err = Some(format!("{e}"));
                                return;
                            }
                        }
                        if events >= MAX_STREAM_EVENTS {
                            return;
                        }
                    }
                })
                .await;
                match stream_err {
                    Some(e) => bad!(Method::SubscribeToTask, format!("stream error: {e}")),
                    None if events == 0 => bad!(
                        Method::SubscribeToTask,
                        format!("subscribed to running task {slow_id} but received no events")
                    ),
                    None => ok!(Method::SubscribeToTask, format!("{events} event(s)")),
                }
            }
            Err(e) => bad!(Method::SubscribeToTask, e),
        },
        Err(e) => bad!(
            Method::SubscribeToTask,
            format!("could not start a slow task: {e}")
        ),
    }

    // ── Push-notification config CRUD ────────────────────────────────────
    let push_task_id = task_id.clone().unwrap_or_default();
    let mut created_config_id: Option<String> = None;

    match client
        .set_push_config(TaskPushNotificationConfig {
            tenant: None,
            id: None,
            task_id: Some(push_task_id.clone()),
            url: webhook_url.to_owned(),
            token: Some("echo-demo-token".into()),
            authentication: None,
        })
        .await
    {
        Ok(cfg) => {
            created_config_id = cfg.id.clone();
            ok!(
                Method::CreateTaskPushNotificationConfig,
                format!("config {:?}", cfg.id)
            );
        }
        Err(e) => bad!(Method::CreateTaskPushNotificationConfig, e),
    }

    match client
        .list_push_configs(ListPushConfigsParams {
            tenant: None,
            task_id: push_task_id.clone(),
            page_size: None,
            page_token: None,
        })
        .await
    {
        Ok(list) => ok!(
            Method::ListTaskPushNotificationConfigs,
            format!("{} config(s)", list.configs.len())
        ),
        Err(e) => bad!(Method::ListTaskPushNotificationConfigs, e),
    }

    if let Some(cfg_id) = created_config_id.clone() {
        match client
            .get_push_config(push_task_id.clone(), cfg_id.clone())
            .await
        {
            Ok(cfg) => ok!(
                Method::GetTaskPushNotificationConfig,
                format!("url {}", cfg.url)
            ),
            Err(e) => bad!(Method::GetTaskPushNotificationConfig, e),
        }
        match client.delete_push_config(push_task_id, cfg_id).await {
            Ok(()) => ok!(Method::DeleteTaskPushNotificationConfig, "deleted"),
            Err(e) => bad!(Method::DeleteTaskPushNotificationConfig, e),
        }
    } else {
        bad!(
            Method::GetTaskPushNotificationConfig,
            "no config id from create"
        );
        bad!(
            Method::DeleteTaskPushNotificationConfig,
            "no config id from create"
        );
    }

    // ── GetExtendedAgentCard ─────────────────────────────────────────────
    match client.get_extended_agent_card().await {
        Ok(card) => ok!(
            Method::GetExtendedAgentCard,
            format!("card '{}'", card.name)
        ),
        Err(e) => bad!(Method::GetExtendedAgentCard, e),
    }

    SweepOutcome { lines, failures }
}

/// Starts a task that stays in `Working` long enough to be subscribed to.
///
/// Uses the streaming send so the id is available immediately — a synchronous
/// send would not return until the task had already finished, which is the
/// problem being worked around.
async fn start_slow_task(client: &A2aClient, slow_prefix: &str) -> Result<String, String> {
    let params = make_send_params(&format!("{slow_prefix}subscribe target"));
    let mut stream = client
        .stream_message(params)
        .await
        .map_err(|e| format!("{e}"))?;

    // Read until an event names the task. The first is a Task snapshot, not a
    // status update.
    for _ in 0..10 {
        match stream.next().await {
            Some(Ok(resp)) => {
                if let Some(id) = task_id_of(&resp) {
                    return Ok(id);
                }
            }
            Some(Err(e)) if e.is_stream_lagged() => {}
            Some(Err(e)) => return Err(format!("{e}")),
            None => break,
        }
    }
    Err("slow task stream ended without naming a task".to_owned())
}

/// Creates a fresh task so `CancelTask` has something of its own to act on.
async fn new_task_for_cancel(client: &A2aClient) -> String {
    match client.send_message(make_send_params("cancel me")).await {
        Ok(SendMessageResponse::Task(t)) => t.id.to_string(),
        // A bogus id still drives the method; the server answers TaskNotFound,
        // which `is_protocol_refusal` accepts. Better than skipping the call.
        _ => "no-such-task-for-cancel".to_owned(),
    }
}

/// `true` when the server answered with a protocol-level decision rather than
/// a transport failure.
///
/// The distinction matters for coverage: a refusal means the method was
/// routed, parsed and evaluated — the thing being measured — whereas a
/// connection error means nothing was exercised at all.
fn is_protocol_refusal(e: &a2a_protocol_client::ClientError) -> bool {
    matches!(e, a2a_protocol_client::ClientError::Protocol(_))
}

fn task_id_of(resp: &a2a_protocol_types::events::StreamResponse) -> Option<String> {
    use a2a_protocol_types::events::StreamResponse as R;
    match resp {
        R::Task(t) => Some(t.id.0.clone()),
        R::StatusUpdate(ev) => Some(ev.task_id.0.clone()),
        R::ArtifactUpdate(ev) => Some(ev.task_id.0.clone()),
        _ => None,
    }
}
