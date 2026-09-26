// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `ServerInterceptor::on_complete` through the handler, and over a real
//! socket whose client disconnects.
//!
//! The chain's own contract is unit-tested beside it
//! (`src/interceptor/completion_tests.rs`). Here: every handler method
//! reports its outcome, and a call whose client goes away mid-flight is
//! reported as cancelled — both to `on_complete` and in the call's
//! `rpc.server.call` record.
//!
//! The second half also pins a premise N26, N27 and N28 rest on: hyper drops
//! a request's future when the client's connection closes. Each of those was
//! a request future dropped at an await its author did not treat as a
//! stopping point; this is the test that says the drop really happens, per
//! HTTP binding, rather than leaving it assumed in their write-ups.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::AsyncWriteExt;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::interceptor::{CallOutcome, ServerInterceptor};
use a2a_protocol_server::metrics::{Metrics, RpcCall};
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_server::{CallContext, RequestHandler};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::message::Message;
use a2a_protocol_types::params::{MessageSendParams, TaskQueryParams};

type Log = Arc<Mutex<Vec<(String, String)>>>;

/// Records `(method, outcome)` for every call it is told about.
struct Outcomes(Log);

impl ServerInterceptor for Outcomes {
    fn before<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn on_complete<'a>(
        &'a self,
        ctx: &'a CallContext,
        outcome: CallOutcome<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        let outcome = match outcome {
            CallOutcome::Succeeded => "succeeded".to_owned(),
            CallOutcome::Failed(e) => format!("failed:{}", e.metric_label()),
            CallOutcome::Cancelled => "cancelled".to_owned(),
            _ => "unknown".to_owned(),
        };
        Box::pin(async move {
            self.0
                .lock()
                .unwrap()
                .push((ctx.method().to_owned(), outcome));
        })
    }
}

/// Completes each task at once.
struct Done;

impl AgentExecutor for Done {
    fn execute<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

/// Says it has started, then works until cancelled — long enough that the
/// client leaves first.
struct Slow(Arc<tokio::sync::Notify>);

impl AgentExecutor for Slow {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.0.notify_one();
            tokio::select! {
                () = tokio::time::sleep(Duration::from_secs(30)) => {}
                () = ctx.cancellation_token.cancelled() => {}
            }
            Ok(())
        })
    }
}

fn params(id: &str) -> MessageSendParams {
    MessageSendParams::new(Message::user_text(id, "hi"))
}

fn from_json<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> T {
    serde_json::from_value(v).expect("params")
}

/// Each handler method tells `on_complete` how it ended — one record per
/// call, named for the method.
#[tokio::test]
async fn every_handler_method_reports_its_outcome() {
    let log = Log::default();
    let handler: RequestHandler = RequestHandlerBuilder::new(Done)
        .with_interceptor(Outcomes(Arc::clone(&log)))
        .build()
        .expect("handler");
    let none: Option<&HashMap<String, String>> = None;

    handler
        .on_send_message(params("m-1"), false, none)
        .await
        .expect("send");
    let _ = handler
        .on_get_task(TaskQueryParams::new("missing"), none)
        .await;
    let _ = handler
        .on_cancel_task(from_json(serde_json::json!({"id": "missing"})), none)
        .await;
    handler
        .on_list_tasks(Default::default(), none)
        .await
        .expect("list");
    let _ = handler
        .on_resubscribe(from_json(serde_json::json!({"id": "missing"})), none)
        .await;
    let _ = handler.on_get_extended_agent_card(none).await;

    let seen = log.lock().unwrap().clone();
    let seen: Vec<(&str, &str)> = seen.iter().map(|(m, o)| (m.as_str(), o.as_str())).collect();
    assert_eq!(
        seen,
        [
            ("SendMessage", "succeeded"),
            ("GetTask", "failed:task_not_found"),
            ("CancelTask", "failed:task_not_found"),
            ("ListTasks", "succeeded"),
            ("SubscribeToTask", "failed:task_not_found"),
            ("GetExtendedAgentCard", "failed:protocol"),
        ]
    );
}

#[derive(Default, Clone)]
struct Calls(Arc<Mutex<Vec<Option<String>>>>);

impl Metrics for Calls {
    fn on_rpc_call(&self, call: &RpcCall<'_>) {
        self.0
            .lock()
            .unwrap()
            .push(call.error_type.map(str::to_owned));
    }
}

const JSONRPC_BODY: &str = r#"{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{"messageId":"m-1","role":"ROLE_USER","parts":[{"text":"hi"}]}}}"#;
const REST_BODY: &str =
    r#"{"message":{"messageId":"m-1","role":"ROLE_USER","parts":[{"text":"hi"}]}}"#;

/// Sends a blocking `SendMessage` over a raw socket, closes the socket while
/// the executor is still working, and returns what the metric and
/// `on_complete` were told.
async fn disconnect_mid_call(rest: bool) -> (Vec<Option<String>>, Vec<(String, String)>) {
    let calls = Calls::default();
    let log = Log::default();
    let started = Arc::new(tokio::sync::Notify::new());
    let handler = Arc::new(
        RequestHandlerBuilder::new(Slow(Arc::clone(&started)))
            .with_metrics(calls.clone())
            .with_interceptor(Outcomes(Arc::clone(&log)))
            .build()
            .expect("handler"),
    );
    let addr = if rest {
        serve_with_addr("127.0.0.1:0", RestDispatcher::new(handler))
            .await
            .expect("serve")
    } else {
        serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler))
            .await
            .expect("serve")
    };
    let (path, body) = if rest {
        ("/message:send", REST_BODY)
    } else {
        ("/", JSONRPC_BODY)
    };
    let mut socket = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\n\
         A2A-Version: 1.0\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(request.as_bytes()).await.expect("write");

    // The call is in flight once the executor has started; it then waits
    // 30 s, so the client leaves long before it would answer.
    tokio::time::timeout(Duration::from_secs(10), started.notified())
        .await
        .expect("the executor starts");
    drop(socket);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let metrics = calls.0.lock().unwrap().clone();
        let outcomes = log.lock().unwrap().clone();
        if (!metrics.is_empty() && !outcomes.is_empty()) || tokio::time::Instant::now() >= deadline
        {
            return (metrics, outcomes);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn cancelled_send() -> Vec<(String, String)> {
    vec![("SendMessage".to_owned(), "cancelled".to_owned())]
}

/// JSON-RPC: the call's future is dropped when its client disconnects.
#[tokio::test]
async fn a_jsonrpc_call_whose_client_disconnects_is_cancelled() {
    let (metrics, outcomes) = disconnect_mid_call(false).await;
    assert_eq!(metrics, [Some("cancelled".to_owned())], "{metrics:?}");
    assert_eq!(outcomes, cancelled_send(), "{outcomes:?}");
}

/// HTTP+JSON: the same.
#[tokio::test]
async fn a_rest_call_whose_client_disconnects_is_cancelled() {
    let (metrics, outcomes) = disconnect_mid_call(true).await;
    assert_eq!(metrics, [Some("cancelled".to_owned())], "{metrics:?}");
    assert_eq!(outcomes, cancelled_send(), "{outcomes:?}");
}
