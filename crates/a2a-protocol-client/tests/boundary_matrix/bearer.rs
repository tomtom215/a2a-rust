// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! R5: one bearer parser, one tenant.

use super::common::*;
use std::net::SocketAddr;
use std::sync::Arc;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::{CombinedDispatcher, DispatchConfig};
use a2a_protocol_server::serve::Server;

// ── R5: one bearer parser, one tenant, on every binding ──────────────────────

use a2a_protocol_server::{BearerTokenAuthInterceptor, tenant_resolver::BearerTokenTenantResolver};

/// A server that authenticates `tok-alice` and routes by the bearer tenant
/// resolver, strictly or with the empty-tenant fallback.
async fn serve_authenticated(strict: bool) -> SocketAddr {
    let mut builder = RequestHandlerBuilder::new(Echo)
        .without_agent_card()
        .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([(
            "tok-alice",
            "alice",
        )]))
        .with_tenant_resolver(BearerTokenTenantResolver::default());
    if strict {
        builder = builder.require_resolved_tenant();
    }
    let handler = Arc::new(builder.build().unwrap());
    let server = Server::bind("127.0.0.1:0").await.unwrap();
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve_with_shutdown(
        CombinedDispatcher::with_config(handler, DispatchConfig::default()),
        std::future::pending(),
    ));
    addr
}

const SPELLINGS: [&str; 6] = [
    "Bearer tok-alice",
    "bearer tok-alice",
    "BEARER tok-alice",
    "BeArEr tok-alice",
    "Bearer   tok-alice",
    "Bearer tok-alice ",
];

fn rest_send() -> serde_json::Value {
    serde_json::json!({"message": {"messageId": "m-1", "role": "ROLE_USER", "parts": [{"text": "hi"}]}})
}

/// Whatever spelling creates a task, the canonical spelling reads it: the
/// spelling does not change the tenant, on REST and on JSON-RPC, whether the
/// server falls back to the empty tenant or requires a resolved one.
#[tokio::test(flavor = "multi_thread")]
async fn a_bearer_spelling_does_not_change_the_tenant_on_any_binding() {
    for strict in [false, true] {
        let addr = serve_authenticated(strict).await;
        for spelling in SPELLINGS {
            let what = format!("{spelling:?}, strict {strict}");
            // REST: create under this spelling, read under the canonical one.
            let (status, body) =
                raw(addr, "POST", "/message:send", spelling, Some(rest_send())).await;
            assert_eq!(status, 200, "REST create, {what}: {body}");
            let id = body["task"]["id"].as_str().expect("a task id").to_owned();
            let (status, got) = raw(
                addr,
                "GET",
                &format!("/tasks/{id}"),
                "Bearer tok-alice",
                None,
            )
            .await;
            assert_eq!(
                status, 200,
                "REST: a task made under {what} is not in the canonical caller's tenant: {got}"
            );

            // JSON-RPC: the same.
            let (status, body) = raw(
                addr,
                "POST",
                "/",
                spelling,
                Some(rpc("SendMessage", rest_send())),
            )
            .await;
            assert_eq!(status, 200, "JSON-RPC create, {what}: {body}");
            let id = body["result"]["task"]["id"]
                .as_str()
                .unwrap_or_else(|| panic!("a task id in {body}"))
                .to_owned();
            let (status, got) = raw(
                addr,
                "POST",
                "/",
                "Bearer tok-alice",
                Some(rpc("GetTask", serde_json::json!({"id": id}))),
            )
            .await;
            assert!(
                status == 200 && got.get("error").is_none(),
                "JSON-RPC: a task made under {what} is not in the canonical caller's tenant: {got}"
            );
        }
    }
}
