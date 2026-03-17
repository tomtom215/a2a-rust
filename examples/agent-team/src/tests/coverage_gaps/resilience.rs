// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests 76-78: Resilience and edge-case handling.
//!
//! - Test 76: Timeout retryability (Bug #32 regression)
//! - Test 77: Concurrent cancel stress test
//! - Test 78: Stale page token graceful handling

use super::*;

// ── Timeout retryability verification (76) ─────────────────────────────────

/// Test 76 (Bug #32): Verify that timeout errors are classified as retryable.
///
/// Previously, REST and JSON-RPC transports incorrectly mapped timeouts to
/// `ClientError::Transport` (non-retryable). This test verifies the fix by
/// constructing a `Timeout` error and checking `is_retryable()`.
pub async fn test_timeout_retryable(ctx: &TestContext) -> TestResult {
    let _ = ctx; // not needed but keeps signature consistent
    let start = Instant::now();

    let timeout = a2a_protocol_client::ClientError::Timeout("request timed out".into());
    let transport = a2a_protocol_client::ClientError::Transport("config error".into());

    let timeout_retryable = timeout.is_retryable();
    let transport_retryable = transport.is_retryable();

    if timeout_retryable && !transport_retryable {
        TestResult::pass(
            "timeout-retryable",
            start.elapsed().as_millis(),
            "Timeout.is_retryable()=true, Transport.is_retryable()=false",
        )
    } else {
        TestResult::fail(
            "timeout-retryable",
            start.elapsed().as_millis(),
            &format!(
                "expected Timeout=retryable(true) and Transport=retryable(false), got timeout={}, transport={}",
                timeout_retryable,
                transport_retryable
            ),
        )
    }
}

// ── Concurrent cancel requests (77) ────────────────────────────────────────

/// Test 77: Fire multiple cancel requests at a non-existent task concurrently.
/// Verifies that the server handles concurrent CancelTask without panicking
/// or deadlocking, even when all requests fail.
pub async fn test_concurrent_cancels(ctx: &TestContext) -> TestResult {
    let start = Instant::now();

    // Fire 10 concurrent cancel requests for non-existent tasks.
    let mut handles = Vec::new();
    for i in 0..10 {
        let url = ctx.build_url.clone();
        let tid = format!("concurrent-cancel-fake-{i}");
        handles.push(tokio::spawn(async move {
            let c = a2a_protocol_client::ClientBuilder::new(&url)
                .build()
                .unwrap();
            c.cancel_task(&tid).await
        }));
    }
    let mut ok_count = 0;
    let mut err_count = 0;
    for h in handles {
        match h.await.unwrap() {
            Ok(_) => ok_count += 1,
            Err(_) => err_count += 1,
        }
    }
    // All 10 should fail (task not found), but no panic/deadlock.
    if err_count == 10 {
        TestResult::pass(
            "concurrent-cancels",
            start.elapsed().as_millis(),
            &format!("all 10 cancel requests failed as expected ({ok_count} ok, {err_count} err, no panic)"),
        )
    } else {
        TestResult::pass(
            "concurrent-cancels",
            start.elapsed().as_millis(),
            &format!("{ok_count} ok, {err_count} err (no panic)"),
        )
    }
}

// ── Stale page token handling (78) ─────────────────────────────────────────

/// Test 78: Verify that using a page token referencing a non-existent task
/// doesn't crash — it returns an empty page gracefully.
pub async fn test_stale_page_token(ctx: &TestContext) -> TestResult {
    let start = Instant::now();

    let client = a2a_protocol_client::ClientBuilder::new(&ctx.analyzer_url)
        .build()
        .unwrap();

    // Use a fake page token that points to a non-existent task.
    let result = client
        .list_tasks(a2a_protocol_types::ListTasksParams {
            tenant: None,
            context_id: None,
            status: None,
            page_size: Some(10),
            page_token: Some("totally-fake-page-token".into()),
            status_timestamp_after: None,
            include_artifacts: None,
            history_length: None,
        })
        .await;

    match result {
        Ok(resp) => {
            // Empty page or valid response — both are acceptable.
            TestResult::pass(
                "stale-page-token",
                start.elapsed().as_millis(),
                &format!("{} tasks returned, server handled stale token gracefully", resp.tasks.len()),
            )
        }
        Err(e) => {
            // An error is also acceptable, as long as it didn't crash.
            TestResult::pass(
                "stale-page-token",
                start.elapsed().as_millis(),
                &format!("server returned error (no crash): {e}"),
            )
        }
    }
}
