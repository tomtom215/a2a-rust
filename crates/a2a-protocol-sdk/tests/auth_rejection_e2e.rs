// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! This SDK's client, refused by this SDK's server, discards the refused
//! token and recovers on the next call — over every binding (audit N36).
//!
//! The client's `BearerAuthInterceptor` tells its `TokenProvider` to drop a
//! token when the agent answers `401` (gRPC `UNAUTHENTICATED` maps to it).
//! The server answered every refused credential `400` / `INVALID_ARGUMENT`,
//! so a revoked or rotated token was sent again on every call until the
//! provider's own cache expired, each call failing as a malformed request.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use a2a_protocol_client::error::{ClientError, ClientResult};
use a2a_protocol_client::token_provider::{BearerAuthInterceptor, TokenProvider};
use a2a_protocol_client::{A2aClient, ClientBuilder};
use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{BearerTokenAuthInterceptor, RequestHandler, RequestHandlerBuilder};
use a2a_protocol_types::error::ErrorCode;
use a2a_protocol_types::params::TaskQueryParams;

struct Exec;
a2a_protocol_server::agent_executor!(Exec, |_ctx, _q| async { Ok(()) });

/// Serves `stale` until told to drop it, then `fresh`.
#[derive(Default)]
struct RotatingProvider {
    invalidated: Mutex<Vec<String>>,
}

impl TokenProvider for RotatingProvider {
    fn access_token(&self) -> Pin<Box<dyn Future<Output = ClientResult<String>> + Send + '_>> {
        let stale = self.invalidated.lock().unwrap().is_empty();
        Box::pin(async move { Ok(if stale { "stale" } else { "fresh" }.to_owned()) })
    }

    fn invalidate(&self, token: &str) {
        self.invalidated.lock().unwrap().push(token.to_owned());
    }
}

fn handler() -> Arc<RequestHandler> {
    Arc::new(
        RequestHandlerBuilder::new(Exec)
            .with_interceptor(BearerTokenAuthInterceptor::new(["fresh"]))
            .build()
            .expect("handler"),
    )
}

/// The first call is refused and drops the stale token; the second reaches
/// the handler, which answers the task it does not have as not found.
async fn recovers(client: &A2aClient, provider: &RotatingProvider) {
    let first = client.get_task(TaskQueryParams::new("nope")).await;
    assert!(
        matches!(
            first,
            Err(ClientError::UnexpectedStatus { status: 401, .. })
        ),
        "the refusal is a 401: {first:?}"
    );
    assert_eq!(*provider.invalidated.lock().unwrap(), ["stale"]);

    let second = client.get_task(TaskQueryParams::new("nope")).await;
    match second {
        Err(ClientError::Protocol(e)) => assert_eq!(e.code, ErrorCode::TaskNotFound),
        other => panic!("the fresh token reaches the handler: {other:?}"),
    }
}

fn client(url: &str, binding: &str, provider: &Arc<RotatingProvider>) -> A2aClient {
    ClientBuilder::new(url)
        .with_protocol_binding(binding)
        .with_interceptor(BearerAuthInterceptor::new(
            Arc::clone(provider) as Arc<dyn TokenProvider>
        ))
        .build()
        .expect("client")
}

#[tokio::test]
async fn a_refused_token_is_dropped_over_json_rpc() {
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler()))
        .await
        .unwrap();
    let provider = Arc::new(RotatingProvider::default());
    recovers(
        &client(&format!("http://{addr}"), "JSONRPC", &provider),
        &provider,
    )
    .await;
}

#[tokio::test]
async fn a_refused_token_is_dropped_over_http_json() {
    let addr = serve_with_addr("127.0.0.1:0", RestDispatcher::new(handler()))
        .await
        .unwrap();
    let provider = Arc::new(RotatingProvider::default());
    recovers(
        &client(&format!("http://{addr}"), "HTTP+JSON", &provider),
        &provider,
    )
    .await;
}

#[cfg(feature = "grpc")]
#[tokio::test]
async fn a_refused_token_is_dropped_over_grpc() {
    use a2a_protocol_client::transport::grpc::GrpcTransport;
    use a2a_protocol_server::dispatch::grpc::{GrpcConfig, GrpcDispatcher};

    let addr = GrpcDispatcher::new(handler(), GrpcConfig::default())
        .serve_with_addr("127.0.0.1:0")
        .await
        .unwrap();
    let url = format!("http://{addr}");
    let provider = Arc::new(RotatingProvider::default());
    let client = ClientBuilder::new(&url)
        .with_custom_transport(GrpcTransport::connect(&url).await.expect("connect"))
        .with_interceptor(BearerAuthInterceptor::new(
            Arc::clone(&provider) as Arc<dyn TokenProvider>
        ))
        .build()
        .expect("client");
    recovers(&client, &provider).await;
}
