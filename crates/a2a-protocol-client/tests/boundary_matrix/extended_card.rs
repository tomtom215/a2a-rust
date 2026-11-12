// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! P3: the extended card is served for the tenant the request names.

use super::common::*;
use a2a_protocol_server::BearerTokenAuthInterceptor;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::{CombinedDispatcher, DispatchConfig};
use a2a_protocol_server::serve::Server;
use a2a_protocol_types::AgentCard;

// ── P3: the extended card is served for the tenant the request names ─────────

use a2a_protocol_server::call_context::CallContext;
use a2a_protocol_server::error::ServerResult;
use a2a_protocol_server::tenant_resolver::HeaderTenantResolver;

/// Serves a card that says which tenant it was made for.
struct PerTenantCards {
    asked: Arc<Mutex<Vec<Option<String>>>>,
}

impl a2a_protocol_server::ExtendedCardProvider for PerTenantCards {
    fn card_for<'a>(
        &'a self,
        ctx: &'a CallContext,
        configured: &'a AgentCard,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ServerResult<AgentCard>> + Send + 'a>>
    {
        self.asked
            .lock()
            .unwrap()
            .push(ctx.tenant().map(str::to_owned));
        let mut card = configured.clone();
        card.description = format!("card for tenant {:?}", ctx.tenant().unwrap_or(""));
        Box::pin(async move { Ok(card) })
    }
}

/// A server whose extended card depends on the tenant; `strict` requires a
/// resolved tenant (from `x-tenant-id`), so a request without one is refused.
async fn serve_extended(strict: bool) -> (SocketAddr, Arc<Mutex<Vec<Option<String>>>>) {
    let asked: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
    let public = extended_public_card("http://placeholder.invalid", "JSONRPC");
    let mut builder = RequestHandlerBuilder::new(Echo)
        .with_agent_card(public.clone())
        .with_extended_agent_card(public)
        .with_extended_card_provider(PerTenantCards {
            asked: Arc::clone(&asked),
        })
        .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([(
            "tok-alice",
            "alice",
        )]));
    if strict {
        builder = builder
            .with_tenant_resolver(HeaderTenantResolver::default())
            .require_resolved_tenant();
    }
    let handler = Arc::new(builder.build().unwrap());
    let server = Server::bind("127.0.0.1:0").await.unwrap();
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve_with_shutdown(
        CombinedDispatcher::with_config(handler, DispatchConfig::default()),
        std::future::pending(),
    ));
    (addr, asked)
}

/// One principal, two tenants: each tenant's request reaches the provider with
/// that tenant, over JSON-RPC and over REST, and gets that tenant's card.
#[tokio::test(flavor = "multi_thread")]
async fn the_extended_card_is_served_for_the_tenant_the_request_names() {
    let (addr, asked) = serve_extended(false).await;
    for binding in ["JSONRPC", "HTTP+JSON"] {
        for tenant in ["alpha", "beta"] {
            let url = format!("http://{addr}");
            let client = ClientBuilder::from_card(&extended_public_card(&url, binding))
                .unwrap()
                .with_tenant(tenant)
                .with_interceptor(alice())
                .build()
                .unwrap();
            asked.lock().unwrap().clear();
            let card = client
                .get_extended_agent_card()
                .await
                .unwrap_or_else(|e| panic!("{binding}, tenant {tenant}: {e}"));
            assert_eq!(
                asked.lock().unwrap().as_slice(),
                [Some(tenant.to_owned())],
                "{binding}: the provider was not given the request's tenant"
            );
            assert_eq!(
                card.description,
                format!("card for tenant {tenant:?}"),
                "{binding}, tenant {tenant}"
            );
        }
    }
}

/// A tenant the server will not admit is refused before the provider runs, as
/// every other operation refuses it.
#[tokio::test(flavor = "multi_thread")]
async fn an_unresolved_tenant_is_refused_before_the_extended_card_provider() {
    let (addr, asked) = serve_extended(true).await;
    // JSON-RPC and REST, with the bearer token but no `x-tenant-id` header.
    let (status, body) = raw(
        addr,
        "POST",
        "/",
        "Bearer tok-alice",
        Some(rpc(
            "GetExtendedAgentCard",
            serde_json::json!({"tenant": "alpha"}),
        )),
    )
    .await;
    assert!(
        status != 200 || body.get("error").is_some(),
        "an unresolved tenant was served: {body}"
    );
    let (status, _) = raw(addr, "GET", "/extendedAgentCard", "Bearer tok-alice", None).await;
    assert_ne!(status, 200, "REST served an unresolved tenant");
    assert!(
        asked.lock().unwrap().is_empty(),
        "the provider ran for a refused tenant"
    );
}
