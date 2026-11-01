// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The socket options a tuned connector gives the connections it opens
//! (audit B-30: "no test pins it").

#[cfg(feature = "tls-rustls")]
use hyper_util::client::legacy::connect::HttpConnector;
#[cfg(feature = "tls-rustls")]
use hyper_util::rt::TokioIo;
#[cfg(feature = "tls-rustls")]
use tower_service::Service;

use super::*;

/// What a connection opened by `tcp` to a local listener has set.
#[cfg(feature = "tls-rustls")]
async fn opened_by(mut tcp: HttpConnector) -> (bool, bool) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let uri: hyper::Uri = format!("http://{}/", listener.local_addr().unwrap())
        .parse()
        .unwrap();
    let accept = tokio::spawn(async move { listener.accept().await });
    let io: TokioIo<tokio::net::TcpStream> = tcp.call(uri).await.expect("connects");
    let _ = accept.await;
    let stream = io.into_inner();
    let nodelay = stream.nodelay().unwrap();
    let socket = tokio::net::TcpSocket::from_std_stream(stream.into_std().unwrap());
    (nodelay, socket.keepalive().unwrap())
}

/// A tuned connector sets `TCP_NODELAY` and `SO_KEEPALIVE`; an untuned one
/// sets neither, which is what the control shows: the options are the
/// function's doing and not the platform's default.
#[cfg(feature = "tls-rustls")]
#[tokio::test]
async fn tune_sets_nodelay_and_keepalive_and_the_default_sets_neither() {
    assert_eq!(opened_by(HttpConnector::new()).await, (false, false));

    let mut tuned = HttpConnector::new();
    tune(&mut tuned);
    assert_eq!(opened_by(tuned).await, (true, true));
}

/// The values the book and the audit quote: a 10 s connect, a 30 s keepalive
/// and a pool that lets go of a connection after 50 s, below the 60 s idle
/// timeout of the load balancers in front of a receiver.
#[test]
fn the_bounds_are_the_documented_ones() {
    assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(10));
    assert_eq!(TCP_KEEPALIVE, Duration::from_secs(30));
    assert_eq!(POOL_IDLE_TIMEOUT, Duration::from_secs(50));
    assert!(POOL_IDLE_TIMEOUT < Duration::from_secs(60));
}
