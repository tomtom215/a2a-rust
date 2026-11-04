// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The gRPC client sends HTTP/2 PINGs only when asked to, and drops a
//! connection whose PINGs go unanswered (audit B-10, D-12).
//!
//! A proxy sits between the client and a hand-written HTTP/2 server. It reads
//! the client's byte stream as HTTP/2 frames and counts the PINGs (frame type
//! 6, ACK flag clear), notes when the client closes the connection, and can
//! stop forwarding what the server sends, so that a PING goes unanswered the
//! way it does when a peer vanishes.

#![cfg(feature = "grpc")]

use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_client::transport::grpc::GrpcTransportConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// What the proxy saw.
#[derive(Default)]
struct Seen {
    pings: AtomicUsize,
    client_closed: AtomicUsize,
    /// While set, what the server sends is not passed on.
    blackhole: AtomicBool,
}

/// Answers every request with an empty, successful gRPC response.
async fn grpc_server() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let svc = hyper::service::service_fn(
                    |_req: hyper::Request<hyper::body::Incoming>| async {
                        let mut trailers = hyper::HeaderMap::new();
                        trailers
                            .insert("grpc-status", hyper::header::HeaderValue::from_static("0"));
                        let frames: Vec<
                            Result<hyper::body::Frame<hyper::body::Bytes>, Infallible>,
                        > = vec![Ok(hyper::body::Frame::trailers(trailers))];
                        let mut resp = hyper::Response::new(http_body_util::StreamBody::new(
                            tokio_stream::iter(frames),
                        ));
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

const PREFACE: usize = 24;

/// Counts the PING frames in the client's bytes as they arrive.
struct Frames {
    buf: Vec<u8>,
    preface_skipped: bool,
}

impl Frames {
    fn pings_in(&mut self, bytes: &[u8]) -> usize {
        self.buf.extend_from_slice(bytes);
        if !self.preface_skipped {
            if self.buf.len() < PREFACE {
                return 0;
            }
            self.buf.drain(..PREFACE);
            self.preface_skipped = true;
        }
        let mut pings = 0;
        while self.buf.len() >= 9 {
            let len = (usize::from(self.buf[0]) << 16)
                | (usize::from(self.buf[1]) << 8)
                | usize::from(self.buf[2]);
            if self.buf.len() < 9 + len {
                break;
            }
            let (kind, flags) = (self.buf[3], self.buf[4]);
            if kind == 6 && flags & 1 == 0 {
                pings += 1;
            }
            self.buf.drain(..9 + len);
        }
        pings
    }
}

async fn proxy(upstream: std::net::SocketAddr, seen: Arc<Seen>) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        while let Ok((client, _)) = listener.accept().await {
            let Ok(server) = TcpStream::connect(upstream).await else {
                continue;
            };
            let (mut client_read, mut client_write) = client.into_split();
            let (mut server_read, mut server_write) = server.into_split();
            let up = Arc::clone(&seen);
            tokio::spawn(async move {
                let mut frames = Frames {
                    buf: Vec::new(),
                    preface_skipped: false,
                };
                let mut chunk = [0_u8; 16_384];
                loop {
                    match client_read.read(&mut chunk).await {
                        Ok(0) | Err(_) => {
                            up.client_closed.fetch_add(1, Ordering::SeqCst);
                            return;
                        }
                        Ok(n) => {
                            up.pings
                                .fetch_add(frames.pings_in(&chunk[..n]), Ordering::SeqCst);
                            if server_write.write_all(&chunk[..n]).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            });
            let down = Arc::clone(&seen);
            tokio::spawn(async move {
                let mut chunk = [0_u8; 16_384];
                while let Ok(n) = server_read.read(&mut chunk).await {
                    if n == 0 {
                        return;
                    }
                    if !down.blackhole.load(Ordering::SeqCst)
                        && client_write.write_all(&chunk[..n]).await.is_err()
                    {
                        return;
                    }
                }
            });
        }
    });
    addr
}

async fn connect(
    via: std::net::SocketAddr,
    keep_alive: Option<(Duration, Duration)>,
) -> a2a_protocol_client::A2aClient {
    let mut builder = ClientBuilder::new(format!("http://{via}")).with_protocol_binding("GRPC");
    if let Some((every, timeout)) = keep_alive {
        builder = builder.with_grpc_keep_alive(every, timeout);
    }
    builder.build_grpc().await.expect("client")
}

#[tokio::test]
async fn a_client_asked_to_keep_alive_pings_an_idle_connection() {
    let seen = Arc::new(Seen::default());
    let via = proxy(grpc_server().await, Arc::clone(&seen)).await;
    let _client = connect(
        via,
        Some((Duration::from_millis(100), Duration::from_secs(5))),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    let pings = seen.pings.load(Ordering::SeqCst);
    assert!(pings >= 4, "{pings} pings in 800 ms at one every 100 ms");
    assert_eq!(seen.client_closed.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn by_default_a_quiet_connection_carries_no_pings() {
    let seen = Arc::new(Seen::default());
    let via = proxy(grpc_server().await, Arc::clone(&seen)).await;
    let _client = connect(via, None).await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(seen.pings.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_connection_whose_pings_go_unanswered_is_dropped_after_the_timeout() {
    let seen = Arc::new(Seen::default());
    let via = proxy(grpc_server().await, Arc::clone(&seen)).await;
    let _client = connect(
        via,
        Some((Duration::from_millis(100), Duration::from_millis(300))),
    )
    .await;
    seen.blackhole.store(true, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(10), async {
        while seen.client_closed.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the client dropped the connection whose pings nobody answered");
}

#[tokio::test]
async fn without_keep_alive_a_silent_peer_is_not_noticed() {
    let seen = Arc::new(Seen::default());
    let via = proxy(grpc_server().await, Arc::clone(&seen)).await;
    let _client = connect(via, None).await;
    seen.blackhole.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(seen.client_closed.load(Ordering::SeqCst), 0);
}

#[test]
fn the_config_is_off_by_default_and_the_setter_sets_both_values() {
    let default = GrpcTransportConfig::default();
    assert_eq!(default.http2_keep_alive_interval, None);
    assert_eq!(default.http2_keep_alive_timeout, Duration::from_secs(20));
    let set = default.with_http2_keep_alive(Duration::from_secs(7), Duration::from_secs(3));
    assert_eq!(set.http2_keep_alive_interval, Some(Duration::from_secs(7)));
    assert_eq!(set.http2_keep_alive_timeout, Duration::from_secs(3));
}
