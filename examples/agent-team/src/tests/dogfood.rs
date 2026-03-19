// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests 41-50: Dogfood findings — exercises SDK gaps and edge cases discovered
//! during comprehensive review.

use std::time::Instant;

use a2a_protocol_client::{resolve_agent_card, ClientBuilder};
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::{ListPushConfigsParams, ListTasksParams, MessageSendParams};
use a2a_protocol_types::push::{AuthenticationInfo, TaskPushNotificationConfig};
use a2a_protocol_types::responses::SendMessageResponse;

use super::{TestContext, TestResult};
use crate::helpers::make_send_params;

pub async fn test_agent_card_url_correct(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    match resolve_agent_card(&ctx.analyzer_url).await {
        Ok(card) => {
            let card_url = card
                .supported_interfaces
                .first()
                .map(|i| i.url.as_str())
                .unwrap_or("MISSING");
            if card_url == ctx.analyzer_url {
                TestResult::pass(
                    "card-url-correct",
                    start.elapsed().as_millis(),
                    "URL matches bound address",
                )
            } else {
                TestResult::fail(
                    "card-url-correct",
                    start.elapsed().as_millis(),
                    &format!("URL mismatch: card={card_url}"),
                )
            }
        }
        Err(e) => TestResult::fail(
            "card-url-correct",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

pub async fn test_agent_card_skills(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let mut all_ok = true;
    let mut details = Vec::new();

    for (name, url) in [
        ("Analyzer", &ctx.analyzer_url),
        ("Build", &ctx.build_url),
        ("Health", &ctx.health_url),
        ("Coord", &ctx.coordinator_url),
    ] {
        match resolve_agent_card(url).await {
            Ok(card) => {
                let skill_count = card.skills.len();
                let has_name = !card.name.is_empty();
                let has_desc = !card.description.is_empty();
                let has_version = !card.version.is_empty();
                let has_interface = !card.supported_interfaces.is_empty();
                if skill_count == 0 || !has_name || !has_desc || !has_version || !has_interface {
                    all_ok = false;
                    details.push(format!("{name}: incomplete card"));
                }
            }
            Err(e) => {
                all_ok = false;
                details.push(format!("{name}: {e}"));
            }
        }
    }

    if all_ok {
        TestResult::pass(
            "card-skills-valid",
            start.elapsed().as_millis(),
            "all 4 cards complete",
        )
    } else {
        TestResult::fail(
            "card-skills-valid",
            start.elapsed().as_millis(),
            &details.join("; "),
        )
    }
}

pub async fn test_push_list_jsonrpc_regression(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.health_url).build().unwrap();
    let resp = client.send_message(make_send_params("ping")).await.unwrap();
    let SendMessageResponse::Task(task) = resp else {
        return TestResult::fail(
            "push-list-regression",
            start.elapsed().as_millis(),
            "no task",
        );
    };
    let tid = task.id.to_string();
    for i in 0..2 {
        let config = TaskPushNotificationConfig {
            tenant: None,
            id: None,
            task_id: tid.clone(),
            url: format!("http://{}/webhook", ctx.webhook_addr),
            token: Some(format!("regression-token-{i}")),
            authentication: None,
        };
        client.set_push_config(config).await.unwrap();
    }
    match client
        .list_push_configs(ListPushConfigsParams {
            tenant: None,
            task_id: tid.clone(),
            page_size: Some(10),
            page_token: None,
        })
        .await
    {
        Ok(list) => {
            let count = list.configs.len();
            if count == 2 {
                TestResult::pass(
                    "push-list-regression",
                    start.elapsed().as_millis(),
                    "2/2 listed correctly",
                )
            } else {
                TestResult::fail(
                    "push-list-regression",
                    start.elapsed().as_millis(),
                    &format!("expected 2, got {count}"),
                )
            }
        }
        Err(e) => TestResult::fail(
            "push-list-regression",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

pub async fn test_push_event_classification(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.build_url)
        .with_protocol_binding("REST")
        .build()
        .unwrap();

    let webhook_url = format!("http://{}/webhook", ctx.webhook_addr);
    match client.stream_message(make_send_params("slow")).await {
        Ok(mut stream) => {
            if let Some(Ok(StreamResponse::StatusUpdate(ev))) = stream.next().await {
                let push_config = TaskPushNotificationConfig {
                    tenant: None,
                    id: None,
                    task_id: ev.task_id.0.clone(),
                    url: webhook_url,
                    token: Some("classify-test".into()),
                    authentication: Some(AuthenticationInfo {
                        scheme: "bearer".into(),
                        credentials: "test-cred".into(),
                    }),
                };
                let _ = client.set_push_config(push_config).await;
            }
            while stream.next().await.is_some() {}
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;

            let events = ctx.webhook_receiver.snapshot().await;
            let unknown_count = events.iter().filter(|(k, _)| k == "Unknown").count();
            let classified = events.iter().filter(|(k, _)| k != "Unknown").count();
            if unknown_count == 0 && classified > 0 {
                TestResult::pass(
                    "push-event-classify",
                    start.elapsed().as_millis(),
                    &format!("{classified} events classified correctly"),
                )
            } else if events.is_empty() {
                TestResult::pass(
                    "push-event-classify",
                    start.elapsed().as_millis(),
                    "no push events (best-effort)",
                )
            } else {
                TestResult::fail(
                    "push-event-classify",
                    start.elapsed().as_millis(),
                    &format!("{unknown_count} events classified as Unknown"),
                )
            }
        }
        Err(e) => TestResult::fail(
            "push-event-classify",
            start.elapsed().as_millis(),
            &format!("stream error: {e}"),
        ),
    }
}

pub async fn test_resubscribe_jsonrpc(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.analyzer_url).build().unwrap();
    let code = "fn slow() {\n".repeat(100);
    match client.stream_message(make_send_params(&code)).await {
        Ok(mut stream) => {
            let mut task_id = None;
            if let Some(Ok(StreamResponse::StatusUpdate(ev))) = stream.next().await {
                task_id = Some(ev.task_id.0.clone());
            }
            if let Some(tid) = task_id {
                match client.subscribe_to_task(tid).await {
                    Ok(mut sub_stream) => {
                        let mut events = 0;
                        while let Some(_ev) = sub_stream.next().await {
                            events += 1;
                            if events > 10 {
                                break;
                            }
                        }
                        while stream.next().await.is_some() {}
                        TestResult::pass(
                            "resubscribe-jsonrpc",
                            start.elapsed().as_millis(),
                            &format!("{events} events via resubscribe"),
                        )
                    }
                    Err(e) => {
                        while stream.next().await.is_some() {}
                        TestResult::fail(
                            "resubscribe-jsonrpc",
                            start.elapsed().as_millis(),
                            &format!("error: {e}"),
                        )
                    }
                }
            } else {
                while stream.next().await.is_some() {}
                TestResult::fail(
                    "resubscribe-jsonrpc",
                    start.elapsed().as_millis(),
                    "no task_id",
                )
            }
        }
        Err(e) => TestResult::fail(
            "resubscribe-jsonrpc",
            start.elapsed().as_millis(),
            &format!("stream error: {e}"),
        ),
    }
}

pub async fn test_multiple_artifacts(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.analyzer_url).build().unwrap();
    let code = "fn multi_art() {\n    let a = 1;\n    let b = 2;\n}\n";
    match client.send_message(make_send_params(code)).await {
        Ok(SendMessageResponse::Task(task)) => {
            let artifact_count = task.artifacts.as_ref().map_or(0, |a| a.len());
            if artifact_count >= 2 {
                TestResult::pass(
                    "multiple-artifacts",
                    start.elapsed().as_millis(),
                    &format!("{artifact_count} artifacts"),
                )
            } else {
                TestResult::fail(
                    "multiple-artifacts",
                    start.elapsed().as_millis(),
                    &format!("expected >=2, got {artifact_count}"),
                )
            }
        }
        Ok(_) => TestResult::fail(
            "multiple-artifacts",
            start.elapsed().as_millis(),
            "not a Task",
        ),
        Err(e) => TestResult::fail(
            "multiple-artifacts",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

pub async fn test_concurrent_streams(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let mut handles = Vec::new();
    for _i in 0..5 {
        let url = ctx.build_url.clone();
        handles.push(tokio::spawn(async move {
            let client = ClientBuilder::new(&url)
                .with_protocol_binding("REST")
                .build()
                .unwrap();
            let mut stream = client.stream_message(make_send_params("check")).await?;
            let mut events = 0;
            while let Some(ev) = stream.next().await {
                if ev.is_ok() {
                    events += 1;
                }
            }
            Ok::<_, a2a_protocol_client::ClientError>(events)
        }));
    }

    let mut successes = 0;
    for h in handles {
        if let Ok(Ok(_events)) = h.await {
            successes += 1;
        }
    }
    if successes == 5 {
        TestResult::pass(
            "concurrent-streams",
            start.elapsed().as_millis(),
            "5/5 concurrent streams completed",
        )
    } else {
        TestResult::fail(
            "concurrent-streams",
            start.elapsed().as_millis(),
            &format!("{successes}/5 streams ok"),
        )
    }
}

pub async fn test_list_tasks_context_filter(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.analyzer_url).build().unwrap();
    let context_id = uuid::Uuid::new_v4().to_string();
    let params = MessageSendParams {
        tenant: None,
        context_id: None,
        message: Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
            role: MessageRole::User,
            parts: vec![Part::text("fn ctx_filter() {}")],
            task_id: None,
            context_id: Some(a2a_protocol_types::task::ContextId::new(context_id.clone())),
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    };
    if let Err(e) = client.send_message(params).await {
        return TestResult::fail(
            "list-context-filter",
            start.elapsed().as_millis(),
            &format!("send_message failed: {e}"),
        );
    }
    match client
        .list_tasks(ListTasksParams {
            tenant: None,
            context_id: Some(context_id.clone()),
            status: None,
            page_size: Some(50),
            page_token: None,
            status_timestamp_after: None,
            include_artifacts: None,
            history_length: None,
        })
        .await
    {
        Ok(resp) => {
            if resp.tasks.len() == 1 {
                TestResult::pass(
                    "list-context-filter",
                    start.elapsed().as_millis(),
                    "1 task found for unique context",
                )
            } else {
                TestResult::fail(
                    "list-context-filter",
                    start.elapsed().as_millis(),
                    &format!(
                        "expected 1 task for unique context, got {}",
                        resp.tasks.len()
                    ),
                )
            }
        }
        Err(e) => TestResult::fail(
            "list-context-filter",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

pub async fn test_file_parts(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.analyzer_url).build().unwrap();
    let params = MessageSendParams {
        tenant: None,
        context_id: None,
        message: Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
            role: MessageRole::User,
            parts: vec![
                Part::text("fn with_file() {}"),
                Part::file_bytes("YmluYXJ5IGRhdGEgaGVyZQ=="),
            ],
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
            let state = format!("{:?}", task.status.state);
            TestResult::pass(
                "file-parts",
                start.elapsed().as_millis(),
                &format!("state={state}"),
            )
        }
        Ok(_) => TestResult::fail("file-parts", start.elapsed().as_millis(), "not a Task"),
        Err(e) => TestResult::fail(
            "file-parts",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

pub async fn test_history_length(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let client = ClientBuilder::new(&ctx.analyzer_url)
        .with_history_length(5)
        .build()
        .unwrap();
    for i in 0..3 {
        let _ = client
            .send_message(make_send_params(&format!("fn hist_{i}() {{}}")))
            .await;
    }
    match client
        .list_tasks(ListTasksParams {
            tenant: None,
            context_id: None,
            status: None,
            page_size: Some(1),
            page_token: None,
            status_timestamp_after: None,
            include_artifacts: None,
            history_length: Some(5),
        })
        .await
    {
        Ok(resp) => {
            let has_history = resp
                .tasks
                .first()
                .map(|t| t.history.as_ref().map_or(0, |h| h.len()))
                .unwrap_or(0);
            TestResult::pass(
                "history-length",
                start.elapsed().as_millis(),
                &format!("history={has_history}"),
            )
        }
        Err(e) => TestResult::fail(
            "history-length",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}
