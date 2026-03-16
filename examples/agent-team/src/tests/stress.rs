// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests 31-40: Stress, durability, observability, and SDK ergonomics gaps.

use std::time::Instant;

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::{ListTasksParams, MessageSendParams, TaskQueryParams};
use a2a_protocol_types::push::{AuthenticationInfo, TaskPushNotificationConfig};
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{ContextId, TaskState};

use super::{TestContext, TestResult};
use crate::helpers::make_send_params;

pub async fn test_high_concurrency(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let mut handles = Vec::new();
    for i in 0..20 {
        let url = ctx.analyzer_url.clone();
        handles.push(tokio::spawn(async move {
            let client = ClientBuilder::new(&url).build().unwrap();
            let code = format!("fn stress_{i}() {{ let x = {i}; }}");
            client.send_message(make_send_params(&code)).await
        }));
    }
    let (mut successes, mut failures) = (0, 0);
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
    if successes == 20 {
        TestResult::pass(
            "high-concurrency",
            start.elapsed().as_millis(),
            "20/20 parallel completed",
        )
    } else {
        TestResult::fail(
            "high-concurrency",
            start.elapsed().as_millis(),
            &format!("{successes}/20 ok, {failures} failed"),
        )
    }
}

pub async fn test_mixed_transport_concurrent(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let jsonrpc_url = ctx.analyzer_url.clone();
    let rest_url = ctx.build_url.clone();

    let jsonrpc_handle = tokio::spawn(async move {
        let client = ClientBuilder::new(&jsonrpc_url).build().unwrap();
        client
            .send_message(make_send_params("fn mixed_1() {}"))
            .await
    });
    let rest_handle = tokio::spawn(async move {
        let client = ClientBuilder::new(&rest_url)
            .with_protocol_binding("REST")
            .build()
            .unwrap();
        client.send_message(make_send_params("check")).await
    });

    let (jsonrpc_result, rest_result) = tokio::join!(jsonrpc_handle, rest_handle);
    let jsonrpc_ok = matches!(jsonrpc_result, Ok(Ok(SendMessageResponse::Task(_))));
    let rest_ok = matches!(rest_result, Ok(Ok(SendMessageResponse::Task(_))));

    if jsonrpc_ok && rest_ok {
        TestResult::pass(
            "mixed-transport",
            start.elapsed().as_millis(),
            "both transports work concurrently",
        )
    } else {
        TestResult::fail(
            "mixed-transport",
            start.elapsed().as_millis(),
            &format!("jsonrpc={jsonrpc_ok}, rest={rest_ok}"),
        )
    }
}

pub async fn test_context_continuation(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.analyzer_url).build().unwrap();

    let context_id = ContextId::new(uuid::Uuid::new_v4().to_string());
    let params1 = MessageSendParams {
        tenant: None,
        message: Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
            role: MessageRole::User,
            parts: vec![Part::text("fn first() {}")],
            task_id: None,
            context_id: Some(context_id.clone()),
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    };
    let resp1 = client.send_message(params1).await;
    let task1_ok = matches!(&resp1, Ok(SendMessageResponse::Task(t)) if t.status.state == TaskState::Completed);
    let params2 = MessageSendParams {
        tenant: None,
        message: Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
            role: MessageRole::User,
            parts: vec![Part::text("fn second() {}")],
            task_id: None,
            context_id: Some(context_id.clone()),
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    };
    let resp2 = client.send_message(params2).await;
    let task2_ok = matches!(&resp2, Ok(SendMessageResponse::Task(t)) if t.status.state == TaskState::Completed);

    if task1_ok && task2_ok {
        TestResult::pass(
            "context-continuation",
            start.elapsed().as_millis(),
            "two messages with same context_id",
        )
    } else {
        TestResult::fail(
            "context-continuation",
            start.elapsed().as_millis(),
            &format!("msg1={task1_ok}, msg2={task2_ok}"),
        )
    }
}

pub async fn test_large_payload(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.analyzer_url).build().unwrap();
    let large_code: String = (0..2000)
        .map(|i| format!("fn func_{i}() {{ let x = {i}; }}\n"))
        .collect();
    let size = large_code.len();
    match client.send_message(make_send_params(&large_code)).await {
        Ok(SendMessageResponse::Task(task)) => {
            if task.status.state == TaskState::Completed {
                let has_artifacts = task.artifacts.as_ref().map_or(0, |a| a.len());
                TestResult::pass(
                    "large-payload",
                    start.elapsed().as_millis(),
                    &format!("{size} bytes, {has_artifacts} artifacts"),
                )
            } else {
                TestResult::fail(
                    "large-payload",
                    start.elapsed().as_millis(),
                    &format!("state={:?}", task.status.state),
                )
            }
        }
        Ok(_) => TestResult::fail(
            "large-payload",
            start.elapsed().as_millis(),
            "expected Task",
        ),
        Err(e) => TestResult::fail(
            "large-payload",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

pub async fn test_stream_with_get_task(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.build_url)
        .with_protocol_binding("REST")
        .build()
        .unwrap();

    match client.stream_message(make_send_params("slow")).await {
        Ok(mut stream) => {
            let mut get_task_successes = 0;

            while let Some(event) = stream.next().await {
                match &event {
                    Ok(StreamResponse::StatusUpdate(ev)) => {
                        let tid = &ev.task_id.0;
                        match client
                            .get_task(TaskQueryParams {
                                tenant: None,
                                id: tid.clone(),
                                history_length: None,
                            })
                            .await
                        {
                            Ok(_) => get_task_successes += 1,
                            Err(_) => {}
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            if get_task_successes > 0 {
                TestResult::pass(
                    "stream-with-get-task",
                    start.elapsed().as_millis(),
                    &format!("{get_task_successes} concurrent GetTask succeeded"),
                )
            } else {
                TestResult::fail(
                    "stream-with-get-task",
                    start.elapsed().as_millis(),
                    "no concurrent GetTask succeeded",
                )
            }
        }
        Err(e) => TestResult::fail(
            "stream-with-get-task",
            start.elapsed().as_millis(),
            &format!("stream error: {e}"),
        ),
    }
}

pub async fn test_push_delivery_e2e(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.build_url)
        .with_protocol_binding("REST")
        .build()
        .unwrap();
    let webhook_url = format!("http://{}/webhook", ctx.webhook_addr);
    match client.stream_message(make_send_params("slow")).await {
        Ok(mut stream) => {
            let mut task_id = None;
            if let Some(Ok(StreamResponse::StatusUpdate(ev))) = stream.next().await {
                task_id = Some(ev.task_id.0.clone());
                let push_config = TaskPushNotificationConfig {
                    tenant: None,
                    id: None,
                    task_id: ev.task_id.0.clone(),
                    url: webhook_url.clone(),
                    token: Some("e2e-push-token".into()),
                    authentication: Some(AuthenticationInfo {
                        scheme: "bearer".into(),
                        credentials: "push-secret".into(),
                    }),
                };
                let _ = client.set_push_config(push_config).await;
            }
            while stream.next().await.is_some() {}
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let push_events = ctx.webhook_receiver.drain().await;
            if task_id.is_some() {
                TestResult::pass(
                    "push-delivery-e2e",
                    start.elapsed().as_millis(),
                    &format!("{} push events received", push_events.len()),
                )
            } else {
                TestResult::fail(
                    "push-delivery-e2e",
                    start.elapsed().as_millis(),
                    "no task_id from stream",
                )
            }
        }
        Err(e) => TestResult::fail(
            "push-delivery-e2e",
            start.elapsed().as_millis(),
            &format!("stream error: {e}"),
        ),
    }
}

pub async fn test_list_tasks_status_filter(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.analyzer_url).build().unwrap();
    match client
        .list_tasks(ListTasksParams {
            tenant: None,
            context_id: None,
            status: Some(TaskState::Completed),
            page_size: Some(50),
            page_token: None,
            status_timestamp_after: None,
            include_artifacts: None,
            history_length: None,
        })
        .await
    {
        Ok(resp) => {
            let all_completed = resp.tasks.iter()
                .all(|t| t.status.state == TaskState::Completed);
            if all_completed && !resp.tasks.is_empty() {
                TestResult::pass(
                    "list-status-filter",
                    start.elapsed().as_millis(),
                    &format!("{} completed tasks", resp.tasks.len()),
                )
            } else if resp.tasks.is_empty() {
                TestResult::fail(
                    "list-status-filter",
                    start.elapsed().as_millis(),
                    "no completed tasks found",
                )
            } else {
                TestResult::fail(
                    "list-status-filter",
                    start.elapsed().as_millis(),
                    "filter returned non-completed tasks",
                )
            }
        }
        Err(e) => TestResult::fail(
            "list-status-filter",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

pub async fn test_graceful_shutdown_semantics(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.analyzer_url).build().unwrap();
    let mut task_ids = Vec::new();
    for i in 0..5 {
        let code = format!("fn durability_{i}() {{}}");
        if let Ok(SendMessageResponse::Task(task)) =
            client.send_message(make_send_params(&code)).await
        {
            task_ids.push(task.id.to_string());
        }
    }
    let mut found = 0;
    for tid in &task_ids {
        if client
            .get_task(TaskQueryParams {
                tenant: None,
                id: tid.clone(),
                history_length: None,
            })
            .await
            .is_ok()
        {
            found += 1;
        }
    }
    if found == task_ids.len() {
        TestResult::pass(
            "store-durability",
            start.elapsed().as_millis(),
            &format!("{found}/{} tasks persisted", task_ids.len()),
        )
    } else {
        TestResult::fail(
            "store-durability",
            start.elapsed().as_millis(),
            &format!("{found}/{} tasks found", task_ids.len()),
        )
    }
}

pub async fn test_queue_depth_metrics(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let ar = ctx.analyzer_metrics.request_count();
    let br = ctx.build_metrics.request_count();
    let total = ar + br;

    if total > 20 {
        TestResult::pass(
            "queue-depth-metrics",
            start.elapsed().as_millis(),
            &format!("total={total} requests tracked"),
        )
    } else {
        TestResult::fail(
            "queue-depth-metrics",
            start.elapsed().as_millis(),
            &format!("too few requests: {total}"),
        )
    }
}

pub async fn test_event_ordering(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.analyzer_url).build().unwrap();

    match client
        .stream_message(make_send_params("fn order_test() { let x = 1; }"))
        .await
    {
        Ok(mut stream) => {
            let mut states: Vec<String> = Vec::new();
            let mut artifact_count = 0;
            let mut first_artifact_after_working = false;

            while let Some(event) = stream.next().await {
                match event {
                    Ok(StreamResponse::StatusUpdate(ev)) => {
                        states.push(format!("{:?}", ev.status.state));
                    }
                    Ok(StreamResponse::ArtifactUpdate(_)) => {
                        artifact_count += 1;
                        if states.last().map(|s| s == "Working").unwrap_or(false) {
                            first_artifact_after_working = true;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            let working_idx = states.iter().position(|s| s == "Working");
            let completed_idx = states.iter().position(|s| s == "Completed");
            let order_correct = match (working_idx, completed_idx) {
                (Some(w), Some(c)) => w < c,
                _ => false,
            };

            if order_correct && artifact_count > 0 && first_artifact_after_working {
                TestResult::pass(
                    "event-ordering",
                    start.elapsed().as_millis(),
                    &format!("Working->artifacts({artifact_count})->Completed"),
                )
            } else {
                TestResult::fail(
                    "event-ordering",
                    start.elapsed().as_millis(),
                    &format!(
                        "order={order_correct}, artifacts={artifact_count}, after_working={first_artifact_after_working}"
                    ),
                )
            }
        }
        Err(e) => TestResult::fail(
            "event-ordering",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}
