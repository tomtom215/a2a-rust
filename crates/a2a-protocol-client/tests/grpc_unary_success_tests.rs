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

/// Answers every call with `stored_task()`, as one length-prefixed frame
/// followed by an OK status trailer.
async fn grpc_server() -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let svc = hyper::service::service_fn(
                    |_req: hyper::Request<hyper::body::Incoming>| async {
                        let payload = pb::Task::try_from(stored_task())
                            .expect("task converts")
                            .encode_to_vec();
                        let mut frame = vec![0_u8];
                        frame.extend_from_slice(
                            &u32::try_from(payload.len()).expect("len").to_be_bytes(),
                        );
                        frame.extend_from_slice(&payload);
                        let mut trailers = hyper::HeaderMap::new();
                        trailers
                            .insert("grpc-status", hyper::header::HeaderValue::from_static("0"));
                        let frames: Vec<Result<Frame<Bytes>, Infallible>> = vec![
                            Ok(Frame::data(Bytes::from(frame))),
                            Ok(Frame::trailers(trailers)),
                        ];
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

#[tokio::test]
async fn a_unary_grpc_call_returns_what_the_server_sent() {
    let addr = grpc_server().await;
    let client = ClientBuilder::new(format!("http://{addr}"))
        .with_protocol_binding("GRPC")
        .build_grpc()
        .await
        .expect("client");
    let task = client
        .get_task(TaskQueryParams {
            tenant: None,
            id: "task-42".into(),
            history_length: None,
        })
        .await
        .expect("the server's task");
    assert_eq!(task, stored_task());
}
