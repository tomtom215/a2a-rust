// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `ClientBuilder::with_max_in_flight` caps the calls a built client has
//! waiting for a response, on the HTTP bindings and on gRPC (audit H6).
//!
//! The unit tests of the wrapper are in `transport/in_flight/tests.rs`; what
//! only a built client can show is that the builder puts the wrapper under
//! the client on every route that builds a transport.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use a2a_protocol_client::{ClientBuilder, ClientError};
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{EventEmitter, agent_executor};
use a2a_protocol_types::{Message, MessageSendParams, Part, TaskQueryParams, TaskState};

/// How many tasks the executor has started, for the capped test and for the
/// other (the tests share a process and each starts its own).
static CAPPED_STARTED: AtomicUsize = AtomicUsize::new(0);
static OPEN_STARTED: AtomicUsize = AtomicUsize::new(0);

/// Works until cancelled.
struct Sleeper;
agent_executor!(Sleeper, |ctx, queue| async {
    if ctx.message.text() == Some("capped") {
        CAPPED_STARTED.fetch_add(1, Ordering::SeqCst);
    } else {
        OPEN_STARTED.fetch_add(1, Ordering::SeqCst);
    }
    EventEmitter::new(ctx, queue)
        .status(TaskState::Working)
        .await?;
    ctx.cancellation_token.cancelled().await;
    Ok(())
});

async fn serve() -> String {
    let handler = Arc::new(
        RequestHandlerBuilder::new(Sleeper)
            .without_agent_card()
            .build()
            .expect("handler"),
    );
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler))
        .await
        .expect("serve");
    format!("http://{addr}")
}

async fn wait_for(mut done: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the first call reached the server");
}

fn refused(error: &ClientError, limit: usize) -> bool {
    matches!(error, ClientError::TooManyPendingRequests { limit: l } if *l == limit)
}

/// A blocking send to an executor that never finishes holds its place; a
/// second call is refused at once, and the place is given back when the first
/// is abandoned.
#[tokio::test]
async fn a_client_with_a_cap_of_one_refuses_the_second_call_while_the_first_waits() {
    let url = serve().await;
    let client = Arc::new(
        ClientBuilder::new(url)
            .with_max_in_flight(1)
            .build()
            .expect("client"),
    );
    let first = {
        let client = Arc::clone(&client);
        tokio::spawn(async move {
            client
                .send_message(MessageSendParams::new(Message::user(
                    "m-1",
                    vec![Part::text("capped")],
                )))
                .await
        })
    };
    // The send holds its place from the moment it is sent; the executor
    // starting shows the server has it.
    wait_for(|| CAPPED_STARTED.load(Ordering::SeqCst) >= 1).await;
    let mut last = String::new();
    let refusal = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match client.get_task(TaskQueryParams::new("nope")).await {
                Err(e) if refused(&e, 1) => return e,
                other => last = format!("{other:?}"),
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("refused while the send waits; last answer: {last}"));
    assert!(refused(&refusal, 1));

    first.abort();
    let _ = first.await;
    let after = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match client.get_task(TaskQueryParams::new("nope")).await {
                Err(e) if refused(&e, 1) => tokio::time::sleep(Duration::from_millis(20)).await,
                other => return other,
            }
        }
    })
    .await
    .expect("room again once the send is abandoned");
    assert!(
        !matches!(&after, Err(e) if refused(e, 1)),
        "no longer refused: {after:?}"
    );
}

#[tokio::test]
async fn a_client_without_a_cap_sends_every_call() {
    let url = serve().await;
    let client = Arc::new(ClientBuilder::new(url).build().expect("client"));
    let held: Vec<_> = (0..4)
        .map(|i| {
            let client = Arc::clone(&client);
            tokio::spawn(async move {
                client
                    .send_message(MessageSendParams::new(Message::user(
                        format!("m-{i}"),
                        vec![Part::text("sleep")],
                    )))
                    .await
            })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let read = client.get_task(TaskQueryParams::new("nope")).await;
    assert!(
        !matches!(&read, Err(e) if matches!(e, ClientError::TooManyPendingRequests { .. })),
        "{read:?}"
    );
    for h in held {
        h.abort();
    }
}

#[cfg(feature = "grpc")]
mod grpc {
    use std::convert::Infallible;

    use a2a_protocol_types::proto as pb;
    use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskStatus};
    use hyper::body::{Bytes, Frame};
    use prost::Message as _;
    use tokio::sync::Semaphore;

    use super::*;

    /// Answers `GetTask` with a task once the test opens its gate, counting
    /// the requests that arrived.
    async fn holding_server(
        gate: Arc<Semaphore>,
        arrived: Arc<AtomicUsize>,
    ) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let gate = Arc::clone(&gate);
                let arrived = Arc::clone(&arrived);
                tokio::spawn(async move {
                    let svc = hyper::service::service_fn(
                        move |req: hyper::Request<hyper::body::Incoming>| {
                            let gate = Arc::clone(&gate);
                            let arrived = Arc::clone(&arrived);
                            async move {
                                use http_body_util::BodyExt;
                                let _ = req.into_body().collect().await;
                                arrived.fetch_add(1, Ordering::SeqCst);
                                gate.acquire().await.expect("open").forget();
                                let task = Task::new(
                                    TaskId::new("task-1"),
                                    ContextId::new("ctx-1"),
                                    TaskStatus::new(TaskState::Completed),
                                );
                                let payload =
                                    pb::Task::try_from(task).expect("converts").encode_to_vec();
                                let mut frame = vec![0_u8];
                                frame.extend_from_slice(
                                    &u32::try_from(payload.len()).expect("len").to_be_bytes(),
                                );
                                frame.extend_from_slice(&payload);
                                let mut trailers = hyper::HeaderMap::new();
                                trailers.insert(
                                    "grpc-status",
                                    hyper::header::HeaderValue::from_static("0"),
                                );
                                let frames: Vec<Result<Frame<Bytes>, Infallible>> = vec![
                                    Ok(Frame::data(Bytes::from(frame))),
                                    Ok(Frame::trailers(trailers)),
                                ];
                                let body =
                                    http_body_util::StreamBody::new(tokio_stream::iter(frames));
                                let mut resp = hyper::Response::new(body);
                                resp.headers_mut().insert(
                                    "content-type",
                                    hyper::header::HeaderValue::from_static("application/grpc"),
                                );
                                Ok::<_, Infallible>(resp)
                            }
                        },
                    );
                    let _ = hyper::server::conn::http2::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    )
                    .serve_connection(hyper_util::rt::TokioIo::new(stream), svc)
                    .await;
                });
            }
        });
        addr
    }

    #[tokio::test]
    async fn the_cap_holds_on_a_client_built_for_grpc() {
        let gate = Arc::new(Semaphore::new(0));
        let arrived = Arc::new(AtomicUsize::new(0));
        let addr = holding_server(Arc::clone(&gate), Arc::clone(&arrived)).await;
        let client = Arc::new(
            ClientBuilder::new(format!("http://{addr}"))
                .with_protocol_binding("GRPC")
                .with_max_in_flight(1)
                .build_grpc()
                .await
                .expect("client"),
        );
        let first = {
            let client = Arc::clone(&client);
            tokio::spawn(async move { client.get_task(TaskQueryParams::new("task-1")).await })
        };
        wait_for(|| arrived.load(Ordering::SeqCst) >= 1).await;
        let mut last = String::new();
        let refusal = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match client.get_task(TaskQueryParams::new("task-1")).await {
                    Err(e) if refused(&e, 1) => return e,
                    other => last = format!("{other:?}"),
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("refused while the first call waits; last answer: {last}"));
        assert!(refused(&refusal, 1));

        gate.add_permits(1);
        first
            .await
            .expect("joined")
            .expect("the first call answers");
        gate.add_permits(1);
        client
            .get_task(TaskQueryParams::new("task-1"))
            .await
            .expect("room again");
    }

    #[tokio::test]
    async fn a_cap_of_zero_is_refused_before_anything_is_dialled() {
        let error = ClientBuilder::new("http://127.0.0.1:9")
            .with_protocol_binding("GRPC")
            .with_max_in_flight(0)
            .build_grpc()
            .await
            .expect_err("refused");
        assert!(
            matches!(&error, ClientError::Config(m) if m.contains("max_in_flight")),
            "{error:?}"
        );
    }
}
