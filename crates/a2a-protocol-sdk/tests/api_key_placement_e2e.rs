// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! This SDK's client authenticates to an agent whose card declares an
//! `APIKeySecurityScheme` (spec §4.5.2) by sending the key where the scheme
//! says, over every binding (audit register R35).
//!
//! `AuthInterceptor::with_scheme(.., "api-key")` writes the key into
//! `Authorization`, which an agent reading `X-Company-Key` never sees.

use std::sync::Arc;

use a2a_protocol_client::auth::{
    AuthInterceptor, CredentialsStore, InMemoryCredentialsStore, SessionId,
};
use a2a_protocol_client::error::ClientError;
use a2a_protocol_client::{A2aClient, ClientBuilder};
use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{ApiKeyAuthInterceptor, RequestHandler, RequestHandlerBuilder};
use a2a_protocol_types::error::ErrorCode;
use a2a_protocol_types::params::TaskQueryParams;
use a2a_protocol_types::security::{ApiKeyLocation, ApiKeySecurityScheme};

struct Exec;
a2a_protocol_server::agent_executor!(Exec, |_ctx, _q| async { Ok(()) });

fn handler() -> Arc<RequestHandler> {
    Arc::new(
        RequestHandlerBuilder::new(Exec)
            .without_agent_card()
            .with_interceptor(ApiKeyAuthInterceptor::new(["k-1"]).with_header("X-Company-Key"))
            .build()
            .expect("handler"),
    )
}

fn credentials() -> (Arc<dyn CredentialsStore>, SessionId) {
    let store = Arc::new(InMemoryCredentialsStore::new());
    let session = SessionId::new("s");
    store.set(session.clone(), "api-key", "k-1".into());
    (store, session)
}

/// The agent's scheme, as its card would declare it.
fn agents_scheme() -> ApiKeySecurityScheme {
    ApiKeySecurityScheme {
        location: ApiKeyLocation::Header,
        name: "X-Company-Key".into(),
        description: None,
    }
}

/// The key in `Authorization` is refused; in the scheme's header it reaches
/// the handler, which answers for the unknown task.
async fn placement_decides(misplaced: &A2aClient, placed: &A2aClient) {
    let refused = misplaced.get_task(TaskQueryParams::new("nope")).await;
    assert!(
        matches!(
            refused,
            Err(ClientError::UnexpectedStatus { status: 401, .. })
        ),
        "a key in Authorization is not the agent's key: {refused:?}"
    );
    match placed.get_task(TaskQueryParams::new("nope")).await {
        Err(ClientError::Protocol(e)) => assert_eq!(e.code, ErrorCode::TaskNotFound),
        other => panic!("the key reaches the handler: {other:?}"),
    }
}

fn clients(url: &str, binding: &str) -> (A2aClient, A2aClient) {
    let (store, session) = credentials();
    let misplaced = ClientBuilder::new(url)
        .with_protocol_binding(binding)
        .with_interceptor(AuthInterceptor::with_scheme(
            Arc::clone(&store),
            session.clone(),
            "api-key",
        ))
        .build()
        .expect("client");
    let placed = ClientBuilder::new(url)
        .with_protocol_binding(binding)
        .with_interceptor(
            AuthInterceptor::api_key(store, session, "api-key", &agents_scheme())
                .expect("a header key"),
        )
        .build()
        .expect("client");
    (misplaced, placed)
}

#[tokio::test]
async fn an_api_key_reaches_the_agent_over_json_rpc() {
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler()))
        .await
        .unwrap();
    let (misplaced, placed) = clients(&format!("http://{addr}"), "JSONRPC");
    placement_decides(&misplaced, &placed).await;
}

#[tokio::test]
async fn an_api_key_reaches_the_agent_over_http_json() {
    let addr = serve_with_addr("127.0.0.1:0", RestDispatcher::new(handler()))
        .await
        .unwrap();
    let (misplaced, placed) = clients(&format!("http://{addr}"), "HTTP+JSON");
    placement_decides(&misplaced, &placed).await;
}

#[cfg(feature = "grpc")]
#[tokio::test]
async fn an_api_key_reaches_the_agent_over_grpc() {
    use a2a_protocol_client::transport::grpc::GrpcTransport;
    use a2a_protocol_server::dispatch::grpc::{GrpcConfig, GrpcDispatcher};

    let addr = GrpcDispatcher::new(handler(), GrpcConfig::default())
        .serve_with_addr("127.0.0.1:0")
        .await
        .unwrap();
    let url = format!("http://{addr}");
    let (store, session) = credentials();
    let misplaced = ClientBuilder::new(&url)
        .with_custom_transport(GrpcTransport::connect(&url).await.expect("connect"))
        .with_interceptor(AuthInterceptor::with_scheme(
            Arc::clone(&store),
            session.clone(),
            "api-key",
        ))
        .build()
        .expect("client");
    let placed = ClientBuilder::new(&url)
        .with_custom_transport(GrpcTransport::connect(&url).await.expect("connect"))
        .with_interceptor(
            AuthInterceptor::api_key(store, session, "api-key", &agents_scheme())
                .expect("a header key"),
        )
        .build()
        .expect("client");
    placement_decides(&misplaced, &placed).await;
}
