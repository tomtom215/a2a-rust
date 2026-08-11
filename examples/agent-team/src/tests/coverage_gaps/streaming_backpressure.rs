// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Test 71: Backpressure — slow reader skips lagged events.
//!
//! Spins up an agent with a tiny event queue capacity and verifies that
//! the stream still completes even when the reader is slow and events are
//! dropped due to backpressure.

use super::*;

// ── Backpressure / Lagged (71) ───────────────────────────────────────────────

/// Test 71: When event queue capacity is tiny, rapid events cause lagging.
/// The stream still completes — the slow reader silently skips missed events.
pub async fn test_backpressure_lagged(_ctx: &TestContext) -> TestResult {
    let start = Instant::now();

    // Spin up an agent with capacity=2 (very small) to force lagging.
    let (listener, addr) = bind_listener().await;
    let url = format!("http://{addr}");

    let metrics = Arc::new(TeamMetrics::new("BackpressureTest"));
    let card = AgentCard {
        url: None,
        name: "BackpressureAgent".into(),
        description: "Agent with tiny event queue".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: url.clone(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "bp-test".into(),
            name: "Backpressure Test".into(),
            description: "Tests lagged events".into(),
            tags: vec![],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none().with_streaming(true),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    };

    let handler = Arc::new(
        RequestHandlerBuilder::new(crate::executors::CodeAnalyzerExecutor)
            .with_agent_card(card)
            .with_interceptor(AuditInterceptor::new("BackpressureTest"))
            .with_metrics(MetricsForward(Arc::clone(&metrics)))
            .with_event_queue_capacity(2)
            .build()
            .expect("build backpressure handler"),
    );
    serve_jsonrpc(listener, handler);

    // Use the SDK client to send a streaming request.
    let client = a2a_protocol_client::ClientBuilder::new(&url)
        .build()
        .unwrap();
    match client
        .stream_message(make_send_params(
            "fn bp() { let x = 1; let y = 2; let z = 3; }",
        ))
        .await
    {
        Ok(mut stream) => {
            let mut event_count = 0;
            let mut lag_signals = 0_u32;
            let mut dropped_total = 0_u64;
            let mut saw_completed = false;
            let mut fatal: Option<String> = None;

            // Grab the task id up front so the store can be consulted below
            // even when this consumer's stream is truncated by lag.
            let task_id = crate::helpers::first_task_id(&mut stream, 10).await;
            event_count += 1;

            while let Some(event) = stream.next().await {
                match event {
                    Ok(a2a_protocol_types::events::StreamResponse::StatusUpdate(ev)) => {
                        event_count += 1;
                        if ev.status.state == a2a_protocol_types::task::TaskState::Completed {
                            saw_completed = true;
                        }
                    }
                    Ok(_) => event_count += 1,
                    // A lag signal is recoverable and the stream continues —
                    // that is the whole contract being tested here. This arm
                    // used to be `Err(_) => break`, which abandoned the stream
                    // at the first lag and so could never observe the terminal
                    // status it then asserted on. The test was failing on its
                    // own error handling, not on server behaviour.
                    Err(e) if e.is_stream_lagged() => {
                        lag_signals += 1;
                        dropped_total += e.dropped_event_count().unwrap_or(0);
                    }
                    Err(e) => {
                        fatal = Some(format!("{e}"));
                        break;
                    }
                }
            }

            if let Some(err) = fatal {
                return TestResult::fail(
                    "backpressure-lagged",
                    start.elapsed().as_millis(),
                    &format!("non-lag stream error after {event_count} events: {err}"),
                );
            }
            // A lagged consumer may legitimately never see the terminal event:
            // the SDK's own lag message says to "resubscribe to resynchronize
            // from a fresh task snapshot", and the queue drops events *for
            // that consumer only* while the store stays authoritative.
            //
            // So the invariant worth asserting is not "the stream still
            // delivers Completed" — that is false by design, and asserting it
            // is what made this test red — but "backpressure costs you events,
            // never the task". Both earlier versions of this test got that
            // wrong in opposite directions: the original broke out of the loop
            // on the lag signal, and the first repair assumed the terminal
            // event would arrive anyway.
            if saw_completed {
                return TestResult::pass(
                    "backpressure-lagged",
                    start.elapsed().as_millis(),
                    &format!(
                        "{event_count} events, {lag_signals} lag signal(s) covering \
                         {dropped_total} dropped event(s), terminal status still streamed"
                    ),
                );
            }

            let Some(tid) = task_id else {
                return TestResult::fail(
                    "backpressure-lagged",
                    start.elapsed().as_millis(),
                    "stream never named a task",
                );
            };
            if lag_signals == 0 {
                return TestResult::fail(
                    "backpressure-lagged",
                    start.elapsed().as_millis(),
                    &format!(
                        "{event_count} events, no lag signal, yet the terminal status \
                         never arrived — truncation without a truncation signal"
                    ),
                );
            }
            // Lagged: the store must still show the task finished.
            match client
                .get_task(a2a_protocol_types::params::TaskQueryParams {
                    tenant: None,
                    id: tid.clone(),
                    history_length: None,
                })
                .await
            {
                Ok(task) if task.status.state == a2a_protocol_types::task::TaskState::Completed => {
                    TestResult::pass(
                        "backpressure-lagged",
                        start.elapsed().as_millis(),
                        &format!(
                            "{event_count} events, {lag_signals} lag signal(s) covering \
                             {dropped_total} dropped event(s); stream truncated but the \
                             store still reports Completed"
                        ),
                    )
                }
                Ok(task) => TestResult::fail(
                    "backpressure-lagged",
                    start.elapsed().as_millis(),
                    &format!(
                        "lagged consumer missed the terminal event AND the store reports \
                         {:?} for task {tid} — backpressure lost the task, not just events",
                        task.status.state
                    ),
                ),
                Err(e) => TestResult::fail(
                    "backpressure-lagged",
                    start.elapsed().as_millis(),
                    &format!("could not verify task {tid} in the store after lag: {e}"),
                ),
            }
        }
        Err(e) => TestResult::fail(
            "backpressure-lagged",
            start.elapsed().as_millis(),
            &format!("stream error: {e}"),
        ),
    }
}
