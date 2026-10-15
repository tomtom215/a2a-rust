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
            .without_agent_card()
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

/// Over WebSocket the credential rides the upgrade and a refusal arrives in
/// the body (binding spec §6), so it is checked apart: it is the same `401`,
/// on a unary call and on a stream, and drops the refused token. Recovering
/// means a new connection, which the fresh token opens. Until 2026-10-15 the
/// refusal was `Protocol(InvalidRequest)`, and no token was dropped.
#[cfg(feature = "websocket")]
#[tokio::test]
async fn a_refused_token_is_a_401_over_websocket() {
    use a2a_protocol_client::transport::websocket::{WebSocketTransport, WebSocketTransportConfig};
    use a2a_protocol_server::dispatch::websocket::WebSocketDispatcher;
    use a2a_protocol_types::message::Message;
    use a2a_protocol_types::params::MessageSendParams;

    let addr = Arc::new(WebSocketDispatcher::new(handler()))
        .serve_with_addr("127.0.0.1:0")
        .await
        .unwrap();
    let url = format!("ws://{addr}");
    let provider = Arc::new(RotatingProvider::default());
    let connect = |token: &str| {
        let headers = [("authorization".to_owned(), format!("Bearer {token}"))].into();
        let (url, provider) = (url.clone(), Arc::clone(&provider));
        async move {
            let transport = WebSocketTransport::connect_with_config(
                &url,
                WebSocketTransportConfig::default().with_extra_headers(headers),
            )
            .await
            .expect("connect");
            ClientBuilder::new(&url)
                .with_custom_transport(transport)
                .with_interceptor(BearerAuthInterceptor::new(
                    provider as Arc<dyn TokenProvider>,
                ))
                .build()
                .expect("client")
        }
    };

    let stale = connect("stale").await;
    let first = stale.get_task(TaskQueryParams::new("nope")).await;
    assert!(
        matches!(
            first,
            Err(ClientError::UnexpectedStatus { status: 401, .. })
        ),
        "the refusal is a 401: {first:?}"
    );
    assert_eq!(*provider.invalidated.lock().unwrap(), ["stale"]);
    let mut stream = stale
        .stream_message(MessageSendParams::new(Message::user_text("m-1", "hi")))
        .await
        .expect("the stream opens; its first frame is the refusal");
    let refused = stream.next().await;
    assert!(
        matches!(
            refused,
            Some(Err(ClientError::UnexpectedStatus { status: 401, .. }))
        ),
        "a stream's refusal is a 401 too: {refused:?}"
    );

    let fresh = connect("fresh").await;
    match fresh.get_task(TaskQueryParams::new("nope")).await {
        Err(ClientError::Protocol(e)) => assert_eq!(e.code, ErrorCode::TaskNotFound),
        other => panic!("the fresh token reaches the handler: {other:?}"),
    }
}
