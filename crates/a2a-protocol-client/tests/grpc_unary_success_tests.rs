// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A successful unary call over gRPC, answered by a hand-written server.
//!
//! Until 2026-09-23 no test in this crate made one. gRPC's success path was
//! exercised from the server and SDK crates, and cargo-mutants runs only the
//! mutated crate's tests, so replacing the transport's conversion of every
//! answer with `null` survived (audit N9). The server here writes the
//! length-prefixed protobuf frame and the `grpc-status: 0` trailer itself, so
//! the test needs nothing from `a2a-protocol-server`.

#![cfg(feature = "grpc")]

use std::convert::Infallible;

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_types::params::TaskQueryParams;
use a2a_protocol_types::proto as pb;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};
use hyper::body::{Bytes, Frame};
use prost::Message as _;

fn stored_task() -> Task {
    Task {
        id: TaskId::new("task-42"),
        context_id: ContextId::new("ctx-7"),
        status: TaskStatus::new(TaskState::Completed),
        history: None,
        artifacts: None,
        metadata: None,
    }
}

/// Answers `GetTask` for `stored_task()`'s id with that task, as one
/// length-prefixed frame followed by an OK status trailer, and any other id
/// with `NOT_FOUND` — so the request the client sends has to carry the id it
/// was given, not just reach the server.
async fn grpc_server() -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let svc = hyper::service::service_fn(
                    |req: hyper::Request<hyper::body::Incoming>| async {
                        use http_body_util::BodyExt;
                        let body = req.into_body().collect().await.expect("body").to_bytes();
                        // Skip the 5-byte gRPC message prefix.
                        let asked = pb::GetTaskRequest::decode(body.get(5..).unwrap_or_default())
                            .map(|r| r.id)
                            .unwrap_or_default();
                        let mut trailers = hyper::HeaderMap::new();
                        let mut frames: Vec<Result<Frame<Bytes>, Infallible>> = Vec::new();
                        if asked == stored_task().id.0 {
                            let payload = pb::Task::try_from(stored_task())
                                .expect("task converts")
                                .encode_to_vec();
                            let mut frame = vec![0_u8];
                            frame.extend_from_slice(
                                &u32::try_from(payload.len()).expect("len").to_be_bytes(),
                            );
                            frame.extend_from_slice(&payload);
                            frames.push(Ok(Frame::data(Bytes::from(frame))));
                            trailers.insert(
                                "grpc-status",
                                hyper::header::HeaderValue::from_static("0"),
                            );
                        } else {
                            trailers.insert(
                                "grpc-status",
                                hyper::header::HeaderValue::from_static("5"),
                            );
                        }
                        frames.push(Ok(Frame::trailers(trailers)));
                        let body = http_body_util::StreamBody::new(tokio_stream::iter(frames));
                        let mut resp = hyper::Response::new(body);
                        resp.headers_mut().insert(
                            "content-type",
                            hyper::header::HeaderValue::from_static("application/grpc"),
                        );
                        Ok::<_, Infallible>(resp)
                    },
                );
                let _ =
                    hyper::server::conn::http2::Builder::new(hyper_util::rt::TokioExecutor::new())
                        .serve_connection(hyper_util::rt::TokioIo::new(stream), svc)
                        .await;
            });
        }
    });
    addr
}

fn query(id: &str) -> TaskQueryParams {
    TaskQueryParams {
        tenant: None,
        id: id.into(),
        history_length: None,
    }
}

#[tokio::test]
async fn a_unary_grpc_call_returns_what_the_server_sent() {
    let addr = grpc_server().await;
    let client = ClientBuilder::new(format!("http://{addr}"))
        .with_protocol_binding("GRPC")
        .build_grpc()
        .await
        .expect("client");
    let task = client
        .get_task(query("task-42"))
        .await
        .expect("the server's task");
    assert_eq!(task, stored_task());

    // The id reaches the wire: the server knows no other.
    assert!(client.get_task(query("task-7")).await.is_err());
}

/// The same through `GrpcTransport::connect` and a custom transport, the
/// route the book documents for a caller who configures the transport itself.
#[tokio::test]
async fn a_transport_from_connect_carries_the_call() {
    use a2a_protocol_client::GrpcTransport;

    let addr = grpc_server().await;
    let url = format!("http://{addr}");
    let transport = GrpcTransport::connect(url.clone()).await.expect("connect");
    let client = ClientBuilder::new(url)
        .with_custom_transport(transport)
        .build()
        .expect("client");
    assert_eq!(
        client.get_task(query("task-42")).await.expect("task"),
        stored_task()
    );
}

/// `connect` refuses an address it cannot dial, naming it.
#[tokio::test]
async fn connect_refuses_an_unusable_address() {
    use a2a_protocol_client::{ClientError, GrpcTransport};

    match GrpcTransport::connect("grpc://agent:1").await {
        Err(ClientError::InvalidEndpoint(msg)) => assert!(msg.contains("grpc://agent:1"), "{msg}"),
        Err(other) => panic!("expected InvalidEndpoint, got {other:?}"),
        Ok(_) => panic!("expected InvalidEndpoint, got a transport"),
    }
}
