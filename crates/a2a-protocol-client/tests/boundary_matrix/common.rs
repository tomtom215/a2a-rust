// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What every test of the matrix shares: a real server, a recording proxy and
//! a few client calls.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use a2a_protocol_client::A2aClient;
use a2a_protocol_server::agent_executor;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::{CombinedDispatcher, DispatchConfig};
use a2a_protocol_server::serve::Server;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::task::TaskState;
use a2a_protocol_types::{AgentCapabilities, AgentCard, AgentInterface, SendMessageResponse};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub(crate) struct Echo;
agent_executor!(Echo, |ctx, queue| async {
    let emit = a2a_protocol_server::EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;
    emit.artifact("a", vec![Part::text("ok")], None, Some(true))
        .await?;
    emit.status(TaskState::Completed).await
});

/// A real server (all bindings on one port), optionally under a base path.
pub(crate) async fn serve(base_path: Option<&str>) -> SocketAddr {
    let handler = Arc::new(
        RequestHandlerBuilder::new(Echo)
            .without_agent_card()
            .build()
            .unwrap(),
    );
    let mut config = DispatchConfig::default();
    if let Some(base) = base_path {
        config = config.with_base_path(base).unwrap();
    }
    let server = Server::bind("127.0.0.1:0").await.unwrap();
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve_with_shutdown(
        CombinedDispatcher::with_config(handler, config),
        std::future::pending(),
    ));
    addr
}

/// What the client sent, as the first line of each request: `METHOD target`.
pub(crate) type Targets = Arc<Mutex<Vec<String>>>;

/// A TCP proxy that records the request line of the first request on each
/// connection and forwards everything unchanged.
pub(crate) async fn recording_proxy(upstream: SocketAddr) -> (SocketAddr, Targets) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let targets: Targets = Arc::default();
    let seen = Arc::clone(&targets);
    tokio::spawn(async move {
        while let Ok((mut client, _)) = listener.accept().await {
            let seen = Arc::clone(&seen);
            tokio::spawn(async move {
                let Ok(mut server) = TcpStream::connect(upstream).await else {
                    return;
                };
                let mut first = vec![0_u8; 16 * 1024];
                let Ok(n) = client.read(&mut first).await else {
                    return;
                };
                if n == 0 {
                    return;
                }
                let line = String::from_utf8_lossy(&first[..n])
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                // "POST /a2a/message:send HTTP/1.1" -> "POST /a2a/message:send"
                let mut parts = line.split(' ');
                let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                seen.lock().unwrap().push(format!("{method} {target}"));
                if server.write_all(&first[..n]).await.is_err() {
                    return;
                }
                let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
            });
        }
    });
    (addr, targets)
}

pub(crate) fn params() -> MessageSendParams {
    MessageSendParams::new(Message::new(
        MessageId::new("m-1"),
        MessageRole::User,
        vec![Part::text("hello")],
    ))
}

pub(crate) async fn send_ok(client: &A2aClient, what: &str) {
    match client.send_message(params()).await {
        Ok(SendMessageResponse::Task(task)) => {
            assert_eq!(task.status.state, TaskState::Completed, "{what}");
        }
        other => panic!("{what}: expected a completed task, got {other:?}"),
    }
}

pub(crate) fn card_for(url: &str, binding: &str) -> AgentCard {
    AgentCard::new("matrix", "1.0", AgentInterface::new(url, binding))
        .with_description("boundary matrix")
        .with_input_modes(vec!["text/plain"])
        .with_output_modes(vec!["text/plain"])
        .with_capabilities(AgentCapabilities::default())
}

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;

/// One raw HTTP call with an exact `Authorization` header: `(status, body)`.
pub(crate) async fn raw(
    addr: SocketAddr,
    method: &str,
    path: &str,
    authorization: &str,
    body: Option<serde_json::Value>,
) -> (u16, serde_json::Value) {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Full<Bytes>>();
    let req = hyper::Request::builder()
        .method(method)
        .uri(format!("http://{addr}{path}"))
        .header("content-type", "application/json")
        .header("a2a-version", "1.0")
        .header("authorization", authorization)
        .body(Full::new(Bytes::from(
            body.map(|b| b.to_string()).unwrap_or_default(),
        )))
        .unwrap();
    let resp = client.request(req).await.unwrap();
    let status = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

pub(crate) fn rpc(method: &str, params: serde_json::Value) -> serde_json::Value {
    serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
}

/// The client's credential interceptor for `tok-alice`.
pub(crate) fn alice() -> a2a_protocol_client::AuthInterceptor {
    use a2a_protocol_client::{CredentialsStore, InMemoryCredentialsStore, SessionId};
    let store = Arc::new(InMemoryCredentialsStore::new());
    let session = SessionId::new("alice");
    store.set(session.clone(), "bearer", "tok-alice".to_owned());
    a2a_protocol_client::AuthInterceptor::new(store, session)
}

pub(crate) fn extended_public_card(url: &str, binding: &str) -> AgentCard {
    AgentCard::new("ext", "1.0", AgentInterface::new(url, binding))
        .with_description("public")
        .with_input_modes(vec!["text/plain"])
        .with_output_modes(vec!["text/plain"])
        .with_skill(
            a2a_protocol_types::AgentSkill::new("echo", "Echo", "Echoes input").with_tags(["echo"]),
        )
        .with_capabilities(AgentCapabilities::default().with_extended_agent_card(true))
        // Spec §13.3: the scheme the bearer interceptor checks, in the public card.
        .with_security_schemes(std::collections::HashMap::from([(
            "bearer".to_owned(),
            a2a_protocol_types::security::SecurityScheme::Http(
                a2a_protocol_types::security::HttpAuthSecurityScheme::new("Bearer".to_owned()),
            ),
        )]))
}
