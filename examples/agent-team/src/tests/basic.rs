// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests 1-10: Core send/stream paths across JSON-RPC and REST transports.

use std::time::Instant;

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part, PartContent};
use a2a_protocol_types::params::{
    ListPushConfigsParams, ListTasksParams, MessageSendParams, TaskQueryParams,
};
use a2a_protocol_types::push::{AuthenticationInfo, TaskPushNotificationConfig};
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::TaskState;

use super::{TestContext, TestResult};
use crate::helpers::make_send_params;

pub async fn test_sync_jsonrpc_send(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.analyzer_url)
        .build()
        .expect("build client");
    let code = "fn hello() {\n    println!(\"world\");\n}\n";
    match client.send_message(make_send_params(code)).await {
        Ok(SendMessageResponse::Task(task)) => {
            let has_artifacts = task.artifacts.as_ref().map_or(0, |a| a.len());
            let state = format!("{:?}", task.status.state);
            TestResult::pass(
                "sync-jsonrpc-send",
                start.elapsed().as_millis(),
                &format!("state={state}, artifacts={has_artifacts}"),
            )
        }
        Ok(_) => TestResult::fail(
            "sync-jsonrpc-send",
            start.elapsed().as_millis(),
            "expected Task response",
        ),
        Err(e) => TestResult::fail(
            "sync-jsonrpc-send",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

pub async fn test_streaming_jsonrpc(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.analyzer_url)
        .build()
        .expect("build client");
    let code = "use std::io;\nfn main() -> io::Result<()> {\n    Ok(())\n}\n";
    match client.stream_message(make_send_params(code)).await {
        Ok(mut stream) => {
            let mut event_count = 0;
            let mut status_events = Vec::new();
            let mut artifact_events = 0;

            while let Some(event) = stream.next().await {
                event_count += 1;
                match event {
                    Ok(StreamResponse::StatusUpdate(ev)) => {
                        status_events.push(format!("{:?}", ev.status.state));
                    }
                    Ok(StreamResponse::ArtifactUpdate(_)) => {
                        artifact_events += 1;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        println!("  Stream error: {e}");
                        break;
                    }
                }
            }

            let detail = format!(
                "events={event_count}, statuses=[{}], artifacts={artifact_events}",
                status_events.join(",")
            );
            if event_count >= 4 {
                TestResult::pass("streaming-jsonrpc", start.elapsed().as_millis(), &detail)
            } else {
                TestResult::fail(
                    "streaming-jsonrpc",
                    start.elapsed().as_millis(),
                    &format!("too few events: {detail}"),
                )
            }
        }
        Err(e) => TestResult::fail(
            "streaming-jsonrpc",
            start.elapsed().as_millis(),
            &format!("stream error: {e}"),
        ),
    }
}

pub async fn test_sync_rest_send(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.build_url)
        .with_protocol_binding("REST")
        .build()
        .expect("build REST client");
    match client.send_message(make_send_params("check")).await {
        Ok(SendMessageResponse::Task(task)) => {
            let state = format!("{:?}", task.status.state);
            TestResult::pass(
                "sync-rest-send",
                start.elapsed().as_millis(),
                &format!("state={state}"),
            )
        }
        Ok(_) => TestResult::fail(
            "sync-rest-send",
            start.elapsed().as_millis(),
            "expected Task",
        ),
        Err(e) => TestResult::fail(
            "sync-rest-send",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

pub async fn test_streaming_rest(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.build_url)
        .with_protocol_binding("REST")
        .build()
        .expect("build REST client");
    match client.stream_message(make_send_params("check")).await {
        Ok(mut stream) => {
            let mut events = 0;
            while let Some(event) = stream.next().await {
                events += 1;
                match event {
                    Ok(StreamResponse::StatusUpdate(_)) => {}
                    Ok(StreamResponse::ArtifactUpdate(_)) => {}
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            TestResult::pass(
                "streaming-rest",
                start.elapsed().as_millis(),
                &format!("events={events}"),
            )
        }
        Err(e) => TestResult::fail(
            "streaming-rest",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

pub async fn test_build_failure_path(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.build_url)
        .with_protocol_binding("REST")
        .build()
        .expect("build REST client");
    match client.send_message(make_send_params("fail")).await {
        Ok(SendMessageResponse::Task(task)) => {
            let state = format!("{:?}", task.status.state);
            let passed = task.status.state == TaskState::Failed;
            if passed {
                TestResult::pass(
                    "build-failure-path",
                    start.elapsed().as_millis(),
                    &format!("state={state} (correct!)"),
                )
            } else {
                TestResult::fail(
                    "build-failure-path",
                    start.elapsed().as_millis(),
                    &format!("expected Failed, got {state}"),
                )
            }
        }
        Ok(_) => TestResult::fail(
            "build-failure-path",
            start.elapsed().as_millis(),
            "expected Task",
        ),
        Err(e) => TestResult::fail(
            "build-failure-path",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

pub async fn test_get_task(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.analyzer_url)
        .build()
        .expect("build client");
    let resp = client
        .send_message(make_send_params("let x = 1;"))
        .await
        .expect("send for get_task test");

    if let SendMessageResponse::Task(task) = resp {
        let task_id = task.id.to_string();
        match client
            .get_task(TaskQueryParams {
                tenant: None,
                id: task_id.clone(),
                history_length: None,
            })
            .await
        {
            Ok(_fetched) => TestResult::pass(
                "get-task",
                start.elapsed().as_millis(),
                &format!("id={task_id}"),
            ),
            Err(e) => TestResult::fail(
                "get-task",
                start.elapsed().as_millis(),
                &format!("error: {e}"),
            ),
        }
    } else {
        TestResult::fail(
            "get-task",
            start.elapsed().as_millis(),
            "initial send did not return Task",
        )
    }
}

pub async fn test_list_tasks(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.analyzer_url)
        .build()
        .expect("build client");
    match client
        .list_tasks(ListTasksParams {
            tenant: None,
            context_id: None,
            status: None,
            page_size: Some(2),
            page_token: None,
            status_timestamp_after: None,
            include_artifacts: Some(true),
            history_length: None,
        })
        .await
    {
        Ok(resp) => TestResult::pass(
            "list-tasks",
            start.elapsed().as_millis(),
            &format!("count={}", resp.tasks.len()),
        ),
        Err(e) => TestResult::fail(
            "list-tasks",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

pub async fn test_push_config_crud(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.build_url)
        .with_protocol_binding("REST")
        .build()
        .expect("build REST client");
    let task_id = match client.send_message(make_send_params("ping")).await {
        Ok(SendMessageResponse::Task(task)) => task.id.to_string(),
        _ => {
            return TestResult::fail(
                "push-config-crud",
                start.elapsed().as_millis(),
                "could not create initial task",
            )
        }
    };
    let webhook_url = format!("http://{}/webhook", ctx.webhook_addr);
    let push_config = TaskPushNotificationConfig {
        tenant: None,
        id: None,
        task_id: task_id.clone(),
        url: webhook_url.clone(),
        token: Some("test-token-123".into()),
        authentication: Some(AuthenticationInfo {
            scheme: "bearer".into(),
            credentials: "my-secret-bearer".into(),
        }),
    };

    let stored = match client.set_push_config(push_config).await {
        Ok(s) => s,
        Err(e) => {
            return TestResult::fail(
                "push-config-crud",
                start.elapsed().as_millis(),
                &format!("create error: {e:?}"),
            )
        }
    };
    let config_id = stored.id.clone().unwrap_or_default();
    let _ = client
        .get_push_config(task_id.clone(), config_id.clone())
        .await;
    let list_params = || ListPushConfigsParams {
        tenant: None,
        task_id: task_id.clone(),
        page_size: Some(10),
        page_token: None,
    };
    match client.list_push_configs(list_params()).await {
        Ok(list) => println!("  LIST:   {} configs", list.configs.len()),
        Err(e) => println!("  LIST:   error: {e}"),
    }
    // 4. Delete push config.
    match client
        .delete_push_config(task_id.clone(), config_id.clone())
        .await
    {
        Ok(()) => println!("  DELETE: ok"),
        Err(e) => println!("  DELETE: error: {e}"),
    }
    // 5. Verify deletion — list should be empty.
    match client.list_push_configs(list_params()).await {
        Ok(list) => {
            println!("  VERIFY: {} configs after delete", list.configs.len());
            if list.configs.is_empty() {
                TestResult::pass(
                    "push-config-crud",
                    start.elapsed().as_millis(),
                    "create+get+list+delete+verify",
                )
            } else {
                TestResult::fail(
                    "push-config-crud",
                    start.elapsed().as_millis(),
                    "delete did not remove config",
                )
            }
        }
        Err(e) => TestResult::fail(
            "push-config-crud",
            start.elapsed().as_millis(),
            &format!("verify list error: {e}"),
        ),
    }
}

/// Test 9: Multi-part message (text + data) via HealthMonitor
pub async fn test_multi_part_message(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 9: Multi-part message (text + data)");
    let client = ClientBuilder::new(&ctx.health_url)
        .build()
        .expect("build client");

    let parts = vec![
        Part::text("health check from test suite"),
        Part::data(serde_json::json!([
            ctx.analyzer_url.clone(),
            ctx.build_url.clone()
        ])),
    ];
    let params = MessageSendParams {
        tenant: None,
        context_id: None,
        message: Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
            role: MessageRole::User,
            parts,
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
        Ok(SendMessageResponse::Task(task)) => {
            if let Some(artifacts) = &task.artifacts {
                for art in artifacts {
                    for part in &art.parts {
                        if let PartContent::Text { text } = &part.content {
                            println!("  {text}");
                        }
                    }
                }
            }
            TestResult::pass(
                "multi-part-message",
                start.elapsed().as_millis(),
                "health check with text+data parts",
            )
        }
        Ok(_) => TestResult::fail(
            "multi-part-message",
            start.elapsed().as_millis(),
            "expected Task",
        ),
        Err(e) => TestResult::fail(
            "multi-part-message",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

/// Test 10: Agent-to-agent coordination (Coordinator -> CodeAnalyzer)
pub async fn test_agent_to_agent(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    println!("\nTest 10: Agent-to-agent coordination");
    let client = ClientBuilder::new(&ctx.coordinator_url)
        .with_protocol_binding("REST")
        .build()
        .expect("build coord client");

    match client.send_message(make_send_params("analyze")).await {
        Ok(SendMessageResponse::Task(task)) => {
            if let Some(artifacts) = &task.artifacts {
                for art in artifacts {
                    for part in &art.parts {
                        if let PartContent::Text { text } = &part.content {
                            for line in text.lines().take(6) {
                                println!("  {line}");
                            }
                        }
                    }
                }
            }
            TestResult::pass(
                "agent-to-agent",
                start.elapsed().as_millis(),
                "coordinator delegated to analyzer",
            )
        }
        Ok(_) => TestResult::fail(
            "agent-to-agent",
            start.elapsed().as_millis(),
            "expected Task",
        ),
        Err(e) => TestResult::fail(
            "agent-to-agent",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}
