// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! TCK test runner — executes all conformance tests against a target server.

use crate::tests;

/// Result of a single conformance test.
pub struct TestResult {
    /// Test name (e.g., "agent_card_discovery").
    pub name: String,
    /// Whether the test passed.
    pub passed: bool,
    /// Human-readable message (error details on failure, "ok" on success).
    pub message: String,
}

impl TestResult {
    pub fn pass(name: &str) -> Self {
        Self {
            name: name.to_string(),
            passed: true,
            message: "ok".to_string(),
        }
    }

    pub fn fail(name: &str, msg: impl Into<String>) -> Self {
        Self {
            name: name.to_string(),
            passed: false,
            message: msg.into(),
        }
    }
}

/// Runs all TCK conformance tests against the given server URL.
pub async fn run_all(url: &str, binding: &str) -> Vec<TestResult> {
    let mut results = Vec::new();

    // ── Agent Card Discovery ──────────────────────────────────────────────
    run_test(&mut results, "agent_card_discovery", async {
        tests::agent_card::test_agent_card_discovery(url).await
    })
    .await;

    run_test(&mut results, "agent_card_required_fields", async {
        tests::agent_card::test_agent_card_required_fields(url).await
    })
    .await;

    run_test(&mut results, "agent_card_content_type", async {
        tests::agent_card::test_agent_card_content_type(url).await
    })
    .await;

    // ── SendMessage ───────────────────────────────────────────────────────
    run_test(&mut results, "send_message_basic", async {
        tests::messaging::test_send_message_basic(url, binding).await
    })
    .await;

    run_test(&mut results, "send_message_returns_task", async {
        tests::messaging::test_send_message_returns_task(url, binding).await
    })
    .await;

    run_test(&mut results, "send_message_context_id", async {
        tests::messaging::test_send_message_context_id(url, binding).await
    })
    .await;

    // ── GetTask ───────────────────────────────────────────────────────────
    run_test(&mut results, "get_task_existing", async {
        tests::task_ops::test_get_task_existing(url, binding).await
    })
    .await;

    run_test(&mut results, "get_task_not_found", async {
        tests::task_ops::test_get_task_not_found(url, binding).await
    })
    .await;

    // ── ListTasks ─────────────────────────────────────────────────────────
    run_test(&mut results, "list_tasks_basic", async {
        tests::task_ops::test_list_tasks_basic(url, binding).await
    })
    .await;

    // ── CancelTask ────────────────────────────────────────────────────────
    run_test(&mut results, "cancel_task", async {
        tests::task_ops::test_cancel_task(url, binding).await
    })
    .await;

    // ── Streaming ─────────────────────────────────────────────────────────
    run_test(&mut results, "streaming_send_message", async {
        tests::streaming::test_streaming_send_message(url, binding).await
    })
    .await;

    // ── Push Notification Config ──────────────────────────────────────────
    run_test(&mut results, "push_config_create", async {
        tests::push_config::test_create_push_config(url, binding).await
    })
    .await;

    run_test(&mut results, "push_config_get", async {
        tests::push_config::test_get_push_config(url, binding).await
    })
    .await;

    run_test(&mut results, "push_config_list", async {
        tests::push_config::test_list_push_configs(url, binding).await
    })
    .await;

    run_test(&mut results, "push_config_delete", async {
        tests::push_config::test_delete_push_config(url, binding).await
    })
    .await;

    // ── Error Handling ────────────────────────────────────────────────────
    run_test(&mut results, "invalid_method_returns_error", async {
        tests::errors::test_invalid_method_returns_error(url, binding).await
    })
    .await;

    run_test(&mut results, "invalid_params_returns_error", async {
        tests::errors::test_invalid_params_returns_error(url, binding).await
    })
    .await;

    // ── Wire Format ───────────────────────────────────────────────────────
    run_test(&mut results, "jsonrpc_envelope_format", async {
        tests::wire_format::test_jsonrpc_envelope_format(url, binding).await
    })
    .await;

    run_test(&mut results, "task_state_values", async {
        tests::wire_format::test_task_state_values(url, binding).await
    })
    .await;

    results
}

async fn run_test<F>(results: &mut Vec<TestResult>, name: &str, test: F)
where
    F: std::future::Future<Output = Result<(), String>>,
{
    let status_icon = match test.await {
        Ok(()) => {
            results.push(TestResult::pass(name));
            "PASS"
        }
        Err(msg) => {
            results.push(TestResult::fail(name, &msg));
            "FAIL"
        }
    };
    println!("  [{status_icon}] {name}");
}
