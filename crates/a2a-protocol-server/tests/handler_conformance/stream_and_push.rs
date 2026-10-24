// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Streams and push configs over a task's life, each asserted at the point
//! the spec puts it rather than after the task has finished:
//!
//! - §3.1.2: a stream "MUST provide immediate feedback on progress": the
//!   `WORKING` update reaches the client while the executor is still
//!   running, through the handler and over JSON-RPC's SSE.
//! - §3.1.8: Get "MUST return configuration details including webhook URL
//!   and notification settings": the whole config, token and
//!   authentication included.
//! - §3.1.7: a config "MUST persist until task completion": the terminal
//!   update is delivered to it, and it is still listed afterwards.
//! - proto `SendMessageConfiguration.history_length`: a streaming send's
//!   `Task` events carry at most that many history messages.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::handler::SendMessageResult;
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::streaming::{EventQueueReader, InMemoryQueueReader};
use a2a_protocol_server::{PushSender, RequestHandler, RequestHandlerBuilder};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::params::{GetPushConfigParams, SendMessageConfiguration};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::TaskState;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use serde_json::json;

use crate::DEADLINE;
use crate::s3_agent::{Scripted, Seen, immediately, send};

/// Records every delivery as `(url, event)`; accepts any URL.
#[derive(Clone, Default)]
pub struct Recorder(pub Arc<Mutex<Vec<(String, String)>>>);

impl Recorder {
    pub fn deliveries(&self) -> Vec<(String, String)> {
        self.0.lock().expect("lock").clone()
    }

    /// Waits until `url` has been sent `event` (deadline poll).
    pub async fn wait_for(&self, url: &str, event: &str) {
        let wanted = (url.to_owned(), event.to_owned());
        tokio::time::timeout(DEADLINE, async {
            while !self.deliveries().contains(&wanted) {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{wanted:?} never delivered: {:?}", self.deliveries()));
    }
}

/// A compact name for an event.
pub fn shape(event: &StreamResponse) -> String {
    match event {
        StreamResponse::Task(t) => format!("task:{}", t.status.state),
        StreamResponse::StatusUpdate(u) => format!("status:{}", u.status.state),
        StreamResponse::ArtifactUpdate(a) => format!("artifact:{}", a.artifact.id),
        StreamResponse::Message(_) => "message".to_owned(),
        _ => "other".to_owned(),
    }
}

impl PushSender for Recorder {
    fn send<'a>(
        &'a self,
        url: &'a str,
        event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>> {
        self.0
            .lock()
            .expect("lock")
            .push((url.to_owned(), shape(event)));
        Box::pin(async { Ok(()) })
    }

    fn allows_private_urls(&self) -> bool {
        true
    }
}

pub fn handler(push: Recorder) -> (Arc<RequestHandler>, Arc<Seen>) {
    let seen = Arc::new(Seen::default());
    let handler = RequestHandlerBuilder::new(Scripted(Arc::clone(&seen)))
        .with_agent_card(crate::card(&[]))
        .with_push_sender(push)
        .build()
        .expect("handler");
    (Arc::new(handler), seen)
}

/// The next event of a stream, within [`DEADLINE`].
async fn next(reader: &mut InMemoryQueueReader) -> Option<StreamResponse> {
    tokio::time::timeout(DEADLINE, reader.read())
        .await
        .expect("an event in time")
        .map(|item| item.expect("not an error").event)
}

#[tokio::test]
async fn progress_reaches_the_stream_while_the_executor_still_runs() {
    let (handler, seen) = handler(Recorder::default());
    // Bounded: a server that answered only once the task ended would hang
    // here, since the agent waits for a release sent below.
    let mut reader = match tokio::time::timeout(
        DEADLINE,
        handler.on_send_message(send("m-1", "wait", None, None), true, None),
    )
    .await
    .expect("the stream is returned while the task runs")
    {
        Ok(SendMessageResult::Stream(s)) => s,
        other => panic!("expected a stream, got {other:?}"),
    };
    let first = next(&mut reader).await.map(|e| shape(&e));
    assert_eq!(first.as_deref(), Some("task:TASK_STATE_SUBMITTED"));
    let second = next(&mut reader).await.map(|e| shape(&e));
    assert_eq!(second.as_deref(), Some("status:TASK_STATE_WORKING"));
    // Only now may the agent finish.
    assert_eq!(seen.runs().len(), 1);
    seen.release.notify_one();
    let mut rest = Vec::new();
    while let Some(event) = next(&mut reader).await {
        rest.push(shape(&event));
    }
    assert_eq!(
        rest.last().map(String::as_str),
        Some("status:TASK_STATE_COMPLETED"),
        "{rest:?}"
    );
}

#[tokio::test]
async fn progress_reaches_an_sse_client_while_the_executor_still_runs() {
    let (handler, seen) = handler(Recorder::default());
    let rpc = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler))
        .await
        .expect("JSON-RPC");
    let envelope = json!({"jsonrpc": "2.0", "id": 1, "method": "SendStreamingMessage",
        "params": {"message": {"messageId": "m-1", "role": "ROLE_USER", "parts": [{"text": "wait"}]}}});
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Full<Bytes>>();
    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{rpc}/"))
        .header("content-type", "application/json")
        .header("a2a-version", "1.0")
        .body(Full::new(Bytes::from(envelope.to_string())))
        .expect("request");
    let mut body = tokio::time::timeout(DEADLINE, client.request(req))
        .await
        .expect("the response head while the task runs")
        .expect("response")
        .into_body();
    let mut text = String::new();
    // Read frames until the WORKING update has arrived; the agent is still
    // waiting for its release, so COMPLETED cannot have.
    while !text.contains("TASK_STATE_WORKING") {
        let frame = tokio::time::timeout(DEADLINE, body.frame())
            .await
            .expect("a frame while the task runs")
            .expect("the stream stays open")
            .expect("a frame");
        if let Ok(data) = frame.into_data() {
            text.push_str(&String::from_utf8_lossy(&data));
        }
    }
    assert!(!text.contains("TASK_STATE_COMPLETED"), "{text}");
    seen.release.notify_one();
    while !text.contains("TASK_STATE_COMPLETED") {
        let frame = tokio::time::timeout(DEADLINE, body.frame())
            .await
            .expect("the final frame")
            .expect("the stream stays open until the end")
            .expect("a frame");
        if let Ok(data) = frame.into_data() {
            text.push_str(&String::from_utf8_lossy(&data));
        }
    }
}

#[tokio::test]
async fn a_push_config_reads_back_whole_and_lasts_through_completion() {
    let push = Recorder::default();
    let (handler, seen) = handler(push.clone());
    let task = match handler
        .on_send_message(immediately(send("m-1", "wait", None, None)), false, None)
        .await
        .expect("send")
    {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.0,
        other => panic!("expected a task, got {other:?}"),
    };
    let url = "https://hook.example.com/a2a";
    let asked: TaskPushNotificationConfig = serde_json::from_value(json!({
        "taskId": task, "url": url, "token": "tok-1",
        "authentication": {"scheme": "Bearer", "credentials": "cred-1"}
    }))
    .expect("config");
    let created = handler
        .on_set_push_config(asked, None)
        .await
        .expect("create");
    let id = created.id.clone().expect("an id");

    // §3.1.8: the whole config, notification settings included.
    let fetched = handler
        .on_get_push_config(
            GetPushConfigParams {
                tenant: None,
                task_id: task.clone(),
                id,
            },
            None,
        )
        .await
        .expect("get");
    let fetched = serde_json::to_value(&fetched).expect("json");
    assert_eq!(fetched, serde_json::to_value(&created).expect("json"));
    assert_eq!(fetched["token"], "tok-1", "{fetched}");
    assert_eq!(
        fetched["authentication"]["credentials"], "cred-1",
        "{fetched}"
    );

    // §3.1.7: it lasts until the task completes, so the terminal update is
    // delivered to it, and it is still there afterwards.
    seen.release.notify_one();
    push.wait_for(url, "status:TASK_STATE_COMPLETED").await;
    let listed = handler
        .on_list_push_configs(&task, None, None)
        .await
        .expect("list");
    assert_eq!(
        serde_json::to_value(&listed).expect("json"),
        json!([fetched]),
        "the config outlives the task's completion"
    );
}

#[tokio::test]
async fn a_streaming_send_s_task_events_carry_at_most_history_length_messages() {
    let (handler, _seen) = handler(Recorder::default());
    // A first turn that asks for input, so the task has history.
    let task = match handler
        .on_send_message(send("m-1", "ask", None, None), false, None)
        .await
        .expect("first turn")
    {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => {
            assert_eq!(t.status.state, TaskState::InputRequired);
            t.id.0
        }
        other => panic!("expected a task, got {other:?}"),
    };
    let mut params = send("m-2", "answer", None, Some(&task));
    params.configuration = Some(SendMessageConfiguration {
        accepted_output_modes: Vec::new(),
        task_push_notification_config: None,
        history_length: Some(1),
        return_immediately: None,
    });
    let Ok(SendMessageResult::Stream(mut reader)) =
        handler.on_send_message(params, true, None).await
    else {
        panic!("expected a stream");
    };
    let mut tasks = Vec::new();
    while let Some(event) = next(&mut reader).await {
        if let StreamResponse::Task(t) = event {
            tasks.push(t);
        }
    }
    assert!(!tasks.is_empty(), "the stream begins with the task");
    for t in &tasks {
        let n = t.history.as_ref().map_or(0, Vec::len);
        assert!(n <= 1, "historyLength 1, got {n}: {t:?}");
    }
    // Not vacuous: the stored task has more than one message.
    let stored = handler
        .on_get_task(
            a2a_protocol_types::params::TaskQueryParams::new(&task),
            None,
        )
        .await
        .expect("get");
    assert!(
        stored.history.as_ref().is_some_and(|h| h.len() > 1),
        "{stored:?}"
    );
}
