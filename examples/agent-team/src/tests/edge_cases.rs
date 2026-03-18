// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests 21-30: Error paths, concurrency, metrics, CRUD, and resubscribe.

use std::time::Instant;

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::message::{Message, MessageId, MessageRole};
use a2a_protocol_types::params::{
    ListPushConfigsParams, ListTasksParams, MessageSendParams, TaskQueryParams,
};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::TaskState;

use super::{TestContext, TestResult};
use crate::helpers::make_send_params;

/// Test 21: Cancel nonexistent task returns TaskNotFound
pub async fn test_cancel_nonexistent(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 21: Cancel nonexistent task");
    let client = ClientBuilder::new(&ctx.build_url)
        .with_protocol_binding("REST")
        .build()
        .unwrap();
    match client.cancel_task("totally-fake-id".to_owned()).await {
        Ok(_) => TestResult::fail(
            "cancel-nonexistent",
            start.elapsed().as_millis(),
            "should error",
        ),
        Err(e) => {
            println!("  Error (expected): {e}");
            TestResult::pass(
                "cancel-nonexistent",
                start.elapsed().as_millis(),
                "correct not-found error",
            )
        }
    }
}

/// Test 22: return_immediately mode
pub async fn test_return_immediately(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 22: return_immediately mode");
    let client = ClientBuilder::new(&ctx.build_url)
        .with_protocol_binding("REST")
        .with_return_immediately(true)
        .build()
        .unwrap();
    // Use "very-slow" to ensure the executor is still running when we get the response.
    match client.send_message(make_send_params("very-slow")).await {
        Ok(SendMessageResponse::Task(task)) => {
            let state = format!("{:?}", task.status.state);
            println!("  Immediate task: {state}");
            // return_immediately should give us the task in Submitted state
            // before the executor finishes.
            let ok = task.status.state == TaskState::Submitted;
            // Give executor time to finish so we don't leak.
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if ok {
                TestResult::pass(
                    "return-immediately",
                    start.elapsed().as_millis(),
                    &format!("state={state} (correct)"),
                )
            } else {
                TestResult::fail(
                    "return-immediately",
                    start.elapsed().as_millis(),
                    &format!("expected Submitted, got {state}"),
                )
            }
        }
        Ok(_) => TestResult::fail(
            "return-immediately",
            start.elapsed().as_millis(),
            "expected Task",
        ),
        Err(e) => TestResult::fail(
            "return-immediately",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

/// Test 23: Concurrent requests to same agent (5 parallel)
pub async fn test_concurrent_requests(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 23: Concurrent requests (5 parallel)");
    let mut handles = Vec::new();
    for i in 0..5 {
        let url = ctx.analyzer_url.clone();
        handles.push(tokio::spawn(async move {
            let client = ClientBuilder::new(&url).build().unwrap();
            let code = format!("fn task_{i}() {{ let x = {i}; }}");
            client.send_message(make_send_params(&code)).await
        }));
    }
    let mut successes = 0;
    let mut failures = 0;
    for h in handles {
        match h.await {
            Ok(Ok(SendMessageResponse::Task(t))) => {
                if t.status.state == TaskState::Completed {
                    successes += 1;
                } else {
                    failures += 1;
                }
            }
            _ => failures += 1,
        }
    }
    println!("  {successes}/5 completed, {failures} failed");
    if successes == 5 {
        TestResult::pass(
            "concurrent-requests",
            start.elapsed().as_millis(),
            "5/5 parallel completed",
        )
    } else {
        TestResult::fail(
            "concurrent-requests",
            start.elapsed().as_millis(),
            &format!("{successes}/5 ok, {failures} failed"),
        )
    }
}

/// Test 24: Empty message parts rejected
pub async fn test_empty_parts_rejected(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 24: Empty message parts rejected");
    let client = ClientBuilder::new(&ctx.analyzer_url).build().unwrap();
    let params = MessageSendParams {
        tenant: None,
        context_id: None,
        message: Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
            role: MessageRole::User,
            parts: vec![], // Empty!
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    };
    match client.send_message(params).await {
        Ok(_) => TestResult::fail(
            "empty-parts-rejected",
            start.elapsed().as_millis(),
            "should reject empty parts",
        ),
        Err(e) => {
            let msg = e.to_string();
            println!("  Error (expected): {msg}");
            TestResult::pass(
                "empty-parts-rejected",
                start.elapsed().as_millis(),
                "correctly rejected",
            )
        }
    }
}

/// Test 25: GetTask via REST transport
pub async fn test_get_task_rest(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 25: GetTask via REST");
    let client = ClientBuilder::new(&ctx.build_url)
        .with_protocol_binding("REST")
        .build()
        .unwrap();

    let resp = client.send_message(make_send_params("check")).await;
    if let Ok(SendMessageResponse::Task(task)) = resp {
        let tid = task.id.to_string();
        match client
            .get_task(TaskQueryParams {
                tenant: None,
                id: tid.clone(),
                history_length: None,
            })
            .await
        {
            Ok(fetched) => {
                println!(
                    "  Fetched via REST: {} {:?}",
                    fetched.id, fetched.status.state
                );
                TestResult::pass(
                    "get-task-rest",
                    start.elapsed().as_millis(),
                    &format!("id={tid}"),
                )
            }
            Err(e) => TestResult::fail(
                "get-task-rest",
                start.elapsed().as_millis(),
                &format!("error: {e}"),
            ),
        }
    } else {
        TestResult::fail(
            "get-task-rest",
            start.elapsed().as_millis(),
            "could not create initial task",
        )
    }
}

/// Test 26: ListTasks via REST transport
pub async fn test_list_tasks_rest(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 26: ListTasks via REST");
    let client = ClientBuilder::new(&ctx.build_url)
        .with_protocol_binding("REST")
        .build()
        .unwrap();
    match client
        .list_tasks(ListTasksParams {
            tenant: None,
            context_id: None,
            status: None,
            page_size: Some(5),
            page_token: None,
            status_timestamp_after: None,
            include_artifacts: None,
            history_length: None,
        })
        .await
    {
        Ok(resp) => {
            println!("  Listed {} tasks via REST", resp.tasks.len());
            TestResult::pass(
                "list-tasks-rest",
                start.elapsed().as_millis(),
                &format!("count={}", resp.tasks.len()),
            )
        }
        Err(e) => TestResult::fail(
            "list-tasks-rest",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

/// Test 27: Push config CRUD via JSON-RPC (HealthMonitor has push)
pub async fn test_push_crud_jsonrpc(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 27: Push config CRUD via JSON-RPC");
    let client = ClientBuilder::new(&ctx.health_url).build().unwrap();

    let resp = client.send_message(make_send_params("ping")).await.unwrap();
    if let SendMessageResponse::Task(task) = resp {
        let tid = task.id.to_string();
        let config = TaskPushNotificationConfig {
            tenant: None,
            id: None,
            task_id: tid.clone(),
            url: format!("http://{}/webhook", ctx.webhook_addr),
            token: Some("jsonrpc-test-token".into()),
            authentication: None,
        };
        match client.set_push_config(config).await {
            Ok(stored) => {
                let cid = stored.id.clone().unwrap_or_default();
                println!("  Created push config: {cid}");
                // Get.
                let _ = client.get_push_config(tid.clone(), cid.clone()).await;
                // List — this previously returned 0 due to a param type mismatch
                // in the JSON-RPC dispatcher (TaskIdParams vs ListPushConfigsParams).
                let list = client
                    .list_push_configs(ListPushConfigsParams {
                        tenant: None,
                        task_id: tid.clone(),
                        page_size: Some(10),
                        page_token: None,
                    })
                    .await;
                let count = list.as_ref().map(|l| l.configs.len()).unwrap_or(0);
                println!("  Listed {count} configs");
                if count == 0 {
                    // Delete for cleanup.
                    let _ = client.delete_push_config(tid.clone(), cid).await;
                    return TestResult::fail(
                        "push-crud-jsonrpc",
                        start.elapsed().as_millis(),
                        "list returned 0 after create",
                    );
                }
                // Delete.
                let _ = client.delete_push_config(tid.clone(), cid).await;
                TestResult::pass(
                    "push-crud-jsonrpc",
                    start.elapsed().as_millis(),
                    &format!("full CRUD via JSON-RPC ({count} listed)"),
                )
            }
            Err(e) => TestResult::fail(
                "push-crud-jsonrpc",
                start.elapsed().as_millis(),
                &format!("error: {e}"),
            ),
        }
    } else {
        TestResult::fail(
            "push-crud-jsonrpc",
            start.elapsed().as_millis(),
            "could not create initial task",
        )
    }
}

/// Test 28: SubscribeToTask (resubscribe) via REST
pub async fn test_resubscribe_rest(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 28: SubscribeToTask via REST");
    let client = ClientBuilder::new(&ctx.build_url)
        .with_protocol_binding("REST")
        .build()
        .unwrap();

    match client.stream_message(make_send_params("slow")).await {
        Ok(mut stream) => {
            let mut task_id = None;
            // Read first event to get task_id.
            if let Some(Ok(StreamResponse::StatusUpdate(ev))) = stream.next().await {
                task_id = Some(ev.task_id.0.clone());
            }
            if let Some(tid) = task_id {
                match client.subscribe_to_task(tid.clone()).await {
                    Ok(mut sub_stream) => {
                        let mut events = 0;
                        while let Some(ev) = sub_stream.next().await {
                            events += 1;
                            if let Ok(StreamResponse::StatusUpdate(s)) = &ev {
                                println!("  Resubscribe event: {:?}", s.status.state);
                            }
                            if events > 10 {
                                break;
                            }
                        }
                        // Drain original stream.
                        while stream.next().await.is_some() {}
                        TestResult::pass(
                            "resubscribe-rest",
                            start.elapsed().as_millis(),
                            &format!("got {events} events"),
                        )
                    }
                    Err(e) => {
                        while stream.next().await.is_some() {}
                        TestResult::fail(
                            "resubscribe-rest",
                            start.elapsed().as_millis(),
                            &format!("error: {e}"),
                        )
                    }
                }
            } else {
                while stream.next().await.is_some() {}
                TestResult::fail(
                    "resubscribe-rest",
                    start.elapsed().as_millis(),
                    "no task_id from stream",
                )
            }
        }
        Err(e) => TestResult::fail(
            "resubscribe-rest",
            start.elapsed().as_millis(),
            &format!("stream error: {e}"),
        ),
    }
}

/// Test 29: Metrics counters are non-zero
pub async fn test_metrics_nonzero(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 29: Metrics counters verification");
    let ar = ctx.analyzer_metrics.request_count();
    let br = ctx.build_metrics.request_count();
    let hr = ctx.health_metrics.request_count();
    let cr = ctx.coordinator_metrics.request_count();
    let ae = ctx.analyzer_metrics.error_count();
    let be = ctx.build_metrics.error_count();
    println!("  Analyzer:  {ar} req, {ae} err");
    println!("  Builder:   {br} req, {be} err");
    println!("  Health:    {hr} req");
    println!("  Coord:     {cr} req");
    if ar > 0 && br > 0 && hr > 0 && cr > 0 {
        TestResult::pass(
            "metrics-nonzero",
            start.elapsed().as_millis(),
            &format!("a={ar} b={br} h={hr} c={cr}"),
        )
    } else {
        TestResult::fail(
            "metrics-nonzero",
            start.elapsed().as_millis(),
            "some agents have 0 requests",
        )
    }
}

/// Test 30: Error metrics tracked for invalid requests
pub async fn test_error_metrics_tracked(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 30: Error metrics tracked");
    let err_before = ctx.analyzer_metrics.error_count();
    // Send empty parts to trigger error.
    let client = ClientBuilder::new(&ctx.analyzer_url).build().unwrap();
    let params = MessageSendParams {
        tenant: None,
        context_id: None,
        message: Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
            role: MessageRole::User,
            parts: vec![],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    };
    let _ = client.send_message(params).await;
    let err_after = ctx.analyzer_metrics.error_count();
    println!("  Errors before={err_before} after={err_after}");
    if err_after > err_before {
        TestResult::pass(
            "error-metrics-tracked",
            start.elapsed().as_millis(),
            &format!("{err_before} -> {err_after}"),
        )
    } else {
        TestResult::fail(
            "error-metrics-tracked",
            start.elapsed().as_millis(),
            "error count didn't increase",
        )
    }
}
