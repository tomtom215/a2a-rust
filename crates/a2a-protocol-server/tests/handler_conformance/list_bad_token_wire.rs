// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A `pageToken` the server could not have issued, on the wire. Google
//! AIP-158 says an invalid page token is an invalid argument; the stores used
//! to answer an empty page, which a client reads as the end of the listing
//! and so never learns that its cursor was wrong. This pins `InvalidParams`
//! (-32602) on JSON-RPC and HTTP 400 on HTTP+JSON, and that a token the
//! server did issue still pages.

use std::sync::Arc;

use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::serve::Server;
use a2a_protocol_server::{RequestHandlerBuilder, agent_executor};
use serde_json::{Value, json};

use crate::s3_agent::{Scripted, Seen, immediately, send};
use crate::{exchange, jsonrpc};

struct Exec;
agent_executor!(Exec, |_ctx, _q| async { Ok(()) });

#[tokio::test]
async fn a_page_token_the_server_did_not_issue_is_invalid_params_on_both_bindings() {
    let handler = Arc::new(
        RequestHandlerBuilder::new(Exec)
            .without_agent_card()
            .build()
            .expect("handler"),
    );
    let rpc = {
        let bound = Server::bind("127.0.0.1:0").await.expect("JSON-RPC");
        let addr = bound.local_addr().expect("JSON-RPC");
        tokio::spawn(bound.serve_with_shutdown(
            JsonRpcDispatcher::new(Arc::clone(&handler)),
            std::future::pending(),
        ));
        addr
    };
    let rest = {
        let bound = Server::bind("127.0.0.1:0").await.expect("HTTP+JSON");
        let addr = bound.local_addr().expect("HTTP+JSON");
        tokio::spawn(bound.serve_with_shutdown(
            RestDispatcher::new(Arc::clone(&handler)),
            std::future::pending(),
        ));
        addr
    };

    for token in ["not-a-token", "12:x", "yesterday"] {
        let answer = jsonrpc(rpc, "ListTasks", &json!({ "pageToken": token }), &[]).await;
        let body: Value = answer.json();
        assert_eq!(body["error"]["code"], json!(-32602), "{token}: {body}");
        assert!(body.get("result").is_none(), "{token}: {body}");
        let message = body["error"]["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("pageToken is malformed"),
            "{token}: {body}"
        );
        assert!(!message.contains(token), "the token is not echoed: {body}");

        let answer = exchange(rest, "GET", &format!("/tasks?pageToken={token}"), None, &[]).await;
        assert_eq!(answer.status, 400, "{token}: {answer:?}");
    }
    // An empty token is no token, as before.
    let answer = jsonrpc(rpc, "ListTasks", &json!({ "pageToken": "" }), &[]).await;
    assert!(answer.json().get("result").is_some(), "{answer:?}");
}

/// A token the handler's own store issued still pages, on the same wire.
#[tokio::test]
async fn a_token_the_server_issued_still_pages() {
    let handler = Arc::new(
        RequestHandlerBuilder::new(Scripted(Arc::new(Seen::default())))
            .without_agent_card()
            .build()
            .expect("handler"),
    );
    for i in 0..3 {
        handler
            .on_send_message(
                immediately(send(&format!("m{i}"), "hello", None, None)),
                false,
                None,
            )
            .await
            .expect("send");
    }
    let bound = Server::bind("127.0.0.1:0").await.expect("JSON-RPC");
    let rpc = bound.local_addr().expect("JSON-RPC");
    tokio::spawn(bound.serve_with_shutdown(
        JsonRpcDispatcher::new(Arc::clone(&handler)),
        std::future::pending(),
    ));

    let first = jsonrpc(rpc, "ListTasks", &json!({ "pageSize": 2 }), &[])
        .await
        .json();
    let token = first["result"]["nextPageToken"]
        .as_str()
        .expect("a next page");
    assert!(!token.is_empty(), "{first}");
    let second = jsonrpc(
        rpc,
        "ListTasks",
        &json!({ "pageSize": 2, "pageToken": token }),
        &[],
    )
    .await
    .json();
    assert_eq!(
        second["result"]["tasks"].as_array().map(Vec::len),
        Some(1),
        "{second}"
    );
}
