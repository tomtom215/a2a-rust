// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! P2: a tenant is an opaque value; it round-trips whatever it is called.

use super::common::*;
use a2a_protocol_server::BearerTokenAuthInterceptor;
use a2a_protocol_server::call_context::CallContext;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::{CombinedDispatcher, DispatchConfig};
use a2a_protocol_server::serve::Server;
use a2a_protocol_types::SendMessageResponse;

// ── P2: a tenant is an opaque value; it round-trips whatever it is called ────

use a2a_protocol_server::interceptor::ServerInterceptor;

/// Records `(method, tenant)` of every call that reaches the handler.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<Call>>>);

/// A call that reached the handler: its method, and the tenant it named.
type Call = (String, Option<String>);

impl ServerInterceptor for Seen {
    fn before<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>,
    > {
        self.0
            .lock()
            .unwrap()
            .push((ctx.method().to_owned(), ctx.tenant().map(str::to_owned)));
        Box::pin(async { Ok(()) })
    }
    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>,
    > {
        Box::pin(async { Ok(()) })
    }
}

async fn serve_recording() -> (SocketAddr, Seen) {
    let seen = Seen::default();
    let public = extended_public_card("http://placeholder.invalid", "JSONRPC");
    let handler = Arc::new(
        RequestHandlerBuilder::new(Echo)
            .with_agent_card(public.clone())
            .with_extended_agent_card(public)
            .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([(
                "tok-alice",
                "alice",
            )]))
            .with_interceptor(seen.clone())
            .accept_unisolated_tenants()
            .build()
            .unwrap(),
    );
    let server = Server::bind("127.0.0.1:0").await.unwrap();
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve_with_shutdown(
        CombinedDispatcher::with_config(handler, DispatchConfig::default()),
        std::future::pending(),
    ));
    (addr, seen)
}

/// Every operation a client makes for tenant `tenant` arrives at the server
/// naming exactly that tenant, however the tenant is spelled: the names of the
/// REST routes themselves, a name with a space, a percent sign, a slash, a
/// colon or non-ASCII text. On REST and JSON-RPC.
#[tokio::test(flavor = "multi_thread")]
async fn an_opaque_tenant_reaches_the_server_unchanged_for_every_operation() {
    let (addr, seen) = serve_recording().await;
    let tenants = [
        "acme",
        "tasks",
        "message",
        "extendedAgentCard",
        "tenants",
        "message:send",
        "a b",
        "100%",
        "a/b",
        "x:y",
        "ünï",
    ];
    for binding in ["HTTP+JSON", "JSONRPC"] {
        for tenant in tenants {
            let url = format!("http://{addr}");
            let client = ClientBuilder::from_card(&extended_public_card(&url, binding))
                .unwrap()
                .with_tenant(tenant)
                .with_interceptor(alice())
                .build()
                .unwrap();
            let what = |op: &str| format!("{binding}, tenant {tenant:?}, {op}");

            seen.0.lock().unwrap().clear();
            let task = match client.send_message(params()).await {
                Ok(SendMessageResponse::Task(task)) => task,
                other => panic!("{}: {other:?}", what("SendMessage")),
            };
            client
                .get_task(a2a_protocol_types::params::TaskQueryParams::new(
                    task.id.0.clone(),
                ))
                .await
                .unwrap_or_else(|e| panic!("{}: {e}", what("GetTask")));
            client
                .list_tasks(a2a_protocol_types::params::ListTasksParams::default())
                .await
                .unwrap_or_else(|e| panic!("{}: {e}", what("ListTasks")));
            let _ = client.cancel_task(task.id.0.clone()).await; // a finished task is not cancelable: the route still reached the handler
            client
                .get_extended_agent_card()
                .await
                .unwrap_or_else(|e| panic!("{}: {e}", what("GetExtendedAgentCard")));

            let calls = seen.0.lock().unwrap().clone();
            let methods: Vec<&str> = calls.iter().map(|(m, _)| m.as_str()).collect();
            for expected in [
                "SendMessage",
                "GetTask",
                "ListTasks",
                "CancelTask",
                "GetExtendedAgentCard",
            ] {
                assert!(
                    methods.contains(&expected),
                    "{}: {expected} never reached the handler (reached: {methods:?})",
                    what("routing")
                );
            }
            for (method, got) in calls {
                assert_eq!(
                    got.as_deref(),
                    Some(tenant),
                    "{}: {method} arrived naming another tenant",
                    what(&method)
                );
            }
        }
    }
}
