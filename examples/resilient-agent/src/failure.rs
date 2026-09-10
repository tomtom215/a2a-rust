// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Act 2 — failure injection, with the numbers the SDK reports and a plain
//! statement of what it does not do.
//!
//! Three faults, each injected on a real socket or in a real executor:
//!
//! 1. An **executor** that fails its first N attempts. Finding: the SDK does
//!    not retry an executor. Each failure is a task in `TASK_STATE_FAILED`
//!    whose status message carries the error text (and whose streamed status
//!    event carries it again as `metadata.error`); the retry is the caller's,
//!    and it is a *new task* — a follow-up message on the failed one is
//!    refused. A client `RetryPolicy` does not change that: a `Failed` task
//!    is a successful RPC.
//! 2. A **webhook** that refuses its first M deliveries. The
//!    [`Metrics::on_push_delivery`] hook reports one outcome per event per
//!    config, and the act puts those outcomes beside the webhook's own tally.
//!    Finding: a delivery reported `failed` is not queued for later — the only
//!    retry is the sender's own attempt schedule inside one delivery, and at
//!    the shipped defaults that schedule is cut short by the handler's
//!    5-second bound.
//! 3. A **proxy** that faults its first K requests, once by dropping the
//!    connection and once with `503`. The client's `RetryPolicy` rides out
//!    both on an idempotent `GetTask`; on `SendMessage` it rides out only the
//!    `503`, because a dropped connection is ambiguous and a re-send could
//!    run the work twice.
//!
//! [`Metrics::on_push_delivery`]: a2a_protocol_server::metrics::Metrics::on_push_delivery

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_client::{ClientBuilder, RetryPolicy};
use a2a_protocol_server::HandlerLimits;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::push::{HttpPushSender, PushRetryPolicy, PushSender as _};
use a2a_protocol_types::params::TaskQueryParams;
use a2a_protocol_types::task::TaskState;
use hyper::StatusCode;

use crate::Check;
use crate::support::executors::{FlakyExecutor, FlakyTally, StreamingExecutor};
use crate::support::injectors::{Fault, faulting_proxy, webhook_sink};
use crate::support::metrics::{RecordingMetrics, render_counts};
use crate::support::{client, expect_task, is_refusal, send_params, send_params_with_push, serve};

pub async fn run() -> Vec<Check> {
    vec![
        executor_failures().await,
        push_refusals().await,
        client_retry().await,
    ]
}

// ── 1. The executor ─────────────────────────────────────────────────────────

const EXECUTOR_LABEL: &str = "Executor failing its first N attempts: the SDK does not retry it";

/// How many attempts the executor fails before succeeding.
const EXECUTOR_FAILURES: u32 = 2;

async fn executor_failures() -> Check {
    Check::from_result(EXECUTOR_LABEL, executor_failures_inner().await)
}

async fn executor_failures_inner() -> Result<String, String> {
    let tally = Arc::new(FlakyTally::failing_first(EXECUTOR_FAILURES));
    let handler = RequestHandlerBuilder::new(FlakyExecutor(Arc::clone(&tally)))
        .build()
        .map_err(|e| format!("building the handler: {e}"))?;
    let (_handler, url) = serve(handler).await?;

    // A generous client retry policy, to show it makes no difference here.
    let retries = 5;
    let client = ClientBuilder::new(&url)
        .with_retry_policy(
            RetryPolicy::default()
                .with_max_retries(retries)
                .with_initial_backoff(Duration::from_millis(10)),
        )
        .build()
        .map_err(|e| format!("building the client: {e}"))?;

    let sends = EXECUTOR_FAILURES + 1;
    let mut states = Vec::new();
    let mut first_failed_id = None;
    let mut first_failed_text = None;
    for n in 1..=sends {
        let task = expect_task(
            client
                .send_message(send_params(&format!("attempt {n}")))
                .await
                .map_err(|e| format!("send {n}: the RPC itself failed: {e}"))?,
        )?;
        // What a blocking caller has to go on: the state and the status
        // message. Since 2026-09-10 the executor's error text is the status
        // message of the Failed task (an agent-role message with one text
        // part), so a blocking caller learns why without streaming.
        let status_text = task
            .status
            .message
            .as_ref()
            .and_then(|m| m.text())
            .map(str::to_owned);
        if task.status.state == TaskState::Failed && first_failed_id.is_none() {
            first_failed_id = Some(task.id.0.clone());
            first_failed_text.clone_from(&status_text);
        }
        states.push((task.status.state, status_text));
    }

    // The first N are Failed and say why; the last is Completed.
    for (n, (state, status_text)) in states.iter().enumerate() {
        let expected = if n < EXECUTOR_FAILURES as usize {
            TaskState::Failed
        } else {
            TaskState::Completed
        };
        if *state != expected {
            return Err(format!(
                "send {} should have produced a {expected:?} task, produced {state:?}",
                n + 1
            ));
        }
        if expected == TaskState::Failed
            && !status_text
                .as_deref()
                .is_some_and(|t| t.contains("injected"))
        {
            return Err(format!(
                "send {}: the Failed task's status message should carry the executor's error \
                 text, got {status_text:?} — a blocking caller is back to seeing only the state",
                n + 1
            ));
        }
    }

    // One invocation per send: the client's retry policy did not re-run a
    // Failed task, and neither did the handler.
    let invocations = tally.invocations();
    if invocations != sends {
        return Err(format!(
            "{sends} sends produced {invocations} executor invocations — something is retrying \
             the executor, and the README says nothing does"
        ));
    }

    // A follow-up on the failed task is refused: the retry is a new task.
    let failed_id = first_failed_id.ok_or("no task failed, so the injector did not fire")?;
    let mut follow_up = send_params("try again on the same task");
    follow_up.message.task_id = Some(a2a_protocol_types::task::TaskId::new(failed_id.clone()));
    match client.send_message(follow_up).await {
        Ok(_) => {
            return Err(format!(
                "a follow-up message on Failed task {failed_id} was accepted — a terminal task \
                 must refuse new messages"
            ));
        }
        Err(e) if is_refusal(&e) => {}
        Err(e) => return Err(format!("the follow-up never reached the server: {e}")),
    }

    // The same text, read back through GetTask after the fact: the status
    // message is persisted with the task, not only returned from the send.
    let fetched = client
        .get_task(TaskQueryParams {
            tenant: None,
            id: failed_id.clone(),
            history_length: None,
        })
        .await
        .map_err(|e| format!("GetTask on the Failed task: {e}"))?;
    let fetched_text = fetched
        .status
        .message
        .as_ref()
        .and_then(|m| m.text())
        .map(str::to_owned);
    if fetched_text != first_failed_text {
        return Err(format!(
            "GetTask on Failed task {failed_id} returned status message {fetched_text:?}, the \
             send returned {first_failed_text:?} — the error text is not persisted"
        ));
    }
    let failed_text = first_failed_text.unwrap_or_default();

    // Streaming callers still get the text where they always did: as
    // `metadata.error` on the terminal status event.
    let streamed_error = streamed_failure_text().await?;

    Ok(format!(
        "N={EXECUTOR_FAILURES}: sends 1..={EXECUTOR_FAILURES} -> Failed, send {sends} -> \
         Completed; executor invoked {invocations}x for {sends} sends with a {retries}-retry \
         client policy (a Failed task is a successful RPC); a follow-up on the Failed task is \
         refused. The Failed task's status message carries the error text ({failed_text:?}), \
         GetTask returns the same, and the streamed status event still carries it as \
         metadata.error ({streamed_error:?})"
    ))
}

/// Streams one send against an executor that fails once and returns the
/// `metadata.error` of the terminal status event.
async fn streamed_failure_text() -> Result<String, String> {
    let handler = RequestHandlerBuilder::new(FlakyExecutor(Arc::new(FlakyTally::failing_first(1))))
        .build()
        .map_err(|e| format!("building the streaming handler: {e}"))?;
    let (_handler, url) = serve(handler).await?;
    let mut stream = client(&url)?
        .stream_message(send_params("stream me"))
        .await
        .map_err(|e| format!("SendStreamingMessage: {e}"))?;
    let mut error = None;
    while let Some(event) = stream.next().await {
        let event = event.map_err(|e| format!("stream error: {e}"))?;
        if let a2a_protocol_types::events::StreamResponse::StatusUpdate(update) = &event
            && update.status.state == TaskState::Failed
        {
            error = update
                .metadata
                .as_ref()
                .and_then(|m| m.get("error"))
                .and_then(|e| e.as_str())
                .map(str::to_owned);
        }
    }
    match error {
        Some(text) if text.contains("injected") => Ok(text),
        other => Err(format!(
            "the streamed Failed status event should carry the executor's error as metadata.error, got {other:?}"
        )),
    }
}

// ── 2. The webhook ──────────────────────────────────────────────────────────

const PUSH_LABEL: &str = "Push webhook refusing its first M deliveries: what Metrics reports";

/// How many deliveries the webhook refuses.
const PUSH_REFUSALS: u32 = 2;

async fn push_refusals() -> Check {
    Check::from_result(PUSH_LABEL, push_refusals_inner().await)
}

/// Runs one task against a webhook that refuses `PUSH_REFUSALS` deliveries,
/// with a sender allowed `attempts` tries per delivery.
///
/// Returns `(outcomes as the Metrics hook saw them, refused, accepted)`.
async fn deliver_through(
    attempts: usize,
) -> Result<(std::collections::BTreeMap<String, usize>, u32, u32), String> {
    let (webhook, sink) = webhook_sink(PUSH_REFUSALS, StatusCode::SERVICE_UNAVAILABLE).await;
    let metrics = Arc::new(RecordingMetrics::default());
    let handler = RequestHandlerBuilder::new(StreamingExecutor {
        step: Duration::from_millis(10),
        die_after_first_chunk: false,
    })
    .with_metrics(Arc::clone(&metrics))
    .with_push_sender(
        HttpPushSender::with_timeout(Duration::from_secs(1))
            .with_retry_policy(
                PushRetryPolicy::default()
                    .with_max_attempts(attempts)
                    .with_backoff(vec![Duration::from_millis(10)]),
            )
            .allow_private_urls(),
    )
    .build()
    .map_err(|e| format!("building the handler: {e}"))?;
    let (_handler, url) = serve(handler).await?;
    let client = client(&url)?;

    let task = expect_task(
        client
            .send_message(send_params_with_push("notify me", &webhook))
            .await
            .map_err(|e| format!("SendMessage: {e}"))?,
    )?;
    if task.status.state != TaskState::Completed {
        return Err(format!(
            "the task should complete regardless of push outcomes, is {:?}",
            task.status.state
        ));
    }
    // Every event the executor emitted is one delivery. Wait for the hook
    // to have reported as many outcomes as the webhook saw requests plus
    // the refusals that never became a request (none here — refusals are
    // requests too), i.e. one outcome per event.
    let events = 4; // Working, chunk one, chunk two, Completed
    let seen = metrics
        .wait_for_push_outcomes(events, Duration::from_secs(5))
        .await;
    if seen != events {
        return Err(format!(
            "expected the Metrics hook to report {events} push outcomes (one per event), it reported {seen}: {:?}",
            metrics.push_outcomes()
        ));
    }
    Ok((metrics.push_counts(), sink.refused(), sink.accepted()))
}

async fn push_refusals_inner() -> Result<String, String> {
    // (a) One attempt per delivery: the first M events fail outright.
    let (single, refused_a, accepted_a) = deliver_through(1).await?;
    let failed = single.get("failed").copied().unwrap_or(0);
    let delivered = single.get("delivered").copied().unwrap_or(0);
    if failed != PUSH_REFUSALS as usize || refused_a != PUSH_REFUSALS {
        return Err(format!(
            "with 1 attempt per delivery, {PUSH_REFUSALS} refusals should be {PUSH_REFUSALS} \
             `failed` outcomes; the hook reported {} and the webhook refused {refused_a}",
            render_counts(&single)
        ));
    }
    if delivered != accepted_a as usize {
        return Err(format!(
            "the hook reported {delivered} `delivered` but the webhook accepted {accepted_a}"
        ));
    }

    // (b) M+1 attempts per delivery: the sender's own retries absorb them.
    let attempts = PUSH_REFUSALS as usize + 1;
    let (retried, refused_b, accepted_b) = deliver_through(attempts).await?;
    let delivered_b = retried.get("delivered").copied().unwrap_or(0);
    if retried.get("failed").copied().unwrap_or(0) != 0 || refused_b != PUSH_REFUSALS {
        return Err(format!(
            "with {attempts} attempts per delivery, {PUSH_REFUSALS} refusals should all be \
             absorbed; the hook reported {} and the webhook refused {refused_b}",
            render_counts(&retried)
        ));
    }

    // (c) The shipped defaults, computed rather than waited for.
    let wants = HttpPushSender::new()
        .max_delivery_duration()
        .map_or("unknown".to_owned(), |d| format!("{d:?}"));
    let bound = HandlerLimits::default().push_delivery_timeout;

    Ok(format!(
        "M={PUSH_REFUSALS}, 4 events: sender with 1 attempt -> {} (webhook refused {refused_a}, \
         accepted {accepted_a}); sender with {attempts} attempts -> {} (webhook refused {refused_b}, \
         accepted {accepted_b}, {delivered_b} delivered after in-sender retries). A `failed` \
         delivery is not re-queued. Defaults: HttpPushSender::new() schedules {wants} per delivery \
         against push_delivery_timeout={bound:?}, so its retries are cut short (`timeout_truncated`)",
        render_counts(&single),
        render_counts(&retried),
    ))
}

// ── 3. The transport ────────────────────────────────────────────────────────

const RETRY_LABEL: &str =
    "Client RetryPolicy: transport faults retried only where a re-send is safe";

/// How many requests the proxy faults before letting one through.
const PROXY_FAULTS: u32 = 2;

async fn client_retry() -> Check {
    Check::from_result(RETRY_LABEL, client_retry_inner().await)
}

async fn client_retry_inner() -> Result<String, String> {
    let tally = Arc::new(FlakyTally::failing_first(0));
    let handler = RequestHandlerBuilder::new(FlakyExecutor(Arc::clone(&tally)))
        .build()
        .map_err(|e| format!("building the handler: {e}"))?;
    let (_handler, agent_url) = serve(handler).await?;

    // A task to GetTask against, created directly.
    let direct = client(&agent_url)?;
    let task_id = expect_task(
        direct
            .send_message(send_params("seed"))
            .await
            .map_err(|e| format!("seeding a task: {e}"))?,
    )?
    .id
    .0;
    let query = || TaskQueryParams {
        tenant: None,
        id: task_id.clone(),
        history_length: None,
    };
    let policy = || {
        RetryPolicy::default()
            .with_max_retries(PROXY_FAULTS + 1)
            .with_initial_backoff(Duration::from_millis(10))
            .with_max_backoff(Duration::from_millis(50))
    };

    // (a) No policy, dropped connection: the fault is real.
    let (plain_url, plain) = faulting_proxy(agent_url.clone(), 1, Fault::DropConnection).await;
    if client(&plain_url)?.get_task(query()).await.is_ok() {
        return Err(
            "a client with no retry policy survived a dropped connection — the injector is not \
             faulting, so the rest of this check would be vacuous"
                .to_owned(),
        );
    }
    let _ = plain;

    // (b) Policy, dropped connection, GetTask: idempotent, so retried.
    let (drop_url, drops) =
        faulting_proxy(agent_url.clone(), PROXY_FAULTS, Fault::DropConnection).await;
    let retrying = ClientBuilder::new(&drop_url)
        .with_retry_policy(policy())
        .build()
        .map_err(|e| format!("building the retrying client: {e}"))?;
    retrying.get_task(query()).await.map_err(|e| {
        format!("GetTask through {PROXY_FAULTS} dropped connections was not ridden out: {e}")
    })?;
    let (get_faulted, get_forwarded) = (drops.faulted(), drops.forwarded());

    // (c) Same policy, dropped connection, SendMessage: ambiguous, so not.
    let (drop_url, drops) =
        faulting_proxy(agent_url.clone(), PROXY_FAULTS, Fault::DropConnection).await;
    let retrying = ClientBuilder::new(&drop_url)
        .with_retry_policy(policy())
        .build()
        .map_err(|e| format!("building the retrying client: {e}"))?;
    if retrying
        .send_message(send_params("ambiguous"))
        .await
        .is_ok()
    {
        return Err(
            "SendMessage over a dropped connection was retried to success — an ambiguous failure \
             on a non-idempotent method can run the work twice"
                .to_owned(),
        );
    }
    let (send_drop_faulted, send_drop_forwarded) = (drops.faulted(), drops.forwarded());

    // (d) Same policy, 503, SendMessage: refused up front, so safe to retry.
    let (status_url, statuses) = faulting_proxy(
        agent_url.clone(),
        PROXY_FAULTS,
        Fault::Status(StatusCode::SERVICE_UNAVAILABLE),
    )
    .await;
    let retrying = ClientBuilder::new(&status_url)
        .with_retry_policy(policy())
        .build()
        .map_err(|e| format!("building the retrying client: {e}"))?;
    expect_task(
        retrying
            .send_message(send_params("refused up front"))
            .await
            .map_err(|e| {
                format!("SendMessage through {PROXY_FAULTS} x 503 was not ridden out: {e}")
            })?,
    )?;
    let (send_503_faulted, send_503_forwarded) = (statuses.faulted(), statuses.forwarded());

    // The executor ran once per task the agent actually received — the seed,
    // and the 503 case. If a dropped-connection SendMessage had been re-sent
    // behind the caller's back, this would be higher.
    let invocations = tally.invocations();
    if invocations != 2 {
        return Err(format!(
            "the agent ran its executor {invocations} times; expected 2 (the seed and the 503 \
             case) — a dropped-connection SendMessage reached it"
        ));
    }

    Ok(format!(
        "K={PROXY_FAULTS}: GetTask over dropped connections -> ok (proxy faulted {get_faulted}, \
         forwarded {get_forwarded}); SendMessage over dropped connections -> error to caller \
         (faulted {send_drop_faulted}, forwarded {send_drop_forwarded}: not retried, ambiguous); \
         SendMessage over 503s -> ok (faulted {send_503_faulted}, forwarded {send_503_forwarded}); \
         executor ran {invocations}x"
    ))
}
