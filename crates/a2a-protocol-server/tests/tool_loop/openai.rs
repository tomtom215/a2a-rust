// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `OpenAiChatModel` against a stand-in `/v1/chat/completions` on loopback:
//! what it sends, what it reads, and how each provider failure is reported.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use a2a_protocol_server::CancellationToken;
use a2a_protocol_server::tool_loop::{
    ChatMessage, ChatRequest, ERROR_BODY_LIMIT, ModelError, OpenAiChatModel, TokenUsage, ToolCall,
    ToolLoop, ToolModel, ToolSet, ToolSpec,
};
use serde_json::{Map, Value, json};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

use super::Status;
use super::raw_http::read_request;

/// A server answering the `n`th connection with `responses[n]`, raw, and
/// keeping every request it read.
struct Mock {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Mock {
    async fn start(responses: Vec<Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        tokio::spawn(async move {
            for response in responses {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let request = read_request(&mut stream).await;
                seen.lock().unwrap().push(request);
                let _ = stream.write_all(&response).await;
                let _ = stream.shutdown().await;
            }
        });
        Self { base, requests }
    }

    /// The JSON body of request `n`.
    fn body(&self, n: usize) -> Value {
        let requests = self.requests.lock().unwrap();
        let raw = &requests[n];
        let body = &raw[raw.find("\r\n\r\n").unwrap() + 4..];
        serde_json::from_str(body).unwrap()
    }

    /// The head of request `n`, as sent.
    fn head(&self, n: usize) -> String {
        let requests = self.requests.lock().unwrap();
        let raw = &requests[n];
        raw[..raw.find("\r\n\r\n").unwrap()].to_owned()
    }
}

fn response(status: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

fn completion(message: &Value) -> Vec<u8> {
    let body = json!({
        "model": "served-model",
        "choices": [{"index": 0, "message": message, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 31, "completion_tokens": 4, "total_tokens": 35}
    });
    response("200 OK", body.to_string().as_bytes())
}

fn model(mock: &Mock) -> OpenAiChatModel {
    OpenAiChatModel::new(&mock.base, "qwen3:1.7b").unwrap()
}

async fn one_turn(
    model: &OpenAiChatModel,
) -> Result<a2a_protocol_server::tool_loop::ModelTurn, ModelError> {
    let messages = [ChatMessage::User("hi".to_owned())];
    model.complete(ChatRequest::new("", &messages, &[])).await
}

/// Two turns through the loop: the second request carries the system
/// preamble, the user turn, the assistant's call with its arguments as a
/// string, and the result under the call's id, beside the catalogue, the
/// extra fields and the key.
#[tokio::test]
async fn the_request_carries_the_conversation_the_catalogue_and_the_extras() {
    let mock = Mock::start(vec![
        completion(&json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_abc",
                "type": "function",
                "function": {"name": "service_status", "arguments": "{\"service\":\"checkout\"}"}
            }]
        })),
        completion(&json!({"role": "assistant", "content": "checkout is healthy"})),
    ])
    .await;
    let mut extra = Map::new();
    extra.insert(
        "chat_template_kwargs".into(),
        json!({"enable_thinking": false}),
    );
    extra.insert("model".into(), json!("not-this-one"));
    let model = model(&mock).with_api_key("sk-test").with_extra_body(extra);
    let tool_loop = ToolLoop::new(model, ToolSet::new().with(Status).unwrap())
        .with_preamble("You check services.");
    let answer = tool_loop
        .run("how is checkout?", &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(answer.text, "checkout is healthy");
    assert_eq!(
        answer.trace,
        [r#"service_status({"service":"checkout"}) -> {"state":"healthy"}"#]
    );

    let head = mock.head(1);
    assert!(
        head.starts_with("POST /v1/chat/completions HTTP/1.1"),
        "{head}"
    );
    let lower = head.to_ascii_lowercase();
    assert!(
        lower.contains("\r\nauthorization: bearer sk-test"),
        "{head}"
    );
    assert!(
        lower.contains("\r\ncontent-type: application/json"),
        "{head}"
    );

    let body = mock.body(1);
    assert_eq!(body["model"], "qwen3:1.7b", "the client's own fields win");
    assert_eq!(
        body["chat_template_kwargs"],
        json!({"enable_thinking": false})
    );
    assert_eq!(
        body["messages"],
        json!([
            {"role": "system", "content": "You check services."},
            {"role": "user", "content": "how is checkout?"},
            {"role": "assistant", "content": null, "tool_calls": [{
                "id": "call_abc",
                "type": "function",
                "function": {"name": "service_status", "arguments": "{\"service\":\"checkout\"}"}
            }]},
            {"role": "tool", "tool_call_id": "call_abc", "content": "{\"state\":\"healthy\"}"}
        ])
    );
    assert_eq!(
        body["tools"],
        json!([{
            "type": "function",
            "function": {
                "name": "service_status",
                "description": "Look up one service's state.",
                "parameters": {
                    "type": "object",
                    "properties": {"service": {"type": "string"}},
                    "required": ["service"]
                }
            }
        }])
    );
}

#[tokio::test]
async fn no_tools_means_no_tools_field_and_no_preamble_no_system_message() {
    let mock = Mock::start(vec![completion(&json!({"content": "hello"}))]).await;
    let turn = one_turn(&model(&mock)).await.unwrap();
    assert_eq!(turn.text, "hello");
    let body = mock.body(0);
    assert!(body.get("tools").is_none(), "{body}");
    assert_eq!(body["messages"], json!([{"role": "user", "content": "hi"}]));
    assert!(
        !mock.head(0).to_ascii_lowercase().contains("authorization"),
        "no key, no header"
    );
}

#[tokio::test]
async fn arguments_are_read_as_a_string_or_as_an_object() {
    let mock = Mock::start(vec![
        completion(&json!({"tool_calls": [{
            "id": "a", "type": "function",
            "function": {"name": "t", "arguments": "{\"x\": 1}"}
        }]})),
        completion(&json!({"tool_calls": [{
            "id": "b", "type": "function",
            "function": {"name": "t", "arguments": {"x": 1}}
        }]})),
    ])
    .await;
    let model = model(&mock);
    let as_string = one_turn(&model).await.unwrap();
    assert_eq!(
        as_string.tool_calls,
        [ToolCall::new("a", "t", "{\"x\": 1}")]
    );
    let as_object = one_turn(&model).await.unwrap();
    assert_eq!(
        as_object.tool_calls,
        [ToolCall::new("b", "t", r#"{"x":1}"#)]
    );
}

#[tokio::test]
async fn usage_and_the_answering_model_are_read() {
    let mock = Mock::start(vec![completion(&json!({"content": "ok"}))]).await;
    let turn = one_turn(&model(&mock)).await.unwrap();
    assert_eq!(turn.usage, Some(TokenUsage::new(Some(31), Some(4))));
    assert_eq!(turn.response_model.as_deref(), Some("served-model"));
}

#[tokio::test]
async fn a_non_2xx_answer_is_the_status_and_at_most_512_bytes_of_body() {
    let long = "x".repeat(4096);
    let mock = Mock::start(vec![
        response(
            "500 Internal Server Error",
            br#"{"error":"Failed to parse tool call arguments as JSON"}"#,
        ),
        response("503 Service Unavailable", long.as_bytes()),
    ])
    .await;
    let model = model(&mock);
    assert_eq!(
        one_turn(&model).await.unwrap_err(),
        ModelError::Status {
            status: 500,
            body: r#"{"error":"Failed to parse tool call arguments as JSON"}"#.to_owned()
        }
    );
    let ModelError::Status { status, body } = one_turn(&model).await.unwrap_err() else {
        panic!("a status error");
    };
    assert_eq!(status, 503);
    assert_eq!(ERROR_BODY_LIMIT, 512);
    assert_eq!(body.len(), 512);
    assert_eq!(body, "x".repeat(512));
}

#[tokio::test]
async fn a_body_over_the_cap_is_refused_and_one_under_it_read() {
    let padding = "y".repeat(2000);
    let big = completion(&json!({"content": padding}));
    let mock = Mock::start(vec![big.clone(), big]).await;
    assert_eq!(
        one_turn(&model(&mock).with_max_response_bytes(1024))
            .await
            .unwrap_err(),
        ModelError::ResponseTooLarge(1024)
    );
    let turn = one_turn(&model(&mock).with_max_response_bytes(64 * 1024))
        .await
        .unwrap();
    assert_eq!(turn.text.len(), 2000);
}

/// A body with no `Content-Length` is capped as it streams, not trusted.
#[tokio::test]
async fn a_chunked_body_over_the_cap_is_refused() {
    let chunk = "z".repeat(1000);
    let mut raw =
        b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n"
            .to_vec();
    for _ in 0..4 {
        raw.extend_from_slice(format!("{:x}\r\n{chunk}\r\n", chunk.len()).as_bytes());
    }
    raw.extend_from_slice(b"0\r\n\r\n");
    let mock = Mock::start(vec![raw]).await;
    assert_eq!(
        one_turn(&model(&mock).with_max_response_bytes(2500))
            .await
            .unwrap_err(),
        ModelError::ResponseTooLarge(2500)
    );
}

#[tokio::test]
async fn a_server_that_never_answers_times_out_at_the_bound() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await;
        drop(stream);
    });
    let model = OpenAiChatModel::new(&base, "m")
        .unwrap()
        .with_timeout(Duration::from_millis(200));
    let started = Instant::now();
    assert_eq!(
        one_turn(&model).await.unwrap_err(),
        ModelError::Timeout(Duration::from_millis(200))
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn an_unreachable_server_is_a_transport_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    drop(listener);
    let err = one_turn(&OpenAiChatModel::new(&base, "m").unwrap())
        .await
        .unwrap_err();
    assert!(matches!(err, ModelError::Transport(_)), "{err:?}");
    assert_eq!(err.error_type(), "transport");
}

#[tokio::test]
async fn an_answer_that_is_not_a_completion_is_an_invalid_response() {
    let mock = Mock::start(vec![response("200 OK", b"<html>busy</html>")]).await;
    let err = one_turn(&model(&mock)).await.unwrap_err();
    assert!(matches!(err, ModelError::InvalidResponse(_)), "{err:?}");
}

/// The specs a caller passes are the ones sent, even outside a loop.
#[tokio::test]
async fn specs_passed_directly_are_sent() {
    let mock = Mock::start(vec![completion(&json!({"content": "ok"}))]).await;
    let messages = [
        ChatMessage::System("extra rule".to_owned()),
        ChatMessage::User("hi".to_owned()),
    ];
    let specs = [ToolSpec::new("t", "does t", json!({"type": "object"}))];
    model(&mock)
        .complete(ChatRequest::new("pre", &messages, &specs))
        .await
        .unwrap();
    let body = mock.body(0);
    assert_eq!(
        body["messages"],
        json!([
            {"role": "system", "content": "pre"},
            {"role": "system", "content": "extra rule"},
            {"role": "user", "content": "hi"}
        ])
    );
    assert_eq!(body["tools"][0]["function"]["name"], "t");
}
