// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Hardening tests for core server components: stores, event queues,
//! interceptors, builder, and concurrency.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_types::error::{A2aError, A2aResult};
use a2a_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_types::message::{Message, MessageId, MessageRole, Part};
use a2a_types::params::{ListTasksParams, MessageSendParams};
use a2a_types::push::TaskPushNotificationConfig;
use a2a_types::responses::SendMessageResponse;
use a2a_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

use a2a_server::builder::RequestHandlerBuilder;
use a2a_server::call_context::CallContext;
use a2a_server::executor::AgentExecutor;
use a2a_server::handler::SendMessageResult;
use a2a_server::interceptor::{ServerInterceptor, ServerInterceptorChain};
use a2a_server::push::{InMemoryPushConfigStore, PushConfigStore, PushSender};
use a2a_server::request_context::RequestContext;
use a2a_server::store::{InMemoryTaskStore, TaskStore, TaskStoreConfig};
use a2a_server::streaming::{EventQueueManager, EventQueueReader, EventQueueWriter};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_task(id: &str, ctx: &str, state: TaskState) -> Task {
    Task {
        id: TaskId::new(id),
        context_id: ContextId::new(ctx),
        status: TaskStatus::new(state),
        history: None,
        artifacts: None,
        metadata: None,
    }
}

fn make_message(text: &str) -> Message {
    Message {
        id: MessageId::new("msg-1"),
        role: MessageRole::User,
        parts: vec![Part::text(text)],
        task_id: None,
        context_id: None,
        reference_task_ids: None,
        extensions: None,
        metadata: None,
    }
}

fn make_send_params(text: &str) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        message: make_message(text),
        configuration: None,
        metadata: None,
    }
}

/// An executor that completes immediately with a Completed status.
struct QuickExecutor;

impl AgentExecutor for QuickExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Completed),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

/// A no-op push sender for testing.
struct MockPushSender;

impl PushSender for MockPushSender {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// 1. InMemoryTaskStore tests
// ═════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn task_store_save_and_get_roundtrip() {
    let store = InMemoryTaskStore::new();
    let task = make_task("t1", "ctx-1", TaskState::Working);

    store.save(task.clone()).await.expect("save");

    let fetched = store
        .get(&TaskId::new("t1"))
        .await
        .expect("get")
        .expect("should be Some");
    assert_eq!(fetched.id, TaskId::new("t1"));
    assert_eq!(fetched.context_id, ContextId::new("ctx-1"));
    assert_eq!(fetched.status.state, TaskState::Working);
}

#[tokio::test]
async fn task_store_get_returns_none_for_missing_task() {
    let store = InMemoryTaskStore::new();

    let result = store
        .get(&TaskId::new("nonexistent"))
        .await
        .expect("get should not error");
    assert!(result.is_none(), "expected None for missing task");
}

#[tokio::test]
async fn task_store_list_with_context_id_filter() {
    let store = InMemoryTaskStore::new();

    store
        .save(make_task("t1", "ctx-a", TaskState::Working))
        .await
        .unwrap();
    store
        .save(make_task("t2", "ctx-b", TaskState::Working))
        .await
        .unwrap();
    store
        .save(make_task("t3", "ctx-a", TaskState::Completed))
        .await
        .unwrap();

    let params = ListTasksParams {
        tenant: None,
        context_id: Some("ctx-a".into()),
        status: None,
        page_size: None,
        page_token: None,
        status_timestamp_after: None,
        include_artifacts: None,
        history_length: None,
    };
    let result = store.list(&params).await.expect("list");
    assert_eq!(result.tasks.len(), 2, "should return 2 tasks for ctx-a");
    assert!(result.tasks.iter().all(|t| t.context_id.0 == "ctx-a"));
}

#[tokio::test]
async fn task_store_list_with_status_filter() {
    let store = InMemoryTaskStore::new();

    store
        .save(make_task("t1", "ctx", TaskState::Working))
        .await
        .unwrap();
    store
        .save(make_task("t2", "ctx", TaskState::Completed))
        .await
        .unwrap();
    store
        .save(make_task("t3", "ctx", TaskState::Completed))
        .await
        .unwrap();

    let params = ListTasksParams {
        tenant: None,
        context_id: None,
        status: Some(TaskState::Completed),
        page_size: None,
        page_token: None,
        status_timestamp_after: None,
        include_artifacts: None,
        history_length: None,
    };
    let result = store.list(&params).await.expect("list");
    assert_eq!(result.tasks.len(), 2, "should return 2 completed tasks");
    assert!(result
        .tasks
        .iter()
        .all(|t| t.status.state == TaskState::Completed));
}

#[tokio::test]
async fn task_store_list_with_page_size_limit() {
    let store = InMemoryTaskStore::new();

    for i in 0..10 {
        store
            .save(make_task(&format!("t{i:02}"), "ctx", TaskState::Working))
            .await
            .unwrap();
    }

    let params = ListTasksParams {
        tenant: None,
        context_id: None,
        status: None,
        page_size: Some(3),
        page_token: None,
        status_timestamp_after: None,
        include_artifacts: None,
        history_length: None,
    };
    let result = store.list(&params).await.expect("list");
    assert_eq!(
        result.tasks.len(),
        3,
        "should return at most page_size tasks"
    );
}

#[tokio::test]
async fn task_store_delete_removes_task() {
    let store = InMemoryTaskStore::new();

    store
        .save(make_task("t1", "ctx", TaskState::Working))
        .await
        .unwrap();

    // Confirm present.
    assert!(store.get(&TaskId::new("t1")).await.unwrap().is_some());

    // Delete.
    store.delete(&TaskId::new("t1")).await.expect("delete");

    // Confirm gone.
    assert!(
        store.get(&TaskId::new("t1")).await.unwrap().is_none(),
        "task should be deleted"
    );
}

#[tokio::test]
async fn task_store_ttl_eviction_removes_expired_terminal_tasks() {
    let config = TaskStoreConfig {
        max_capacity: None,
        task_ttl: Some(Duration::from_millis(50)),
    };
    let store = InMemoryTaskStore::with_config(config);

    // Save a completed task.
    store
        .save(make_task("old", "ctx", TaskState::Completed))
        .await
        .unwrap();

    // Wait for the TTL to expire.
    tokio::time::sleep(Duration::from_millis(80)).await;

    // Trigger eviction by saving another task (eviction runs on write).
    store
        .save(make_task("new", "ctx", TaskState::Working))
        .await
        .unwrap();

    // The old completed task should be evicted.
    let old = store.get(&TaskId::new("old")).await.unwrap();
    assert!(
        old.is_none(),
        "expired terminal task should be evicted after TTL"
    );

    // The new working task should still be present.
    let new = store.get(&TaskId::new("new")).await.unwrap();
    assert!(new.is_some(), "non-terminal task should survive TTL check");
}

#[tokio::test]
async fn task_store_ttl_eviction_spares_non_terminal_tasks() {
    let config = TaskStoreConfig {
        max_capacity: None,
        task_ttl: Some(Duration::from_millis(50)),
    };
    let store = InMemoryTaskStore::with_config(config);

    // Save a working (non-terminal) task.
    store
        .save(make_task("working", "ctx", TaskState::Working))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(80)).await;

    // Trigger eviction.
    store
        .save(make_task("trigger", "ctx", TaskState::Working))
        .await
        .unwrap();

    // Working task should survive since TTL only applies to terminal tasks.
    let working = store.get(&TaskId::new("working")).await.unwrap();
    assert!(
        working.is_some(),
        "non-terminal task should not be evicted by TTL"
    );
}

#[tokio::test]
async fn task_store_capacity_eviction_removes_oldest_terminal_tasks() {
    let config = TaskStoreConfig {
        max_capacity: Some(3),
        task_ttl: None,
    };
    let store = InMemoryTaskStore::with_config(config);

    // Fill the store with terminal tasks.
    store
        .save(make_task("t1", "ctx", TaskState::Completed))
        .await
        .unwrap();
    store
        .save(make_task("t2", "ctx", TaskState::Failed))
        .await
        .unwrap();
    store
        .save(make_task("t3", "ctx", TaskState::Completed))
        .await
        .unwrap();

    // Adding a 4th task should trigger capacity eviction of the oldest terminal.
    store
        .save(make_task("t4", "ctx", TaskState::Working))
        .await
        .unwrap();

    // We should have at most 3 tasks now.
    let params = ListTasksParams {
        tenant: None,
        context_id: None,
        status: None,
        page_size: Some(50),
        page_token: None,
        status_timestamp_after: None,
        include_artifacts: None,
        history_length: None,
    };
    let result = store.list(&params).await.unwrap();
    assert!(
        result.tasks.len() <= 3,
        "store should evict to stay within max_capacity, got {} tasks",
        result.tasks.len()
    );

    // The new working task must still be present.
    assert!(store.get(&TaskId::new("t4")).await.unwrap().is_some());
}

// ═════════════════════════════════════════════════════════════════════════════
// 2. InMemoryPushConfigStore tests
// ═════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn push_config_store_crud_lifecycle() {
    let store = InMemoryPushConfigStore::new();

    // Set.
    let config = TaskPushNotificationConfig::new("task-1", "https://example.com/hook");
    let saved = store.set(config).await.expect("set");
    assert!(saved.id.is_some(), "should have an assigned ID");
    let config_id = saved.id.clone().unwrap();

    // Get.
    let fetched = store
        .get("task-1", &config_id)
        .await
        .expect("get")
        .expect("should be Some");
    assert_eq!(fetched.url, "https://example.com/hook");
    assert_eq!(fetched.task_id, "task-1");

    // List.
    let configs = store.list("task-1").await.expect("list");
    assert_eq!(configs.len(), 1);

    // Delete.
    store.delete("task-1", &config_id).await.expect("delete");

    // Verify gone.
    let after_delete = store.get("task-1", &config_id).await.expect("get");
    assert!(after_delete.is_none(), "config should be deleted");

    let list_after = store.list("task-1").await.expect("list");
    assert!(list_after.is_empty(), "list should be empty after delete");
}

#[tokio::test]
async fn push_config_store_get_returns_none_for_missing_config() {
    let store = InMemoryPushConfigStore::new();

    let result = store
        .get("task-missing", "id-missing")
        .await
        .expect("get should not error");
    assert!(result.is_none(), "expected None for missing config");
}

#[tokio::test]
async fn push_config_store_auto_assigns_id_if_not_present() {
    let store = InMemoryPushConfigStore::new();

    let config = TaskPushNotificationConfig::new("task-1", "https://example.com/hook");
    assert!(config.id.is_none(), "new config should not have ID yet");

    let saved = store.set(config).await.expect("set");
    assert!(saved.id.is_some(), "store should auto-assign an ID");
    assert!(
        !saved.id.as_ref().unwrap().is_empty(),
        "assigned ID should be non-empty"
    );
}

#[tokio::test]
async fn push_config_store_preserves_explicit_id() {
    let store = InMemoryPushConfigStore::new();

    let mut config = TaskPushNotificationConfig::new("task-1", "https://example.com/hook");
    config.id = Some("my-custom-id".into());

    let saved = store.set(config).await.expect("set");
    assert_eq!(
        saved.id.as_deref(),
        Some("my-custom-id"),
        "explicit ID should be preserved"
    );

    // Retrieve by the explicit ID.
    let fetched = store
        .get("task-1", "my-custom-id")
        .await
        .expect("get")
        .expect("should find by explicit ID");
    assert_eq!(fetched.url, "https://example.com/hook");
}

// ═════════════════════════════════════════════════════════════════════════════
// 3. EventQueueManager tests
// ═════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn event_queue_get_or_create_returns_writer_and_reader_on_first_call() {
    let manager = EventQueueManager::new();
    let task_id = TaskId::new("eq-1");

    let (writer, reader) = manager.get_or_create(&task_id).await;
    assert!(
        reader.is_some(),
        "first get_or_create should return a reader"
    );
    // Writer should be usable (not null).
    drop(writer);
}

#[tokio::test]
async fn event_queue_get_or_create_returns_existing_writer_no_reader_on_second_call() {
    let manager = EventQueueManager::new();
    let task_id = TaskId::new("eq-2");

    let (_writer1, reader1) = manager.get_or_create(&task_id).await;
    assert!(reader1.is_some());

    let (_writer2, reader2) = manager.get_or_create(&task_id).await;
    assert!(
        reader2.is_none(),
        "second get_or_create should return None for reader"
    );
}

#[tokio::test]
async fn event_queue_destroy_allows_fresh_creation() {
    let manager = EventQueueManager::new();
    let task_id = TaskId::new("eq-3");

    // Create then destroy.
    let (_writer, _reader) = manager.get_or_create(&task_id).await;
    manager.destroy(&task_id).await;

    // Re-create should give a new reader.
    let (_writer2, reader2) = manager.get_or_create(&task_id).await;
    assert!(
        reader2.is_some(),
        "get_or_create after destroy should return a fresh reader"
    );
}

#[tokio::test]
async fn event_queue_write_and_read_events() {
    let manager = EventQueueManager::new();
    let task_id = TaskId::new("eq-4");

    let (writer, reader) = manager.get_or_create(&task_id).await;
    let mut reader = reader.expect("should get reader");

    // Write an event.
    let event = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: task_id.clone(),
        context_id: "ctx".into(),
        status: TaskStatus::new(TaskState::Working),
        metadata: None,
    });
    writer.write(event).await.expect("write");

    // Drop writer and manager reference to close channel.
    drop(writer);
    manager.destroy(&task_id).await;

    // Read the event back.
    let received = reader.read().await.expect("read should return Some");
    let update = received.expect("event should be Ok");
    assert!(
        matches!(update, StreamResponse::StatusUpdate(ref u) if u.status.state == TaskState::Working),
        "should read back the Working status event"
    );

    // Channel is closed after writer is dropped.
    assert!(reader.read().await.is_none(), "channel should be closed");
}

#[tokio::test]
async fn event_queue_writer_close_causes_reader_none() {
    let manager = EventQueueManager::new();
    let task_id = TaskId::new("eq-5");

    let (writer, reader) = manager.get_or_create(&task_id).await;
    let mut reader = reader.expect("reader");

    // Drop the writer without writing anything.
    drop(writer);
    manager.destroy(&task_id).await;

    // Reader should get None immediately.
    assert!(
        reader.read().await.is_none(),
        "reader should return None when writer is dropped without writing"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 4. ServerInterceptorChain tests
// ═════════════════════════════════════════════════════════════════════════════

/// An interceptor that always succeeds and records calls.
struct RecordingInterceptor {
    name: String,
    before_calls: Arc<std::sync::Mutex<Vec<String>>>,
    after_calls: Arc<std::sync::Mutex<Vec<String>>>,
}

impl ServerInterceptor for RecordingInterceptor {
    fn before<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.before_calls.lock().unwrap().push(self.name.clone());
            Ok(())
        })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.after_calls.lock().unwrap().push(self.name.clone());
            Ok(())
        })
    }
}

/// An interceptor that always fails in `before`.
struct FailingInterceptor;

impl ServerInterceptor for FailingInterceptor {
    fn before<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Err(A2aError::internal("interceptor failed")) })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn interceptor_chain_empty_chain_succeeds() {
    let chain = ServerInterceptorChain::new();
    let ctx = CallContext::new("test/method");

    chain
        .run_before(&ctx)
        .await
        .expect("empty chain before should succeed");
    chain
        .run_after(&ctx)
        .await
        .expect("empty chain after should succeed");
}

#[tokio::test]
async fn interceptor_chain_error_stops_chain() {
    let mut chain = ServerInterceptorChain::new();
    let before_calls = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let after_calls = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

    // Add a failing interceptor first.
    chain.push(Arc::new(FailingInterceptor));

    // Add a recording interceptor second (should never be reached).
    chain.push(Arc::new(RecordingInterceptor {
        name: "second".into(),
        before_calls: Arc::clone(&before_calls),
        after_calls: Arc::clone(&after_calls),
    }));

    let ctx = CallContext::new("test/method");
    let result = chain.run_before(&ctx).await;
    assert!(result.is_err(), "chain should propagate interceptor error");

    // The second interceptor should not have been called.
    assert!(
        before_calls.lock().unwrap().is_empty(),
        "second interceptor should not be called when first fails"
    );
}

#[tokio::test]
async fn interceptor_chain_runs_before_in_order_and_after_in_reverse() {
    let mut chain = ServerInterceptorChain::new();
    let before_calls = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let after_calls = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

    chain.push(Arc::new(RecordingInterceptor {
        name: "A".into(),
        before_calls: Arc::clone(&before_calls),
        after_calls: Arc::clone(&after_calls),
    }));
    chain.push(Arc::new(RecordingInterceptor {
        name: "B".into(),
        before_calls: Arc::clone(&before_calls),
        after_calls: Arc::clone(&after_calls),
    }));
    chain.push(Arc::new(RecordingInterceptor {
        name: "C".into(),
        before_calls: Arc::clone(&before_calls),
        after_calls: Arc::clone(&after_calls),
    }));

    let ctx = CallContext::new("test/method");

    chain.run_before(&ctx).await.expect("before");
    chain.run_after(&ctx).await.expect("after");

    let before_order: Vec<String> = before_calls.lock().unwrap().clone();
    let after_order: Vec<String> = after_calls.lock().unwrap().clone();

    assert_eq!(
        before_order,
        vec!["A", "B", "C"],
        "before hooks should run in insertion order"
    );
    assert_eq!(
        after_order,
        vec!["C", "B", "A"],
        "after hooks should run in reverse insertion order"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 5. RequestHandlerBuilder tests
// ═════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn builder_build_with_all_defaults_succeeds() {
    let handler = RequestHandlerBuilder::new(QuickExecutor)
        .build()
        .expect("build with defaults should succeed");

    // Verify it's functional by sending a message.
    let result = handler
        .on_send_message(make_send_params("test"), false)
        .await
        .expect("send message");
    assert!(matches!(result, SendMessageResult::Response(_)));
}

#[tokio::test]
async fn builder_with_task_store_uses_custom_store() {
    let custom_store = InMemoryTaskStore::with_config(TaskStoreConfig {
        max_capacity: Some(5),
        task_ttl: None,
    });

    let handler = RequestHandlerBuilder::new(QuickExecutor)
        .with_task_store(custom_store)
        .build()
        .expect("build with custom store");

    // The handler should work normally.
    let result = handler
        .on_send_message(make_send_params("hello"), false)
        .await
        .expect("send message");
    match result {
        SendMessageResult::Response(SendMessageResponse::Task(task)) => {
            assert_eq!(task.status.state, TaskState::Completed);
        }
        _ => panic!("expected Response(Task)"),
    }
}

#[tokio::test]
async fn builder_with_push_sender_enables_push() {
    let handler = RequestHandlerBuilder::new(QuickExecutor)
        .with_push_sender(MockPushSender)
        .build()
        .expect("build with push sender");

    // Push config operations should succeed (not return PushNotSupported).
    let config = TaskPushNotificationConfig::new("task-1", "https://example.com/hook");
    let saved = handler
        .on_set_push_config(config)
        .await
        .expect("set push config should succeed when push is enabled");
    assert!(saved.id.is_some());
}

#[tokio::test]
async fn builder_without_push_sender_rejects_push_config() {
    let handler = RequestHandlerBuilder::new(QuickExecutor)
        .build()
        .expect("build without push sender");

    let config = TaskPushNotificationConfig::new("task-1", "https://example.com/hook");
    let err = handler.on_set_push_config(config).await.unwrap_err();
    assert!(
        matches!(err, a2a_server::ServerError::PushNotSupported),
        "should reject push config when no push sender is configured"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 6. Concurrency tests
// ═════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn concurrency_multiple_send_message_calls_on_same_handler() {
    let handler = Arc::new(
        RequestHandlerBuilder::new(QuickExecutor)
            .build()
            .expect("build handler"),
    );

    let mut handles = Vec::new();
    for i in 0..10 {
        let h = Arc::clone(&handler);
        handles.push(tokio::spawn(async move {
            h.on_send_message(make_send_params(&format!("msg-{i}")), false)
                .await
        }));
    }

    let mut completed_count = 0;
    for handle in handles {
        let result = handle.await.expect("join");
        match result {
            Ok(SendMessageResult::Response(SendMessageResponse::Task(task))) => {
                assert_eq!(task.status.state, TaskState::Completed);
                completed_count += 1;
            }
            Ok(_) => panic!("expected Response(Task)"),
            Err(e) => panic!("unexpected error: {e:?}"),
        }
    }
    assert_eq!(completed_count, 10, "all 10 messages should complete");
}

#[tokio::test]
async fn concurrency_reads_while_writing_to_task_store() {
    let store = Arc::new(InMemoryTaskStore::new());

    // Pre-populate some tasks.
    for i in 0..5 {
        store
            .save(make_task(&format!("pre-{i}"), "ctx", TaskState::Completed))
            .await
            .unwrap();
    }

    let mut handles = Vec::new();

    // Spawn writers.
    for i in 0..10 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            s.save(make_task(&format!("w-{i}"), "ctx", TaskState::Working))
                .await
                .expect("concurrent save");
        }));
    }

    // Spawn readers concurrently.
    for i in 0..10 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let params = ListTasksParams {
                tenant: None,
                context_id: None,
                status: None,
                page_size: Some(50),
                page_token: None,
                status_timestamp_after: None,
                include_artifacts: None,
                history_length: None,
            };
            let result = s.list(&params).await.expect("concurrent list");
            // Should always return at least the pre-populated tasks.
            assert!(
                !result.tasks.is_empty(),
                "concurrent read {i} should find at least some tasks"
            );
        }));
    }

    for handle in handles {
        handle.await.expect("join");
    }

    // After all writes, verify total count.
    let params = ListTasksParams {
        tenant: None,
        context_id: None,
        status: None,
        page_size: Some(50),
        page_token: None,
        status_timestamp_after: None,
        include_artifacts: None,
        history_length: None,
    };
    let final_result = store.list(&params).await.unwrap();
    assert_eq!(
        final_result.tasks.len(),
        15,
        "should have 5 pre-populated + 10 written tasks"
    );
}
