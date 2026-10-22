// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! This repository's server signs what it serves and this client verifies
//! it (ADR 0027), over JSON-RPC and REST: a stream with an artifact in two
//! chunks and status notes, then `GetTask`, whose merged artifact verifies
//! whole. A client trusting another key refuses the same content.
#![cfg(feature = "signing")]

use std::net::SocketAddr;
use std::sync::Arc;

use a2a_protocol_client::{ClientBuilder, ClientError};
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::{Dispatcher, EventEmitter, agent_executor};
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::message::{Message, MessageRole, Part};
use a2a_protocol_types::params::{MessageSendParams, TaskQueryParams};
use a2a_protocol_types::provenance::{ContentSigner, verify_content};
use a2a_protocol_types::signing::Jwks;
use a2a_protocol_types::task::TaskState;

fn signer(kid: &str) -> ContentSigner {
    let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
        .expect("a key");
    ContentSigner::ed25519(kid, pkcs8.as_ref()).expect("an Ed25519 signer")
}

struct Agent;
agent_executor!(Agent, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;
    emit.artifact("report", vec![Part::text("part one")], None, Some(false))
        .await?;
    emit.artifact(
        "report",
        vec![Part::text("part two")],
        Some(true),
        Some(true),
    )
    .await?;
    emit.status(TaskState::Completed).await
});

async fn serve(rest: bool, by: ContentSigner) -> SocketAddr {
    let handler = Arc::new(
        RequestHandlerBuilder::new(Agent)
            .without_agent_card()
            .with_content_signer(by)
            .build()
            .expect("handler"),
    );
    let dispatcher: Arc<dyn Dispatcher> = if rest {
        Arc::new(RestDispatcher::new(handler))
    } else {
        Arc::new(JsonRpcDispatcher::new(handler))
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let io = hyper_util::rt::TokioIo::new(stream);
            let d = Arc::clone(&dispatcher);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let d = Arc::clone(&d);
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

fn client(addr: SocketAddr, rest: bool, keys: Jwks) -> a2a_protocol_client::A2aClient {
    let mut builder = ClientBuilder::new(format!("http://{addr}"))
        .without_capability_checks()
        .with_content_keys(keys);
    if rest {
        builder = builder.with_protocol_binding("HTTP+JSON");
    }
    builder.build().expect("client")
}

fn send() -> MessageSendParams {
    MessageSendParams::new(Message::new(
        "m-1",
        MessageRole::User,
        vec![Part::text("go")],
    ))
}

async fn round_trip(rest: bool) {
    let by = signer("agent");
    let keys = Jwks::new(vec![by.public_jwk()]);
    let addr = serve(rest, by).await;
    let c = client(addr, rest, keys.clone());

    let mut stream = c.stream_message(send()).await.expect("stream");
    let (mut chunks, mut task_id) = (0, None);
    while let Some(event) = stream.next().await {
        match event.expect("every frame verifies") {
            StreamResponse::ArtifactUpdate(u) => {
                verify_content(&u.artifact, &keys).expect("a chunk verifies as itself");
                task_id = Some(u.task_id.0.clone());
                chunks += 1;
            }
            StreamResponse::Task(t) => task_id = Some(t.id.0.clone()),
            _ => {}
        }
    }
    assert_eq!(chunks, 2);

    let task = c
        .get_task(TaskQueryParams {
            tenant: None,
            id: task_id.expect("a task id"),
            history_length: None,
        })
        .await
        .expect("the stored task verifies");
    let report = &task.artifacts.as_ref().expect("artifacts")[0];
    assert_eq!(report.parts.len(), 2, "merged");
    verify_content(report, &keys).expect("the merged artifact verifies whole");

    // A client trusting another key refuses the same content.
    let wary = client(addr, rest, Jwks::new(vec![signer("other").public_jwk()]));
    let mut stream = wary.stream_message(send()).await.expect("stream");
    let mut refused = false;
    while let Some(event) = stream.next().await {
        if let Err(e) = event {
            assert!(matches!(e, ClientError::ContentNotVerified(_)), "{e}");
            refused = true;
        }
    }
    assert!(refused);
}

#[tokio::test]
async fn jsonrpc_signed_by_the_server_verified_by_the_client() {
    round_trip(false).await;
}

#[tokio::test]
async fn rest_signed_by_the_server_verified_by_the_client() {
    round_trip(true).await;
}
