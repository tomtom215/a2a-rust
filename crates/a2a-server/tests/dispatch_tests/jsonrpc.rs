// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! JSON-RPC dispatcher tests.
//!
//! Covers send message, get task, unknown method, invalid JSON, missing params,
//! extended agent card, list tasks, push config CRUD, streaming SSE,
//! A2A-Version header, and content-type handling.

use super::*;

#[tokio::test]
async fn jsonrpc_send_message_returns_task() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(1),
        "SendMessage",
        serde_json::to_value(make_send_params()).unwrap(),
    );
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcSuccessResponse<SendMessageResponse> =
        serde_json::from_slice(&body).expect("parse response");
    assert_eq!(result.id, Some(serde_json::json!(1)));
    match result.result {
        SendMessageResponse::Task(task) => {
            assert_eq!(task.status.state, TaskState::Completed);
        }
        _ => panic!("expected Task variant"),
    }
}

#[tokio::test]
async fn jsonrpc_get_task_not_found() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(2),
        "GetTask",
        serde_json::json!({"id": "nonexistent"}),
    );
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    // JSON-RPC errors still return HTTP 200.
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcErrorResponse = serde_json::from_slice(&body).expect("parse error");
    assert_eq!(result.id, Some(serde_json::json!(2)));
    // TaskNotFound = -32001
    assert_eq!(result.error.code, -32001, "expected TaskNotFound error code");
}

#[tokio::test]
async fn jsonrpc_unknown_method() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let rpc =
        JsonRpcRequest::with_params(serde_json::json!(3), "UnknownMethod", serde_json::json!({}));
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcErrorResponse = serde_json::from_slice(&body).expect("parse error");
    // MethodNotFound = -32601
    assert_eq!(result.error.code, -32601);
}

#[tokio::test]
async fn jsonrpc_invalid_json() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from("not json at all")))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcErrorResponse = serde_json::from_slice(&body).expect("parse error");
    // ParseError = -32700
    assert_eq!(result.error.code, -32700);
}

#[tokio::test]
async fn jsonrpc_missing_params() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    // GetTask without params.
    let rpc = JsonRpcRequest::new(serde_json::json!(4), "GetTask");
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcErrorResponse = serde_json::from_slice(&body).expect("parse error");
    // InvalidParams = -32602
    assert_eq!(result.error.code, -32602);
}

#[tokio::test]
async fn jsonrpc_get_extended_agent_card() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let rpc = JsonRpcRequest::new(serde_json::json!(5), "GetExtendedAgentCard");
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcSuccessResponse<AgentCard> =
        serde_json::from_slice(&body).expect("parse response");
    assert_eq!(result.result.name, "Test Agent");
}

#[tokio::test]
async fn jsonrpc_list_tasks() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let rpc = JsonRpcRequest::with_params(serde_json::json!(6), "ListTasks", serde_json::json!({}));
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcSuccessResponse<serde_json::Value> =
        serde_json::from_slice(&body).expect("parse response");
    // Should be a valid response with tasks array.
    assert!(result.result.get("tasks").is_some());
}

#[tokio::test]
async fn jsonrpc_push_config_crud() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    // Create push config.
    let config = TaskPushNotificationConfig::new("task-1", "https://example.com/hook");
    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(10),
        "CreateTaskPushNotificationConfig",
        serde_json::to_value(&config).unwrap(),
    );
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcSuccessResponse<TaskPushNotificationConfig> =
        serde_json::from_slice(&body).expect("parse response");
    assert!(result.result.id.is_some());
    let config_id = result.result.id.unwrap();

    // Get push config.
    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(11),
        "GetTaskPushNotificationConfig",
        serde_json::json!({"taskId": "task-1", "id": config_id}),
    );
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcSuccessResponse<TaskPushNotificationConfig> =
        serde_json::from_slice(&body).expect("parse get response");
    assert_eq!(result.result.url, "https://example.com/hook");

    // List push configs.
    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(12),
        "ListTaskPushNotificationConfigs",
        serde_json::json!({"taskId": "task-1"}),
    );
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcSuccessResponse<ListPushConfigsResponse> =
        serde_json::from_slice(&body).expect("parse list response");
    assert_eq!(result.result.configs.len(), 1);

    // Delete push config.
    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(13),
        "DeleteTaskPushNotificationConfig",
        serde_json::json!({"taskId": "task-1", "id": config_id}),
    );
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn jsonrpc_send_streaming_returns_sse() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(20),
        "SendStreamingMessage",
        serde_json::to_value(make_send_params()).unwrap(),
    );
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );
}

#[tokio::test]
async fn jsonrpc_response_has_a2a_version_header() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let rpc_req = a2a_protocol_types::JsonRpcRequest::with_params(
        serde_json::json!(1),
        "SendMessage",
        serde_json::to_value(make_send_params()).unwrap(),
    );
    let body = serde_json::to_vec(&rpc_req).unwrap();
    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("send");
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()
            .get("A2A-Version")
            .and_then(|v| v.to_str().ok()),
        Some("1.0.0"),
    );
}

#[tokio::test]
async fn jsonrpc_rejects_wrong_content_type() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(1),
        "SendMessage",
        serde_json::to_value(make_send_params()).unwrap(),
    );
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "text/plain")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: JsonRpcErrorResponse = serde_json::from_slice(&body).expect("parse error");
    assert_eq!(
        result.error.code, -32700,
        "wrong content type should be ParseError"
    );
}

#[tokio::test]
async fn jsonrpc_accepts_a2a_content_type() {
    let (addr, _handle) = start_jsonrpc_server().await;
    let client = http_client();

    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(1),
        "SendMessage",
        serde_json::to_value(make_send_params()).unwrap(),
    );
    let body = serde_json::to_vec(&rpc).unwrap();

    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/a2a+json")
        .body(Full::new(Bytes::from(body)))
        .unwrap();

    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let result: serde_json::Value = serde_json::from_slice(&body).expect("parse");
    assert!(
        result.get("result").is_some(),
        "a2a+json should be accepted"
    );
}
