// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `GetExtendedAgentCard` over JSON-RPC carries the tenant its `params.tenant`
//! names to the handler, and an empty one names none (adversarial review P3,
//! 2026-11-12). The tenant is what an [`ExtendedCardProvider`] sees in
//! `ctx.tenant()`, so a provider can tell two tenants of one caller apart.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use a2a_protocol_server::agent_card::ExtendedCardProvider;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::error::ServerResult;
use a2a_protocol_server::{CallContext, agent_executor};
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::jsonrpc::{JsonRpcRequest, JsonRpcSuccessResponse};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};

struct Idle;
agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });

fn card() -> AgentCard {
    AgentCard::new(
        "tenants",
        "1.0.0",
        AgentInterface::jsonrpc("http://localhost:1"),
    )
    .with_description("records the tenant a request names")
    .with_skill(AgentSkill::new("chat", "Chat", "d").with_tags(["t"]))
    .with_capabilities(AgentCapabilities::none().with_extended_agent_card(true))
}

/// Remembers the tenant each call to it ran in.
struct Records(Arc<Mutex<Vec<Option<String>>>>);

impl ExtendedCardProvider for Records {
    fn card_for<'a>(
        &'a self,
        ctx: &'a CallContext,
        configured: &'a AgentCard,
    ) -> Pin<Box<dyn Future<Output = ServerResult<AgentCard>> + Send + 'a>> {
        self.0.lock().unwrap().push(ctx.tenant().map(str::to_owned));
        Box::pin(async move { Ok(configured.clone()) })
    }
}

async fn serve(seen: Arc<Mutex<Vec<Option<String>>>>) -> SocketAddr {
    let handler = Arc::new(
        RequestHandlerBuilder::new(Idle)
            .with_agent_card(card())
            .with_extended_agent_card(card())
            .with_extended_card_provider(Records(seen))
            .allow_unauthenticated_extended_card()
            // The tenant only labels the call here; nothing is partitioned by it.
            .accept_unisolated_tenants()
            .build()
            .expect("build handler"),
    );
    let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let io = hyper_util::rt::TokioIo::new(stream);
            let dispatcher = Arc::clone(&dispatcher);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let d = Arc::clone(&dispatcher);
                    async move { Ok::<_, std::convert::Infallible>(d.dispatch(req).await) }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, service)
                .await;
            });
        }
    });
    addr
}

/// Sends `GetExtendedAgentCard` with `params` (none when `None`) and checks it
/// was answered with the card.
async fn ask(addr: SocketAddr, params: Option<serde_json::Value>) {
    let rpc = match params {
        Some(p) => JsonRpcRequest::with_params(serde_json::json!(1), "GetExtendedAgentCard", p),
        None => JsonRpcRequest::new(serde_json::json!(1), "GetExtendedAgentCard"),
    };
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Full<Bytes>>();
    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/"))
        .header("content-type", "application/json")
        .header("a2a-version", "1.0")
        .body(Full::new(Bytes::from(serde_json::to_vec(&rpc).unwrap())))
        .unwrap();
    let resp = client.request(req).await.expect("request");
    assert_eq!(resp.status(), 200);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let answered: JsonRpcSuccessResponse<AgentCard> = serde_json::from_slice(&body)
        .unwrap_or_else(|e| panic!("a success response with the card ({e}): {body:?}"));
    assert_eq!(answered.result.name, "tenants");
}

#[tokio::test]
async fn the_tenant_a_request_names_reaches_the_provider() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let addr = serve(Arc::clone(&seen)).await;
    ask(addr, Some(serde_json::json!({"tenant": "acme"}))).await;
    assert_eq!(*seen.lock().unwrap(), [Some("acme".to_owned())]);
}

#[tokio::test]
async fn a_request_that_names_no_tenant_runs_in_none() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let addr = serve(Arc::clone(&seen)).await;
    // Absent, absent in the params, and empty: none of them names a tenant.
    ask(addr, None).await;
    ask(addr, Some(serde_json::json!({}))).await;
    ask(addr, Some(serde_json::json!({"tenant": ""}))).await;
    assert_eq!(*seen.lock().unwrap(), [None, None, None]);
}
