// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests 72-75: Push notification configuration.
//!
//! - Test 72: Global config limit enforcement
//! - Test 73: Webhook URL scheme validation (rejects non-HTTP)
//! - Test 74: Combined status + context_id filter on ListTasks
//! - Test 75: Latency metrics callback fires for completed requests

use super::*;

// ── Push config global limit (72) ────────────────────────────────────────

/// Test 72: The InMemoryPushConfigStore enforces a global config limit.
pub async fn test_push_config_global_limit(_ctx: &TestContext) -> TestResult {
    use a2a_protocol_server::push::InMemoryPushConfigStore;
    use a2a_protocol_server::push::PushConfigStore;

    let start = Instant::now();

    // Create a store with a very small global limit (3) and per-task limit (100).
    let store = InMemoryPushConfigStore::with_max_configs_per_task(100).with_max_total_configs(3);

    // Insert 3 configs across 3 different tasks — should succeed.
    for i in 0..3 {
        let config = a2a_protocol_types::push::TaskPushNotificationConfig::new(
            format!("task-{i}"),
            format!("https://example.com/hook-{i}"),
        );
        if let Err(e) = store.set(config).await {
            return TestResult::fail(
                "push-global-limit",
                start.elapsed().as_millis(),
                &format!("config {i} should have succeeded but failed: {e}"),
            );
        }
    }

    // 4th config should be rejected (global limit = 3).
    let config = a2a_protocol_types::push::TaskPushNotificationConfig::new(
        "task-3",
        "https://example.com/hook-overflow",
    );
    let result = store.set(config).await;
    match result {
        Err(e) => TestResult::pass(
            "push-global-limit",
            start.elapsed().as_millis(),
            &format!("global limit enforced at 3 configs: {e}"),
        ),
        Ok(_) => TestResult::fail(
            "push-global-limit",
            start.elapsed().as_millis(),
            "4th config should have been rejected by global limit of 3",
        ),
    }
}

// ── Webhook URL scheme validation (73) ───────────────────────────────────

/// Test 73: Webhook URL scheme validation rejects non-HTTP schemes.
pub async fn test_webhook_url_scheme_validation(_ctx: &TestContext) -> TestResult {
    use a2a_protocol_server::push::PushSender;

    let start = Instant::now();

    let sender = a2a_protocol_server::push::HttpPushSender::new();
    let event = a2a_protocol_types::events::StreamResponse::StatusUpdate(
        a2a_protocol_types::events::TaskStatusUpdateEvent {
            task_id: a2a_protocol_types::task::TaskId::new("t1"),
            context_id: a2a_protocol_types::task::ContextId::new("c1"),
            status: a2a_protocol_types::task::TaskStatus::new(
                a2a_protocol_types::task::TaskState::Working,
            ),
            metadata: None,
        },
    );
    let config =
        a2a_protocol_types::push::TaskPushNotificationConfig::new("t1", "ftp://evil.com/hook");

    let result = sender.send("ftp://evil.com/hook", &event, &config).await;
    match result {
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("unsupported scheme") {
                TestResult::pass(
                    "webhook-url-scheme",
                    start.elapsed().as_millis(),
                    &format!("ftp:// scheme correctly rejected: {msg}"),
                )
            } else {
                TestResult::fail(
                    "webhook-url-scheme",
                    start.elapsed().as_millis(),
                    &format!("expected 'unsupported scheme' in error message, got: {msg}"),
                )
            }
        }
        Ok(_) => TestResult::fail(
            "webhook-url-scheme",
            start.elapsed().as_millis(),
            "ftp:// should have been rejected but request succeeded",
        ),
    }
}

// ── Combined status + context filter (74) ────────────────────────────────

/// Test 74: ListTasks with both status and context_id filters applied together.
pub async fn test_combined_status_context_filter(ctx: &TestContext) -> TestResult {
    let start = Instant::now();

    let unique_ctx = uuid::Uuid::new_v4().to_string();

    // Send a message with a unique context_id.
    let mut params = make_send_params("fn combined_test() {}");
    params.message.context_id = Some(a2a_protocol_types::task::ContextId::new(unique_ctx.clone()));
    let client = a2a_protocol_client::ClientBuilder::new(&ctx.analyzer_url)
        .build()
        .unwrap();

    match client.send_message(params).await {
        Ok(a2a_protocol_types::responses::SendMessageResponse::Task(task)) => {
            // Now list with both status=Completed and context_id filter.
            let list_params = a2a_protocol_types::params::ListTasksParams {
                tenant: None,
                context_id: Some(unique_ctx.clone()),
                status: Some(task.status.state),
                page_size: Some(10),
                page_token: None,
                status_timestamp_after: None,
                include_artifacts: None,
                history_length: None,
            };
            match client.list_tasks(list_params).await {
                Ok(resp) => {
                    if resp.tasks.len() == 1 && resp.tasks[0].id == task.id {
                        TestResult::pass(
                            "combined-filter",
                            start.elapsed().as_millis(),
                            &format!("status+context filter returned exactly 1 matching task (id={})", task.id),
                        )
                    } else {
                        TestResult::fail(
                            "combined-filter",
                            start.elapsed().as_millis(),
                            &format!(
                                "expected exactly 1 task with id={}, got {} tasks",
                                task.id,
                                resp.tasks.len()
                            ),
                        )
                    }
                }
                Err(e) => TestResult::fail(
                    "combined-filter",
                    start.elapsed().as_millis(),
                    &format!("list error: {e}"),
                ),
            }
        }
        Ok(_) => TestResult::fail(
            "combined-filter",
            start.elapsed().as_millis(),
            "expected Task response from SendMessage",
        ),
        Err(e) => TestResult::fail(
            "combined-filter",
            start.elapsed().as_millis(),
            &format!("send error: {e}"),
        ),
    }
}

// ── Latency metrics callback (75) ────────────────────────────────────────

/// Test 75: Verify metrics callback fires for completed requests.
pub async fn test_latency_metrics(ctx: &TestContext) -> TestResult {
    let start = Instant::now();

    // Send a request and verify the request count incremented.
    let before = ctx.analyzer_metrics.request_count();
    let client = a2a_protocol_client::ClientBuilder::new(&ctx.analyzer_url)
        .build()
        .unwrap();
    let _ = client.send_message(make_send_params("latency test")).await;
    let after = ctx.analyzer_metrics.request_count();

    if after > before {
        TestResult::pass(
            "latency-metrics",
            start.elapsed().as_millis(),
            &format!("metrics incremented: {} -> {} (delta={})", before, after, after - before),
        )
    } else {
        TestResult::fail(
            "latency-metrics",
            start.elapsed().as_millis(),
            &format!(
                "metrics should have incremented but stayed at {} -> {}",
                before, after
            ),
        )
    }
}
