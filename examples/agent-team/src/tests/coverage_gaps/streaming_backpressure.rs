// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

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
            let mut saw_completed = false;
            while let Some(event) = stream.next().await {
                match event {
                    Ok(a2a_protocol_types::events::StreamResponse::StatusUpdate(ev)) => {
                        event_count += 1;
                        if ev.status.state == a2a_protocol_types::task::TaskState::Completed {
                            saw_completed = true;
                        }
                    }
                    Ok(_) => event_count += 1,
                    Err(_) => break,
                }
            }
            // With capacity=2, the reader may miss some events but should still
            // see the final Completed status (it's the last thing emitted).
            if saw_completed {
                TestResult::pass(
                    "backpressure-lagged",
                    start.elapsed().as_millis(),
                    &format!("{event_count} events received, completed=true"),
                )
            } else {
                TestResult::fail(
                    "backpressure-lagged",
                    start.elapsed().as_millis(),
                    &format!("{event_count} events received, but Completed status was not seen"),
                )
            }
        }
        Err(e) => TestResult::fail(
            "backpressure-lagged",
            start.elapsed().as_millis(),
            &format!("stream error: {e}"),
        ),
    }
}
