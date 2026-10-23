// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The refusals of `SendMessage` (§3.1.1) and `SendStreamingMessage`
//! (§3.1.2), each through the handler's `on_send_message` in both modes,
//! not only through the helper that decides them:
//!
//! - `ContentTypeNotSupportedError` for a part whose `mediaType` the card
//!   does not accept; until 2026-10-23 only `ensure_input_modes_supported`
//!   itself was tested, so dropping its call from `validate_send_params`
//!   failed nothing at the handler. Over JSON-RPC too, unary and streaming.
//! - `UnsupportedOperationError` for a message to a terminal task, and
//!   `TaskNotFoundError` for one naming an unknown task, on the streaming
//!   send (the unary ones are `messaging/tests.rs`).

use std::sync::Arc;

use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::error::ServerError;
use a2a_protocol_server::handler::SendMessageResult;
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{HttpPushSender, RequestHandler, RequestHandlerBuilder};
use a2a_protocol_types::error::ErrorCode;
use a2a_protocol_types::message::{Message, Part};
use a2a_protocol_types::params::{ListTasksParams, MessageSendParams};
use a2a_protocol_types::responses::SendMessageResponse;
use serde_json::json;

use crate::s3_agent::{Scripted, Seen, send};
use crate::{card, first_sse_data, jsonrpc};

const UNDECLARED: &str = "application/x-undeclared";

fn handler() -> Arc<RequestHandler> {
    Arc::new(
        RequestHandlerBuilder::new(Scripted(Arc::new(Seen::default())))
            .with_agent_card(card(&[]))
            .with_push_sender(HttpPushSender::new())
            .build()
            .expect("handler"),
    )
}

fn undeclared_part(id: &str) -> MessageSendParams {
    MessageSendParams::new(Message::user(
        id,
        vec![Part::text("answer").with_media_type(UNDECLARED)],
    ))
}

async fn stored_tasks(handler: &RequestHandler) -> usize {
    handler
        .on_list_tasks(ListTasksParams::default(), None)
        .await
        .expect("list")
        .tasks
        .len()
}

#[tokio::test]
async fn a_part_of_an_undeclared_media_type_is_refused_by_both_sends() {
    let handler = handler();
    for streaming in [false, true] {
        let err = handler
            .on_send_message(undeclared_part("m-1"), streaming, None)
            .await
            .expect_err("an undeclared media type");
        assert_eq!(
            err.to_a2a_error().code,
            ErrorCode::ContentTypeNotSupported,
            "streaming {streaming}: {err:?}"
        );
    }
    assert_eq!(stored_tasks(&handler).await, 0, "refused before a task");

    // The control: the same part, declared, is accepted.
    let declared = MessageSendParams::new(Message::user(
        "m-2",
        vec![Part::text("answer").with_media_type("text/plain")],
    ));
    handler
        .on_send_message(declared, false, None)
        .await
        .expect("a declared media type");
}

#[tokio::test]
async fn a_part_of_an_undeclared_media_type_is_refused_over_json_rpc_on_both_sends() {
    let rpc = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler()))
        .await
        .expect("JSON-RPC");
    let params = json!({"message": {
        "messageId": "m-1", "role": "ROLE_USER",
        "parts": [{"text": "answer", "mediaType": UNDECLARED}]
    }});
    let code = i64::from(ErrorCode::ContentTypeNotSupported.as_i32());
    for method in ["SendMessage", "SendStreamingMessage"] {
        let answer = jsonrpc(rpc, method, &params, &[]).await;
        let body = if answer.body.trim_start().starts_with('{') {
            answer.json()
        } else {
            first_sse_data(&answer.body)
        };
        assert_eq!(
            body["error"]["code"].as_i64(),
            Some(code),
            "{method}: {body}"
        );
    }
}

/// A finished task, made by a unary send the scripted agent completes.
async fn finished(handler: &RequestHandler) -> String {
    match handler
        .on_send_message(send("m-0", "answer", None, None), false, None)
        .await
        .expect("send")
    {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => {
            assert!(t.status.state.is_terminal(), "{t:?}");
            t.id.0
        }
        other => panic!("expected a task, got {other:?}"),
    }
}

#[tokio::test]
async fn a_streaming_send_to_a_terminal_task_is_unsupported_operation() {
    let handler = handler();
    let done = finished(&handler).await;
    let err = handler
        .on_send_message(send("m-1", "answer", None, Some(&done)), true, None)
        .await
        .expect_err("a terminal task takes no message");
    assert!(
        matches!(&err, ServerError::UnsupportedOperation(m) if m.contains("terminal")),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_streaming_send_naming_an_unknown_task_is_task_not_found() {
    let handler = handler();
    let err = handler
        .on_send_message(
            send("m-1", "answer", None, Some("no-such-task")),
            true,
            None,
        )
        .await
        .expect_err("an unknown task");
    assert!(
        matches!(&err, ServerError::TaskNotFound(id) if id.0 == "no-such-task"),
        "{err:?}"
    );
    assert_eq!(stored_tasks(&handler).await, 0, "nothing stored");
}
