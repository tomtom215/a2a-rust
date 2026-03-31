// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests 11-20: Orchestration, metadata, cancellation, and agent card discovery.

use std::time::Instant;

use a2a_protocol_client::{resolve_agent_card, ClientBuilder};
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part, PartContent};
use a2a_protocol_types::params::{MessageSendParams, TaskQueryParams};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::TaskState;

use super::{TestContext, TestResult};
use crate::helpers::make_send_params;

/// Test 11: Full-check orchestration (Coordinator -> Analyzer + BuildMonitor)
pub async fn test_full_orchestration(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 11: Full-check orchestration (Coordinator -> multiple agents)");
    let client = ClientBuilder::new(&ctx.coordinator_url)
        .with_protocol_binding("REST")
        .build()
        .expect("build coord client");

    match client.send_message(make_send_params("full-check")).await {
        Ok(SendMessageResponse::Task(task)) => {
            if let Some(artifacts) = &task.artifacts {
                for art in artifacts {
                    for part in &art.parts {
                        if let PartContent::Text(text) = &part.content {
                            for line in text.lines().take(10) {
                                println!("  {line}");
                            }
                        }
                    }
                }
            }
            TestResult::pass(
                "full-orchestration",
                start.elapsed().as_millis(),
                "coordinator -> analyzer + build monitor",
            )
        }
        Ok(_) => TestResult::fail(
            "full-orchestration",
            start.elapsed().as_millis(),
            "expected Task",
        ),
        Err(e) => TestResult::fail(
            "full-orchestration",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

/// Test 12: Health orchestration (Coordinator -> HealthMonitor -> all agents)
pub async fn test_health_orchestration(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 12: Health orchestration (Coordinator -> HealthMonitor)");
    let client = ClientBuilder::new(&ctx.coordinator_url)
        .with_protocol_binding("REST")
        .build()
        .expect("build coord client");

    match client.send_message(make_send_params("health")).await {
        Ok(SendMessageResponse::Task(task)) => {
            if let Some(artifacts) = &task.artifacts {
                for art in artifacts {
                    for part in &art.parts {
                        if let PartContent::Text(text) = &part.content {
                            for line in text.lines() {
                                println!("  {line}");
                            }
                        }
                    }
                }
            }
            TestResult::pass(
                "health-orchestration",
                start.elapsed().as_millis(),
                "coordinator -> health monitor -> all agents",
            )
        }
        Ok(_) => TestResult::fail(
            "health-orchestration",
            start.elapsed().as_millis(),
            "expected Task",
        ),
        Err(e) => TestResult::fail(
            "health-orchestration",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

/// Test 13: Message with metadata
pub async fn test_message_metadata(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 13: Message with metadata");
    let client = ClientBuilder::new(&ctx.analyzer_url)
        .build()
        .expect("build client");

    let params = MessageSendParams {
        tenant: None,
        context_id: None,
        message: Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
            role: MessageRole::User,
            parts: vec![Part::text("fn test() {}")],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: Some(serde_json::json!({
            "source": "agent-team-test",
            "priority": "high",
        })),
    };

    match client.send_message(params).await {
        Ok(SendMessageResponse::Task(task)) => {
            println!("  Task with metadata: {:?}", task.status.state);
            TestResult::pass(
                "message-metadata",
                start.elapsed().as_millis(),
                "metadata attached and processed",
            )
        }
        Ok(_) => TestResult::fail(
            "message-metadata",
            start.elapsed().as_millis(),
            "expected Task",
        ),
        Err(e) => TestResult::fail(
            "message-metadata",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

/// Test 14: CancelTask mid-stream (BuildMonitor)
pub async fn test_cancel_task(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 14: CancelTask mid-stream (BuildMonitor)");
    let client = ClientBuilder::new(&ctx.build_url)
        .with_protocol_binding("REST")
        .build()
        .expect("build REST client");

    match client.stream_message(make_send_params("slow")).await {
        Ok(mut stream) => {
            let mut task_id = None;
            let mut events_before_cancel = 0;
            while let Some(event) = stream.next().await {
                events_before_cancel += 1;
                match &event {
                    Ok(StreamResponse::StatusUpdate(ev)) => {
                        task_id = Some(ev.task_id.0.clone());
                        println!("  Status: {:?}", ev.status.state);
                    }
                    Ok(StreamResponse::ArtifactUpdate(ev)) => {
                        task_id = Some(ev.task_id.0.clone());
                        for part in &ev.artifact.parts {
                            if let PartContent::Text(text) = &part.content {
                                println!("  Build: {text}");
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        println!("  Error: {e}");
                        break;
                    }
                }
                // After 2 events, cancel the task.
                if events_before_cancel >= 2 {
                    if let Some(ref tid) = task_id {
                        println!("  Canceling task {tid}...");
                        match client.cancel_task(tid.clone()).await {
                            Ok(task) => {
                                println!("  Cancel result: {:?}", task.status.state);
                            }
                            Err(e) => {
                                println!("  Cancel error: {e}");
                            }
                        }
                    }
                    break;
                }
            }

            // Drain remaining events.
            let mut events_after_cancel = 0;
            while let Some(event) = stream.next().await {
                events_after_cancel += 1;
                if let Ok(StreamResponse::StatusUpdate(ev)) = &event {
                    println!("  Post-cancel status: {:?}", ev.status.state);
                }
            }

            // Verify via GetTask that the task is canceled.
            if let Some(tid) = task_id {
                let query = TaskQueryParams {
                    tenant: None,
                    id: tid.clone(),
                    history_length: None,
                };
                match client.get_task(query).await {
                    Ok(task) => {
                        if task.status.state == TaskState::Canceled {
                            TestResult::pass(
                                "cancel-task",
                                start.elapsed().as_millis(),
                                &format!(
                                    "events_before={events_before_cancel} events_after={events_after_cancel}"
                                ),
                            )
                        } else {
                            TestResult::fail(
                                "cancel-task",
                                start.elapsed().as_millis(),
                                &format!("expected Canceled, got {:?}", task.status.state),
                            )
                        }
                    }
                    Err(e) => TestResult::fail(
                        "cancel-task",
                        start.elapsed().as_millis(),
                        &format!("GetTask error: {e}"),
                    ),
                }
            } else {
                TestResult::fail(
                    "cancel-task",
                    start.elapsed().as_millis(),
                    "no task_id received from stream",
                )
            }
        }
        Err(e) => TestResult::fail(
            "cancel-task",
            start.elapsed().as_millis(),
            &format!("stream error: {e}"),
        ),
    }
}

/// Test 15: GetTask for nonexistent ID returns proper error
pub async fn test_get_nonexistent_task(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 15: GetTask for nonexistent ID");
    let client = ClientBuilder::new(&ctx.analyzer_url).build().unwrap();
    match client
        .get_task(TaskQueryParams {
            tenant: None,
            id: "nonexistent-task-id-12345".to_owned(),
            history_length: None,
        })
        .await
    {
        Ok(_) => TestResult::fail(
            "get-nonexistent-task",
            start.elapsed().as_millis(),
            "should have returned error",
        ),
        Err(e) => {
            let msg = e.to_string();
            println!("  Error (expected): {msg}");
            TestResult::pass(
                "get-nonexistent-task",
                start.elapsed().as_millis(),
                "correct error for missing task",
            )
        }
    }
}

/// Test 16: ListTasks pagination walk (page_size=1, iterate)
pub async fn test_pagination_walk(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 16: ListTasks pagination walk");
    let client = ClientBuilder::new(&ctx.analyzer_url).build().unwrap();

    let mut all_task_ids = Vec::new();
    let mut page_token: Option<String> = None;
    let mut pages = 0;
    loop {
        let resp = client
            .list_tasks(a2a_protocol_types::params::ListTasksParams {
                tenant: None,
                context_id: None,
                status: None,
                page_size: Some(1),
                page_token: page_token.clone(),
                status_timestamp_after: None,
                include_artifacts: None,
                history_length: None,
            })
            .await
            .unwrap();
        pages += 1;
        for t in &resp.tasks {
            all_task_ids.push(t.id.to_string());
        }
        if resp.next_page_token.is_empty() || pages > 50 {
            break;
        }
        page_token = Some(resp.next_page_token);
    }
    println!(
        "  Walked {pages} pages, found {} unique tasks",
        all_task_ids.len()
    );

    let unique: std::collections::HashSet<_> = all_task_ids.iter().collect();
    let no_dupes = unique.len() == all_task_ids.len();
    if no_dupes && all_task_ids.len() > 1 {
        TestResult::pass(
            "pagination-walk",
            start.elapsed().as_millis(),
            &format!("{} tasks, {pages} pages, no dupes", all_task_ids.len()),
        )
    } else if !no_dupes {
        TestResult::fail(
            "pagination-walk",
            start.elapsed().as_millis(),
            &format!(
                "duplicates! {} total, {} unique",
                all_task_ids.len(),
                unique.len()
            ),
        )
    } else {
        TestResult::pass(
            "pagination-walk",
            start.elapsed().as_millis(),
            &format!("{} tasks, {pages} pages", all_task_ids.len()),
        )
    }
}

/// Test 17: Agent card discovery via REST
pub async fn test_agent_card_rest(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 17: Agent card discovery (REST)");
    match resolve_agent_card(&ctx.build_url).await {
        Ok(card) => {
            println!("  Card: {} v{}", card.name, card.version);
            let binding = card
                .supported_interfaces
                .first()
                .map(|i| i.protocol_binding.as_str())
                .unwrap_or("?");
            println!("  Protocol: {binding}");
            let has_streaming = card.capabilities.streaming.unwrap_or(false);
            let has_push = card.capabilities.push_notifications.unwrap_or(false);
            println!("  Streaming={has_streaming}, Push={has_push}");
            TestResult::pass(
                "agent-card-discovery",
                start.elapsed().as_millis(),
                &format!("{} ({binding})", card.name),
            )
        }
        Err(e) => TestResult::fail(
            "agent-card-discovery",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

/// Test 18: Agent card discovery via JSON-RPC endpoint
pub async fn test_agent_card_jsonrpc(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 18: Agent card discovery (JSON-RPC)");
    match resolve_agent_card(&ctx.analyzer_url).await {
        Ok(card) => {
            println!("  Card: {} v{}", card.name, card.version);
            TestResult::pass(
                "agent-card-jsonrpc",
                start.elapsed().as_millis(),
                &card.name.to_string(),
            )
        }
        Err(e) => TestResult::fail(
            "agent-card-jsonrpc",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

/// Test 19: Push config on agent without push support returns error
pub async fn test_push_not_supported(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 19: Push config on no-push agent");
    let client = ClientBuilder::new(&ctx.analyzer_url).build().unwrap();
    let config = TaskPushNotificationConfig {
        tenant: None,
        id: None,
        task_id: "any-task-id".to_owned(),
        url: format!("http://{}/webhook", ctx.webhook_addr),
        token: None,
        authentication: None,
    };
    match client.set_push_config(config).await {
        Ok(_) => TestResult::fail(
            "push-not-supported",
            start.elapsed().as_millis(),
            "should reject — no push sender",
        ),
        Err(e) => {
            println!("  Error (expected): {e}");
            TestResult::pass(
                "push-not-supported",
                start.elapsed().as_millis(),
                "correctly rejected",
            )
        }
    }
}

/// Test 20: Cancel already-completed task returns error
pub async fn test_cancel_completed(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 20: Cancel completed task");
    let client = ClientBuilder::new(&ctx.build_url)
        .with_protocol_binding("REST")
        .build()
        .unwrap();

    let resp = client.send_message(make_send_params("check")).await;
    if let Ok(SendMessageResponse::Task(task)) = resp {
        let tid = task.id.to_string();
        assert_eq!(task.status.state, TaskState::Completed);
        match client.cancel_task(tid.clone()).await {
            Ok(_) => TestResult::fail(
                "cancel-completed",
                start.elapsed().as_millis(),
                "should reject cancelling completed",
            ),
            Err(e) => {
                println!("  Error (expected): {e}");
                TestResult::pass(
                    "cancel-completed",
                    start.elapsed().as_millis(),
                    "correctly rejected",
                )
            }
        }
    } else {
        TestResult::fail(
            "cancel-completed",
            start.elapsed().as_millis(),
            "could not create initial task",
        )
    }
}
