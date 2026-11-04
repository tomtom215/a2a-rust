// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The pool's connect timeout bounds a connect that never completes, for
//! every client given the pool (ADR 0032, decision 1).
//!
//! A listener whose backlog is full and which never accepts leaves a new
//! connect unanswered: on Linux the kernel drops the SYN rather than refuse
//! it. The backlog is 0 here and filled before the client dials.

#![cfg(target_os = "linux")]

use std::time::{Duration, Instant};

use a2a_protocol_client::{ClientBuilder, HttpPool};
use a2a_protocol_types::params::TaskQueryParams;

/// A listening address nothing will answer. The listener and the connections
/// that fill its queue are returned so they stay open.
async fn unanswered() -> (
    tokio::net::TcpListener,
    Vec<tokio::net::TcpStream>,
    std::net::SocketAddr,
) {
    let socket = tokio::net::TcpSocket::new_v4().expect("socket");
    socket
        .bind("127.0.0.1:0".parse().expect("addr"))
        .expect("bind");
    let listener = socket.listen(0).expect("listen");
    let addr = listener.local_addr().expect("addr");
    let mut fillers = Vec::new();
    for _ in 0..16 {
        match tokio::time::timeout(
            Duration::from_millis(200),
            tokio::net::TcpStream::connect(addr),
        )
        .await
        {
            Ok(Ok(s)) => fillers.push(s),
            _ => break,
        }
    }
    (listener, fillers, addr)
}

#[tokio::test]
async fn a_connect_that_never_completes_fails_at_the_pools_connect_timeout() {
    const BOUND: Duration = Duration::from_millis(400);
    let (_listener, _fillers, addr) = unanswered().await;
    let pool = HttpPool::builder()
        .with_connect_timeout(BOUND)
        .build()
        .expect("pool");

    for binding in ["JSONRPC", "HTTP+JSON"] {
        let client = ClientBuilder::new(format!("http://{addr}"))
            .with_protocol_binding(binding)
            .with_pool(&pool)
            .with_timeout(Duration::from_secs(30))
            .build()
            .expect("client");
        let started = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(20),
            client.get_task(TaskQueryParams::new("t")),
        )
        .await
        .expect("the call ended on its own, well before the 30 s request timeout");
        let elapsed = started.elapsed();
        let err = result.expect_err("nothing answered");
        assert!(
            elapsed >= BOUND.mul_f32(0.8) && elapsed < Duration::from_secs(5),
            "{binding}: ended after {elapsed:?} with {err}; the connect should have \
             been cut off at about {BOUND:?}"
        );
    }
}
