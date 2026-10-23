// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Spec §4.6.3: "If a client requests a versions of an extension that the
//! agent does not support, the agent SHOULD ignore the extension for that
//! interaction and proceed without it". The response's `A2A-Extensions`
//! header echoed only what the card declares, but the executor's
//! `RequestContext::activated_extensions()` was the raw request list, so an
//! agent was told an undeclared version was active.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_server::{EventEmitter, HttpPushSender, RequestHandlerBuilder};
use a2a_protocol_types::TaskState;
use a2a_protocol_types::error::A2aResult;
use serde_json::json;

use crate::{card, exchange, jsonrpc};

const V1: &str = "https://example.com/ext/a/v1";
const V2: &str = "https://example.com/ext/a/v2";
const UNKNOWN: &str = "https://example.com/ext/unknown/v1";

/// Records what each execution was told was activated.
#[derive(Clone, Default)]
struct Recording(Arc<Mutex<Vec<Vec<String>>>>);

impl Recording {
    fn take(&self) -> Vec<Vec<String>> {
        std::mem::take(&mut *self.0.lock().expect("lock"))
    }
}

impl AgentExecutor for Recording {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.0
                .lock()
                .expect("lock")
                .push(ctx.activated_extensions().to_vec());
            let emit = EventEmitter::new(ctx, queue);
            emit.status(TaskState::Working).await?;
            emit.status(TaskState::Completed).await
        })
    }
}

fn send(id: &str) -> serde_json::Value {
    json!({"message": {"messageId": id, "role": "ROLE_USER", "parts": [{"text": "hi"}]}})
}

/// The card declares `…/a/v1`; the client asks for v1, v2 and an extension
/// nobody declares. The call proceeds, the executor is told only v1, and the
/// response header says the same, on both HTTP bindings.
#[tokio::test]
async fn the_executor_sees_only_the_declared_extensions_the_client_asked_for() {
    let seen = Recording::default();
    let handler = Arc::new(
        RequestHandlerBuilder::new(seen.clone())
            .with_agent_card(card(&[(V1, false)]))
            .with_push_sender(HttpPushSender::new())
            .build()
            .expect("handler"),
    );
    let rpc = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(Arc::clone(&handler)))
        .await
        .expect("JSON-RPC");
    let rest = serve_with_addr("127.0.0.1:0", RestDispatcher::new(Arc::clone(&handler)))
        .await
        .expect("HTTP+JSON");
    let asked = format!("{V2}, {UNKNOWN}, {V1}");
    let extra = [("A2A-Extensions", asked.as_str())];

    let answer = jsonrpc(rpc, "SendMessage", &send("m-rpc"), &extra).await;
    assert_eq!(
        answer.json()["result"]["task"]["status"]["state"],
        "TASK_STATE_COMPLETED",
        "the call proceeds: {answer:?}"
    );
    assert_eq!(
        answer.headers.get("a2a-extensions").map(String::as_str),
        Some(V1)
    );

    let answer = exchange(rest, "POST", "/message:send", Some(&send("m-rest")), &extra).await;
    assert_eq!(answer.status, 200, "{answer:?}");
    assert_eq!(
        answer.headers.get("a2a-extensions").map(String::as_str),
        Some(V1)
    );

    assert_eq!(seen.take(), vec![vec![V1.to_owned()], vec![V1.to_owned()]]);
}

/// Only an undeclared version asked for: the call proceeds without it, the
/// executor sees nothing activated and no header is echoed.
#[tokio::test]
async fn an_unsupported_version_alone_is_ignored_for_the_interaction() {
    let seen = Recording::default();
    let handler = Arc::new(
        RequestHandlerBuilder::new(seen.clone())
            .with_agent_card(card(&[(V1, false)]))
            .with_push_sender(HttpPushSender::new())
            .build()
            .expect("handler"),
    );
    let rpc = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(Arc::clone(&handler)))
        .await
        .expect("JSON-RPC");
    let answer = jsonrpc(rpc, "SendMessage", &send("m-1"), &[("A2A-Extensions", V2)]).await;
    assert_eq!(
        answer.json()["result"]["task"]["status"]["state"],
        "TASK_STATE_COMPLETED",
        "{answer:?}"
    );
    assert!(!answer.headers.contains_key("a2a-extensions"), "{answer:?}");
    assert_eq!(seen.take(), vec![Vec::<String>::new()]);
}

/// With no card there is no declaration to narrow against: the request's
/// list reaches the executor as sent.
#[tokio::test]
async fn without_a_card_the_requested_list_is_passed_through() {
    let seen = Recording::default();
    let handler = RequestHandlerBuilder::new(seen.clone())
        .without_agent_card()
        .build()
        .expect("handler");
    let h = crate::headers(&[("A2A-Extensions", &format!("{V2}, {V1}"))]);
    handler
        .on_send_message(
            serde_json::from_value(send("m-1")).expect("params"),
            false,
            Some(&h),
        )
        .await
        .expect("send");
    assert_eq!(seen.take(), vec![vec![V2.to_owned(), V1.to_owned()]]);
}
