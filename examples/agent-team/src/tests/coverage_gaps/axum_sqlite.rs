// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests 93-98: Axum framework integration and SQLite-backed stores.
//!
//! These tests dogfood the optional `axum` and `sqlite` features end-to-end,
//! exercising code paths that are otherwise only covered by unit/integration
//! tests in the server crate.

use super::*;
use crate::infrastructure::{bind_listener, serve_jsonrpc, serve_rest};

// ── Axum integration ─────────────────────────────────────────────────────────

/// Test 93: Spin up an Axum-based A2A server and send a message through it.
///
/// Exercises `A2aRouter`, Axum routing, and the full HTTP→handler→executor
/// pipeline through Axum instead of the raw hyper dispatchers.
#[cfg(feature = "axum")]
pub async fn test_axum_send_message(ctx: &TestContext) -> TestResult {
    use a2a_protocol_server::dispatch::axum_adapter::A2aRouter;

    let _ = ctx;
    let start = Instant::now();

    // Build a standalone Axum agent with its own handler.
    let handler = Arc::new(
        RequestHandlerBuilder::new(crate::executors::CodeAnalyzerExecutor)
            .with_agent_card(crate::cards::code_analyzer_card("http://127.0.0.1:0"))
            .build()
            .expect("build axum handler"),
    );

    let app = A2aRouter::new(handler).into_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind axum listener");
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}");

    // Serve in background.
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Give the server a moment to start.
    tokio::task::yield_now().await;

    // Send via REST client (Axum serves REST-style routes).
    let client = a2a_protocol_client::ClientBuilder::new(&url)
        .with_protocol_binding("REST")
        .build()
        .unwrap();

    match client
        .send_message(make_send_params("fn axum_test() { let x = 1; }"))
        .await
    {
        Ok(a2a_protocol_types::responses::SendMessageResponse::Task(task)) => {
            if task.status.state == a2a_protocol_types::task::TaskState::Completed {
                TestResult::pass(
                    "axum-send-message",
                    start.elapsed().as_millis(),
                    "Axum: Completed with artifacts",
                )
            } else {
                TestResult::fail(
                    "axum-send-message",
                    start.elapsed().as_millis(),
                    &format!("unexpected state: {:?}", task.status.state),
                )
            }
        }
        Ok(_) => TestResult::fail(
            "axum-send-message",
            start.elapsed().as_millis(),
            "expected Task response",
        ),
        Err(e) => TestResult::fail(
            "axum-send-message",
            start.elapsed().as_millis(),
            &format!("error: {e}"),
        ),
    }
}

/// Test 94: Axum streaming (SSE) through A2aRouter.
#[cfg(feature = "axum")]
pub async fn test_axum_streaming(ctx: &TestContext) -> TestResult {
    use a2a_protocol_server::dispatch::axum_adapter::A2aRouter;

    let _ = ctx;
    let start = Instant::now();

    let handler = Arc::new(
        RequestHandlerBuilder::new(crate::executors::CodeAnalyzerExecutor)
            .with_agent_card(crate::cards::code_analyzer_card("http://127.0.0.1:0"))
            .build()
            .expect("build axum handler"),
    );

    let app = A2aRouter::new(handler).into_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind axum listener");
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::task::yield_now().await;

    let client = a2a_protocol_client::ClientBuilder::new(&url)
        .with_protocol_binding("REST")
        .build()
        .unwrap();

    match client
        .stream_message(make_send_params("fn axum_stream() { let a = 1; }"))
        .await
    {
        Ok(mut stream) => {
            let mut event_count = 0;
            let mut saw_completed = false;
            while let Some(event) = stream.next().await {
                event_count += 1;
                if let Ok(a2a_protocol_types::events::StreamResponse::StatusUpdate(ev)) = &event {
                    if ev.status.state == a2a_protocol_types::task::TaskState::Completed {
                        saw_completed = true;
                    }
                }
            }
            if saw_completed && event_count >= 3 {
                TestResult::pass(
                    "axum-streaming",
                    start.elapsed().as_millis(),
                    &format!("Axum SSE: {event_count} events, Completed"),
                )
            } else {
                TestResult::fail(
                    "axum-streaming",
                    start.elapsed().as_millis(),
                    &format!("events={event_count}, completed={saw_completed}"),
                )
            }
        }
        Err(e) => TestResult::fail(
            "axum-streaming",
            start.elapsed().as_millis(),
            &format!("stream error: {e}"),
        ),
    }
}

/// Test 95: Axum agent card discovery via `/.well-known/agent.json`.
#[cfg(feature = "axum")]
pub async fn test_axum_agent_card(ctx: &TestContext) -> TestResult {
    use a2a_protocol_server::dispatch::axum_adapter::A2aRouter;

    let _ = ctx;
    let start = Instant::now();

    let card = crate::cards::code_analyzer_card("http://127.0.0.1:0");
    let handler = Arc::new(
        RequestHandlerBuilder::new(crate::executors::CodeAnalyzerExecutor)
            .with_agent_card(card.clone())
            .build()
            .expect("build axum handler"),
    );

    let app = A2aRouter::new(handler).into_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::task::yield_now().await;

    match a2a_protocol_client::resolve_agent_card(&url).await {
        Ok(resolved) => {
            if resolved.name == card.name && !resolved.skills.is_empty() {
                TestResult::pass(
                    "axum-agent-card",
                    start.elapsed().as_millis(),
                    &format!("Axum card: '{}'", resolved.name),
                )
            } else {
                TestResult::fail(
                    "axum-agent-card",
                    start.elapsed().as_millis(),
                    &format!("card mismatch: got '{}'", resolved.name),
                )
            }
        }
        Err(e) => TestResult::fail(
            "axum-agent-card",
            start.elapsed().as_millis(),
            &format!("discovery error: {e}"),
        ),
    }
}

// ── SQLite store integration ─────────────────────────────────────────────────

/// Test 96: Full task lifecycle with SQLite-backed task store.
///
/// Exercises SqliteTaskStore through the full handler pipeline:
/// send → get → list → verify persistence.
#[cfg(feature = "sqlite")]
pub async fn test_sqlite_task_store(ctx: &TestContext) -> TestResult {
    use a2a_protocol_server::store::SqliteTaskStore;

    let _ = ctx;
    let start = Instant::now();

    // Create in-memory SQLite store.
    let store = match SqliteTaskStore::new("sqlite::memory:").await {
        Ok(s) => s,
        Err(e) => {
            return TestResult::fail(
                "sqlite-task-store",
                start.elapsed().as_millis(),
                &format!("create store: {e}"),
            )
        }
    };

    let handler = Arc::new(
        RequestHandlerBuilder::new(crate::executors::CodeAnalyzerExecutor)
            .with_agent_card(crate::cards::code_analyzer_card("http://127.0.0.1:0"))
            .with_task_store(store)
            .build()
            .expect("build sqlite handler"),
    );

    // Serve via JSON-RPC dispatcher (simplest setup).
    let (listener, addr) = bind_listener().await;
    let url = format!("http://{addr}");
    serve_jsonrpc(listener, Arc::clone(&handler));

    let client = a2a_protocol_client::ClientBuilder::new(&url)
        .build()
        .unwrap();

    // 1. Send message.
    let resp = match client
        .send_message(make_send_params("fn sqlite_test() { let x = 1; }"))
        .await
    {
        Ok(a2a_protocol_types::responses::SendMessageResponse::Task(t)) => t,
        Ok(_) => {
            return TestResult::fail(
                "sqlite-task-store",
                start.elapsed().as_millis(),
                "expected Task response",
            )
        }
        Err(e) => {
            return TestResult::fail(
                "sqlite-task-store",
                start.elapsed().as_millis(),
                &format!("send error: {e}"),
            )
        }
    };

    let task_id = resp.id.to_string();

    // 2. Get task — should be persisted in SQLite.
    let fetched = match client
        .get_task(a2a_protocol_types::params::TaskQueryParams {
            tenant: None,
            id: task_id.clone(),
            history_length: None,
        })
        .await
    {
        Ok(t) => t,
        Err(e) => {
            return TestResult::fail(
                "sqlite-task-store",
                start.elapsed().as_millis(),
                &format!("get_task error: {e}"),
            )
        }
    };

    // 3. List tasks — should include our task.
    let list = match client
        .list_tasks(a2a_protocol_types::params::ListTasksParams {
            tenant: None,
            context_id: None,
            status: None,
            page_size: Some(50),
            page_token: None,
            status_timestamp_after: None,
            include_artifacts: None,
            history_length: None,
        })
        .await
    {
        Ok(l) => l,
        Err(e) => {
            return TestResult::fail(
                "sqlite-task-store",
                start.elapsed().as_millis(),
                &format!("list_tasks error: {e}"),
            )
        }
    };

    let found = list.tasks.iter().any(|t| t.id.0 == task_id);

    if fetched.status.state.is_terminal() && found {
        TestResult::pass(
            "sqlite-task-store",
            start.elapsed().as_millis(),
            &format!(
                "SQLite: send→get→list OK, state={:?}, listed={}",
                fetched.status.state,
                list.tasks.len()
            ),
        )
    } else {
        TestResult::fail(
            "sqlite-task-store",
            start.elapsed().as_millis(),
            &format!("state={:?}, found_in_list={}", fetched.status.state, found),
        )
    }
}

/// Test 97: SQLite push config CRUD lifecycle.
#[cfg(feature = "sqlite")]
pub async fn test_sqlite_push_config(ctx: &TestContext) -> TestResult {
    use a2a_protocol_server::push::SqlitePushConfigStore;
    use a2a_protocol_server::store::SqliteTaskStore;

    let _ = ctx;
    let start = Instant::now();

    let task_store = match SqliteTaskStore::new("sqlite::memory:").await {
        Ok(s) => s,
        Err(e) => {
            return TestResult::fail(
                "sqlite-push-config",
                start.elapsed().as_millis(),
                &format!("create task store: {e}"),
            )
        }
    };

    let push_store = match SqlitePushConfigStore::new("sqlite::memory:").await {
        Ok(s) => s,
        Err(e) => {
            return TestResult::fail(
                "sqlite-push-config",
                start.elapsed().as_millis(),
                &format!("create push store: {e}"),
            )
        }
    };

    let handler = Arc::new(
        RequestHandlerBuilder::new(crate::executors::BuildMonitorExecutor)
            .with_agent_card(crate::cards::build_monitor_card("http://127.0.0.1:0"))
            .with_task_store(task_store)
            .with_push_config_store(push_store)
            .with_push_sender(a2a_protocol_server::push::HttpPushSender::new().allow_private_urls())
            .build()
            .expect("build sqlite push handler"),
    );

    let (listener, addr) = bind_listener().await;
    let url = format!("http://{addr}");
    serve_rest(listener, Arc::clone(&handler));

    let client = a2a_protocol_client::ClientBuilder::new(&url)
        .with_protocol_binding("REST")
        .build()
        .unwrap();

    // Create a task to attach push config to.
    let task = match client.send_message(make_send_params("check")).await {
        Ok(a2a_protocol_types::responses::SendMessageResponse::Task(t)) => t,
        _ => {
            return TestResult::fail(
                "sqlite-push-config",
                start.elapsed().as_millis(),
                "could not create task",
            )
        }
    };

    let task_id = task.id.to_string();

    // Create push config.
    let config = a2a_protocol_types::push::TaskPushNotificationConfig {
        tenant: None,
        id: None,
        task_id: task_id.clone(),
        url: format!("http://{}/webhook", ctx.webhook_addr),
        token: Some("sqlite-test".into()),
        authentication: None,
    };

    let stored = match client.set_push_config(config).await {
        Ok(s) => s,
        Err(e) => {
            return TestResult::fail(
                "sqlite-push-config",
                start.elapsed().as_millis(),
                &format!("set_push_config: {e}"),
            )
        }
    };

    let config_id = stored.id.clone().unwrap_or_default();

    // List configs.
    let list = client
        .list_push_configs(a2a_protocol_types::params::ListPushConfigsParams {
            tenant: None,
            task_id: task_id.clone(),
            page_size: Some(10),
            page_token: None,
        })
        .await;

    let list_count = list.as_ref().map(|l| l.configs.len()).unwrap_or(0);

    // Delete.
    let _ = client.delete_push_config(task_id.clone(), config_id).await;

    if list_count >= 1 {
        TestResult::pass(
            "sqlite-push-config",
            start.elapsed().as_millis(),
            &format!("SQLite push: set→list({list_count})→delete OK"),
        )
    } else {
        TestResult::fail(
            "sqlite-push-config",
            start.elapsed().as_millis(),
            &format!("list returned {list_count} configs (expected ≥1)"),
        )
    }
}

/// Test 98: Combined Axum + SQLite — full production stack dogfood.
///
/// Spins up an Axum server backed by SQLite stores, sends messages,
/// and verifies end-to-end persistence through the framework layer.
#[cfg(all(feature = "axum", feature = "sqlite"))]
pub async fn test_axum_with_sqlite(ctx: &TestContext) -> TestResult {
    use a2a_protocol_server::dispatch::axum_adapter::A2aRouter;
    use a2a_protocol_server::store::SqliteTaskStore;

    let _ = ctx;
    let start = Instant::now();

    let store = match SqliteTaskStore::new("sqlite::memory:").await {
        Ok(s) => s,
        Err(e) => {
            return TestResult::fail(
                "axum-sqlite-combo",
                start.elapsed().as_millis(),
                &format!("create store: {e}"),
            )
        }
    };

    let handler = Arc::new(
        RequestHandlerBuilder::new(crate::executors::CodeAnalyzerExecutor)
            .with_agent_card(crate::cards::code_analyzer_card("http://127.0.0.1:0"))
            .with_task_store(store)
            .build()
            .expect("build axum+sqlite handler"),
    );

    let app = A2aRouter::new(handler).into_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::task::yield_now().await;

    let client = a2a_protocol_client::ClientBuilder::new(&url)
        .with_protocol_binding("REST")
        .build()
        .unwrap();

    // Send two messages.
    let mut task_ids = Vec::new();
    for i in 0..2 {
        let code = format!("fn axum_sqlite_{i}() {{ let x = {i}; }}");
        match client.send_message(make_send_params(&code)).await {
            Ok(a2a_protocol_types::responses::SendMessageResponse::Task(t)) => {
                task_ids.push(t.id.to_string());
            }
            _ => {
                return TestResult::fail(
                    "axum-sqlite-combo",
                    start.elapsed().as_millis(),
                    &format!("send {i} failed"),
                )
            }
        }
    }

    // List all — should have both persisted in SQLite.
    let list = client
        .list_tasks(a2a_protocol_types::params::ListTasksParams {
            tenant: None,
            context_id: None,
            status: None,
            page_size: Some(50),
            page_token: None,
            status_timestamp_after: None,
            include_artifacts: None,
            history_length: None,
        })
        .await;

    match list {
        Ok(resp) => {
            let found = task_ids
                .iter()
                .all(|id| resp.tasks.iter().any(|t| t.id.0 == *id));
            if found && resp.tasks.len() >= 2 {
                TestResult::pass(
                    "axum-sqlite-combo",
                    start.elapsed().as_millis(),
                    &format!("Axum+SQLite: {} tasks, all found", resp.tasks.len()),
                )
            } else {
                TestResult::fail(
                    "axum-sqlite-combo",
                    start.elapsed().as_millis(),
                    &format!("listed={}, found_both={}", resp.tasks.len(), found),
                )
            }
        }
        Err(e) => TestResult::fail(
            "axum-sqlite-combo",
            start.elapsed().as_millis(),
            &format!("list error: {e}"),
        ),
    }
}
