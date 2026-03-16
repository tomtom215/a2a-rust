// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests for A2aClient method implementations using a mock transport.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use a2a_protocol_client::error::{ClientError, ClientResult};
use a2a_protocol_client::interceptor::{CallInterceptor, ClientRequest, ClientResponse};
use a2a_protocol_client::streaming::EventStream;
use a2a_protocol_client::transport::Transport;
use a2a_protocol_client::ClientBuilder;
use a2a_protocol_types::{
    ListPushConfigsParams, ListTasksParams, Message, MessageId, MessageRole, MessageSendParams,
    Part, SendMessageResponse, TaskPushNotificationConfig, TaskQueryParams, TaskState,
};

// ── Mock transport ──────────────────────────────────────────────────────────

/// A mock transport that returns a pre-configured JSON value for any request.
struct MockTransport {
    response: serde_json::Value,
    call_count: Arc<AtomicUsize>,
    last_method: Arc<std::sync::Mutex<Option<String>>>,
}

impl MockTransport {
    fn new(response: serde_json::Value) -> Self {
        Self {
            response,
            call_count: Arc::new(AtomicUsize::new(0)),
            last_method: Arc::new(std::sync::Mutex::new(None)),
        }
    }
}

impl Transport for MockTransport {
    fn send_request<'a>(
        &'a self,
        method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        *self.last_method.lock().unwrap() = Some(method.to_owned());
        let resp = self.response.clone();
        Box::pin(async move { Ok(resp) })
    }

    fn send_streaming_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        Box::pin(async move {
            Err(ClientError::Transport(
                "streaming not supported in mock".into(),
            ))
        })
    }
}

/// A mock transport that always returns an error.
struct ErrorTransport {
    error_msg: String,
}

impl ErrorTransport {
    fn new(msg: &str) -> Self {
        Self {
            error_msg: msg.to_owned(),
        }
    }
}

impl Transport for ErrorTransport {
    fn send_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        let msg = self.error_msg.clone();
        Box::pin(async move { Err(ClientError::Transport(msg)) })
    }

    fn send_streaming_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        let msg = self.error_msg.clone();
        Box::pin(async move { Err(ClientError::Transport(msg)) })
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn make_client(transport: impl Transport) -> a2a_protocol_client::A2aClient {
    ClientBuilder::new("http://localhost:8080")
        .with_custom_transport(transport)
        .build()
        .expect("build client")
}

fn make_client_with_interceptor(
    transport: impl Transport,
    interceptor: impl CallInterceptor,
) -> a2a_protocol_client::A2aClient {
    ClientBuilder::new("http://localhost:8080")
        .with_custom_transport(transport)
        .with_interceptor(interceptor)
        .build()
        .expect("build client")
}

fn make_send_params(text: &str) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        message: Message {
            id: MessageId::new("msg-1"),
            role: MessageRole::User,
            parts: vec![Part::text(text)],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    }
}

fn raw_task_json(id: &str, state: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "contextId": "ctx-1",
        "status": {
            "state": state,
            "timestamp": "2026-01-01T00:00:00Z"
        }
    })
}

/// Wraps a task in the SendMessageResponse enum format: `{"task": {...}}`.
fn send_message_response_json(id: &str, state: &str) -> serde_json::Value {
    serde_json::json!({
        "task": raw_task_json(id, state)
    })
}

// ── send_message tests ──────────────────────────────────────────────────────

#[tokio::test]
async fn send_message_returns_task_response() {
    let resp = send_message_response_json("task-1", "TASK_STATE_COMPLETED");
    let transport = MockTransport::new(resp);
    let client = make_client(transport);

    let result = client.send_message(make_send_params("hello")).await;
    match result.unwrap() {
        SendMessageResponse::Task(task) => {
            assert_eq!(task.id.0, "task-1");
            assert_eq!(task.status.state, TaskState::Completed);
        }
        other => panic!("expected Task response, got {other:?}"),
    }
}

#[tokio::test]
async fn send_message_calls_correct_method() {
    let resp = send_message_response_json("task-1", "TASK_STATE_WORKING");
    let transport = MockTransport::new(resp);
    let count = Arc::clone(&transport.call_count);
    let method = Arc::clone(&transport.last_method);
    let client = make_client(transport);

    let _ = client.send_message(make_send_params("hi")).await;
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(method.lock().unwrap().as_deref(), Some("SendMessage"));
}

#[tokio::test]
async fn send_message_transport_error_propagates() {
    let client = make_client(ErrorTransport::new("connection refused"));

    let result = client.send_message(make_send_params("hello")).await;
    let err = result.unwrap_err();
    assert!(
        matches!(err, ClientError::Transport(ref msg) if msg.contains("connection refused")),
        "expected Transport error, got {err:?}"
    );
}

#[tokio::test]
async fn send_message_invalid_response_returns_serialization_error() {
    // Return a value that cannot be deserialized as SendMessageResponse.
    let transport = MockTransport::new(serde_json::json!({"invalid": true}));
    let client = make_client(transport);

    let result = client.send_message(make_send_params("hello")).await;
    assert!(
        matches!(result.unwrap_err(), ClientError::Serialization(_)),
        "expected Serialization error"
    );
}

// ── get_task tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn get_task_returns_task() {
    let resp = raw_task_json("task-42", "TASK_STATE_WORKING");
    let transport = MockTransport::new(resp);
    let client = make_client(transport);

    let params = TaskQueryParams {
        tenant: None,
        id: "task-42".into(),
        history_length: None,
    };
    let task = client.get_task(params).await.unwrap();
    assert_eq!(task.id.0, "task-42");
    assert_eq!(task.status.state, TaskState::Working);
}

#[tokio::test]
async fn get_task_calls_correct_method() {
    let transport = MockTransport::new(raw_task_json("t", "TASK_STATE_WORKING"));
    let method = Arc::clone(&transport.last_method);
    let client = make_client(transport);

    let params = TaskQueryParams {
        tenant: None,
        id: "t".into(),
        history_length: None,
    };
    let _ = client.get_task(params).await;
    assert_eq!(method.lock().unwrap().as_deref(), Some("GetTask"));
}

#[tokio::test]
async fn get_task_transport_error_propagates() {
    let client = make_client(ErrorTransport::new("timeout"));

    let params = TaskQueryParams {
        tenant: None,
        id: "t".into(),
        history_length: None,
    };
    let err = client.get_task(params).await.unwrap_err();
    assert!(matches!(err, ClientError::Transport(_)));
}

// ── list_tasks tests ────────────────────────────────────────────────────────

#[tokio::test]
async fn list_tasks_returns_task_list() {
    let resp = serde_json::json!({
        "tasks": [
            raw_task_json("task-1", "TASK_STATE_WORKING"),
            raw_task_json("task-2", "TASK_STATE_COMPLETED"),
        ]
    });
    let transport = MockTransport::new(resp);
    let client = make_client(transport);

    let params = ListTasksParams {
        tenant: None,
        context_id: None,
        status: None,
        page_size: None,
        page_token: None,
        status_timestamp_after: None,
        include_artifacts: None,
        history_length: None,
    };
    let result = client.list_tasks(params).await.unwrap();
    assert_eq!(result.tasks.len(), 2);
}

#[tokio::test]
async fn list_tasks_calls_correct_method() {
    let resp = serde_json::json!({"tasks": []});
    let transport = MockTransport::new(resp);
    let method = Arc::clone(&transport.last_method);
    let client = make_client(transport);

    let params = ListTasksParams {
        tenant: None,
        context_id: None,
        status: None,
        page_size: None,
        page_token: None,
        status_timestamp_after: None,
        include_artifacts: None,
        history_length: None,
    };
    let _ = client.list_tasks(params).await;
    assert_eq!(method.lock().unwrap().as_deref(), Some("ListTasks"));
}

// ── cancel_task tests ───────────────────────────────────────────────────────

#[tokio::test]
async fn cancel_task_returns_canceled_task() {
    let resp = raw_task_json("task-1", "TASK_STATE_CANCELED");
    let transport = MockTransport::new(resp);
    let client = make_client(transport);

    let task = client.cancel_task("task-1").await.unwrap();
    assert_eq!(task.id.0, "task-1");
    assert_eq!(task.status.state, TaskState::Canceled);
}

#[tokio::test]
async fn cancel_task_calls_correct_method() {
    let transport = MockTransport::new(raw_task_json("t", "TASK_STATE_CANCELED"));
    let method = Arc::clone(&transport.last_method);
    let client = make_client(transport);

    let _ = client.cancel_task("t").await;
    assert_eq!(method.lock().unwrap().as_deref(), Some("CancelTask"));
}

#[tokio::test]
async fn cancel_task_accepts_string_and_str() {
    let transport = MockTransport::new(raw_task_json("t", "TASK_STATE_CANCELED"));
    let client = make_client(transport);

    // &str
    let _ = client.cancel_task("t").await;
    // String
    let _ = client.cancel_task(String::from("t")).await;
}

// ── push config tests ───────────────────────────────────────────────────────

#[tokio::test]
async fn set_push_config_returns_stored_config() {
    let resp = serde_json::json!({
        "taskId": "task-1",
        "url": "https://example.com/hook",
        "id": "config-1"
    });
    let transport = MockTransport::new(resp);
    let client = make_client(transport);

    let config = TaskPushNotificationConfig::new("task-1", "https://example.com/hook");
    let result = client.set_push_config(config).await.unwrap();
    assert_eq!(result.task_id, "task-1");
    assert_eq!(result.id.as_deref(), Some("config-1"));
}

#[tokio::test]
async fn set_push_config_calls_correct_method() {
    let resp = serde_json::json!({
        "taskId": "t",
        "url": "https://example.com/hook"
    });
    let transport = MockTransport::new(resp);
    let method = Arc::clone(&transport.last_method);
    let client = make_client(transport);

    let config = TaskPushNotificationConfig::new("t", "https://example.com/hook");
    let _ = client.set_push_config(config).await;
    assert_eq!(
        method.lock().unwrap().as_deref(),
        Some("CreateTaskPushNotificationConfig")
    );
}

#[tokio::test]
async fn get_push_config_returns_config() {
    let resp = serde_json::json!({
        "taskId": "task-1",
        "url": "https://example.com/hook",
        "id": "cfg-1"
    });
    let transport = MockTransport::new(resp);
    let client = make_client(transport);

    let result = client.get_push_config("task-1", "cfg-1").await.unwrap();
    assert_eq!(result.task_id, "task-1");
    assert_eq!(result.url, "https://example.com/hook");
}

#[tokio::test]
async fn get_push_config_calls_correct_method() {
    let resp = serde_json::json!({
        "taskId": "t",
        "url": "https://example.com/hook",
        "id": "c"
    });
    let transport = MockTransport::new(resp);
    let method = Arc::clone(&transport.last_method);
    let client = make_client(transport);

    let _ = client.get_push_config("t", "c").await;
    assert_eq!(
        method.lock().unwrap().as_deref(),
        Some("GetTaskPushNotificationConfig")
    );
}

#[tokio::test]
async fn list_push_configs_returns_list() {
    let resp = serde_json::json!({
        "configs": [
            {"taskId": "t", "url": "https://a.example.com/hook", "id": "c1"},
            {"taskId": "t", "url": "https://b.example.com/hook", "id": "c2"}
        ]
    });
    let transport = MockTransport::new(resp);
    let client = make_client(transport);

    let params = ListPushConfigsParams {
        tenant: None,
        task_id: "t".into(),
        page_size: None,
        page_token: None,
    };
    let result = client.list_push_configs(params).await.unwrap();
    assert_eq!(result.configs.len(), 2);
}

#[tokio::test]
async fn delete_push_config_succeeds() {
    // delete_push_config ignores the response body, just checks for Ok.
    let transport = MockTransport::new(serde_json::json!(null));
    let client = make_client(transport);

    client.delete_push_config("task-1", "cfg-1").await.unwrap();
}

#[tokio::test]
async fn delete_push_config_calls_correct_method() {
    let transport = MockTransport::new(serde_json::json!(null));
    let method = Arc::clone(&transport.last_method);
    let client = make_client(transport);

    let _ = client.delete_push_config("t", "c").await;
    assert_eq!(
        method.lock().unwrap().as_deref(),
        Some("DeleteTaskPushNotificationConfig")
    );
}

// ── Interceptor integration tests ───────────────────────────────────────────

struct HeaderInjectingInterceptor;

impl CallInterceptor for HeaderInjectingInterceptor {
    async fn before(&self, req: &mut ClientRequest) -> ClientResult<()> {
        req.extra_headers
            .insert("x-custom".into(), "test-value".into());
        Ok(())
    }

    async fn after(&self, _resp: &ClientResponse) -> ClientResult<()> {
        Ok(())
    }
}

/// Transport that captures headers for inspection.
struct HeaderCapturingTransport {
    response: serde_json::Value,
    captured_headers: Arc<std::sync::Mutex<HashMap<String, String>>>,
}

impl HeaderCapturingTransport {
    fn new(response: serde_json::Value) -> Self {
        Self {
            response,
            captured_headers: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }
}

impl Transport for HeaderCapturingTransport {
    fn send_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        *self.captured_headers.lock().unwrap() = extra_headers.clone();
        let resp = self.response.clone();
        Box::pin(async move { Ok(resp) })
    }

    fn send_streaming_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        Box::pin(async move { Err(ClientError::Transport("not supported".into())) })
    }
}

#[tokio::test]
async fn interceptor_headers_are_passed_to_transport() {
    let transport = HeaderCapturingTransport::new(raw_task_json("t", "TASK_STATE_WORKING"));
    let headers = Arc::clone(&transport.captured_headers);
    let client = make_client_with_interceptor(transport, HeaderInjectingInterceptor);

    let params = TaskQueryParams {
        tenant: None,
        id: "t".into(),
        history_length: None,
    };
    let _ = client.get_task(params).await;

    let captured = headers.lock().unwrap();
    assert_eq!(
        captured.get("x-custom").map(String::as_str),
        Some("test-value")
    );
}

// ── Error interceptor tests ─────────────────────────────────────────────────

struct FailingBeforeInterceptor;

impl CallInterceptor for FailingBeforeInterceptor {
    async fn before(&self, _req: &mut ClientRequest) -> ClientResult<()> {
        Err(ClientError::Transport("interceptor rejected".into()))
    }

    async fn after(&self, _resp: &ClientResponse) -> ClientResult<()> {
        Ok(())
    }
}

#[tokio::test]
async fn failing_interceptor_prevents_transport_call() {
    let transport = MockTransport::new(raw_task_json("t", "TASK_STATE_WORKING"));
    let count = Arc::clone(&transport.call_count);
    let client = make_client_with_interceptor(transport, FailingBeforeInterceptor);

    let result = client.send_message(make_send_params("hi")).await;
    assert!(result.is_err());
    assert_eq!(
        count.load(Ordering::SeqCst),
        0,
        "transport should not be called"
    );
}
