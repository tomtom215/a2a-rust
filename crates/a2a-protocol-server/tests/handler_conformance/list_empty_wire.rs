// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! An empty `ListTasks` page on the wire. Spec §5.7 says "Arrays marked as
//! required MUST contain at least one element", and a2a.proto marks
//! `ListTasksResponse.tasks` REQUIRED; §3.1.4 defines ListTasks as a
//! filtered query with "Errors: None specific to this operation" and says
//! `nextPageToken` "MUST be set to an empty string" on the last page. No
//! answer satisfies both readings for a filter that matches nothing; the SDK
//! answers an empty page with both keys present, `"tasks": []` and
//! `"nextPageToken": ""`, which is what this pins on JSON-RPC and HTTP+JSON.

use std::sync::Arc;

use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{RequestHandlerBuilder, agent_executor};
use serde_json::{Value, json};

use crate::{exchange, jsonrpc};

struct Exec;
agent_executor!(Exec, |_ctx, _q| async { Ok(()) });

fn assert_empty_page(page: &Value, what: &str) {
    assert_eq!(page["tasks"], json!([]), "{what}: {page}");
    assert_eq!(page["nextPageToken"], json!(""), "{what}: {page}");
    assert_eq!(page["totalSize"], json!(0), "{what}: {page}");
}

#[tokio::test]
async fn an_empty_list_tasks_page_carries_an_empty_tasks_array_and_page_token() {
    let handler = Arc::new(
        RequestHandlerBuilder::new(Exec)
            .without_agent_card()
            .build()
            .expect("handler"),
    );
    let rpc = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(Arc::clone(&handler)))
        .await
        .expect("JSON-RPC");
    let rest = serve_with_addr("127.0.0.1:0", RestDispatcher::new(Arc::clone(&handler)))
        .await
        .expect("HTTP+JSON");

    // Nothing stored, and a filter that matches nothing.
    for params in [json!({}), json!({"contextId": "no-such-context"})] {
        let answer = jsonrpc(rpc, "ListTasks", &params, &[]).await;
        assert_empty_page(&answer.json()["result"], &format!("JSON-RPC {params}"));
    }
    for path in ["/tasks", "/tasks?contextId=no-such-context"] {
        let answer = exchange(rest, "GET", path, None, &[]).await;
        assert_eq!(answer.status, 200, "{answer:?}");
        assert_empty_page(&answer.json(), &format!("HTTP+JSON {path}"));
    }
}
