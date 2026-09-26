// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Every binding against every hostile script (audit escape class 6, E6).
//!
//! Until 2026-09-23 the stall, cut-off and mis-frame tests lived one stub per
//! test file, for JSON-RPC and HTTP+JSON only; nothing produced those failures
//! over WebSocket or gRPC. Each test here runs one script against one binding
//! and asserts what the client must do about it: give up on a silent peer at
//! its bound, report a stream that ended early as incomplete, report a frame
//! that does not parse as an error rather than hang on it, and surface a
//! refusal as an authentication failure a token provider can act on.
//!
//! Every wait is bounded by `GUARD`, far past every configured bound, so a
//! regression fails here instead of hanging the suite.

#![cfg(feature = "testing")]

use std::time::Duration;

use a2a_protocol_client::testing::{Binding, RunningPeer, ScriptedPeer};
use a2a_protocol_client::{A2aClient, ClientBuilder, ClientError, EventStream};
use a2a_protocol_types::{Message, MessageRole, MessageSendParams, Part};

const GUARD: Duration = Duration::from_secs(20);
const IDLE: Duration = Duration::from_millis(300);

fn params() -> MessageSendParams {
    MessageSendParams::new(Message::new("m", MessageRole::User, vec![Part::text("hi")]))
}

async fn client(binding: Binding, url: &str) -> ClientResult {
    let builder = ClientBuilder::new(url)
        .with_timeout(IDLE)
        .with_stream_idle_timeout(Some(IDLE));
    match binding {
        Binding::JsonRpc => builder.build(),
        Binding::Rest => builder.with_protocol_binding("HTTP+JSON").build(),
        #[cfg(feature = "websocket")]
        Binding::WebSocket => {
            // The WebSocket transport carries its own request bound (30 s by
            // default), which the builder's `with_timeout` does not reach.
            let config = a2a_protocol_client::transport::WebSocketTransportConfig::default()
                .with_request_timeout(IDLE);
            let transport =
                a2a_protocol_client::transport::WebSocketTransport::connect_with_config(
                    url, config,
                )
                .await?;
            builder.with_custom_transport(transport).build()
        }
        #[cfg(feature = "grpc")]
        Binding::Grpc => builder.with_protocol_binding("GRPC").build_grpc().await,
        _ => unreachable!("every binding this build speaks is listed"),
    }
}

type ClientResult = Result<A2aClient, ClientError>;

fn bindings() -> Vec<Binding> {
    vec![
        Binding::JsonRpc,
        Binding::Rest,
        #[cfg(feature = "websocket")]
        Binding::WebSocket,
        #[cfg(feature = "grpc")]
        Binding::Grpc,
    ]
}

async fn next(stream: &mut EventStream) -> Option<Result<(), ClientError>> {
    tokio::time::timeout(GUARD, stream.next())
        .await
        .expect("the client must not hang past the guard")
        .map(|r| r.map(|_| ()))
}

/// Opens a stream against `peer` and returns it, with the running peer it
/// must not outlive, after its first event.
async fn stream_after_one(binding: Binding, peer: ScriptedPeer) -> (RunningPeer, EventStream) {
    let peer = peer.on(binding).start().await.expect("peer");
    let client = client(binding, peer.url()).await.expect("client");
    let mut stream = tokio::time::timeout(GUARD, client.stream_message(params()))
        .await
        .expect("stream opens within the guard")
        .expect("stream");
    assert!(
        matches!(next(&mut stream).await, Some(Ok(()))),
        "{binding:?}: the first event arrives"
    );
    (peer, stream)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stream_that_stalls_ends_at_the_idle_bound() {
    let mut wrong = Vec::new();
    for binding in bindings() {
        let (_peer, mut stream) =
            stream_after_one(binding, ScriptedPeer::new().stall_after(1)).await;
        match next(&mut stream).await {
            Some(Err(ClientError::Timeout(msg))) if msg.contains("idle") => {}
            other => wrong.push(format!(
                "{binding:?}: expected an idle Timeout, got {other:?}"
            )),
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A peer that vanishes mid-stream is the textbook transient failure, and a
/// caller writing binding-agnostic retry or resume logic needs it to read the
/// same everywhere. The HTTP bindings see a connection cut mid-body
/// (`Http`); WebSocket and gRPC see the stream end before its final event
/// (`IncompleteStream`). Both are retryable. Until 2026-09-23 the WebSocket
/// cut was a `Transport` error and the gRPC one a `Protocol(InternalError)`,
/// neither of them retryable (audit N13).
#[tokio::test(flavor = "multi_thread")]
async fn a_stream_cut_off_before_its_final_event_is_a_retryable_error() {
    let mut wrong = Vec::new();
    for binding in bindings() {
        let (_peer, mut stream) =
            stream_after_one(binding, ScriptedPeer::new().cut_off_after(1)).await;
        // The HTTP bindings see the connection cut mid-body; WebSocket and
        // gRPC see the stream end before its final event. Both retryable.
        let observed_end = !matches!(binding, Binding::JsonRpc | Binding::Rest);
        match next(&mut stream).await {
            Some(Err(e @ ClientError::IncompleteStream { .. })) if observed_end => {
                assert!(e.is_retryable());
            }
            Some(Err(e @ ClientError::Http(_))) if !observed_end => assert!(e.is_retryable()),
            other => wrong.push(format!(
                "{binding:?}: expected {}, got {other:?}",
                if observed_end {
                    "IncompleteStream"
                } else {
                    "a retryable Http error"
                }
            )),
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_frame_that_does_not_parse_is_an_error_not_a_timeout() {
    let mut wrong = Vec::new();
    for binding in bindings() {
        let (_peer, mut stream) =
            stream_after_one(binding, ScriptedPeer::new().misframe_after(1)).await;
        match next(&mut stream).await {
            Some(Err(ClientError::Timeout(msg))) => {
                wrong.push(format!(
                    "{binding:?}: a malformed frame was reported as a timeout: {msg}"
                ));
            }
            Some(Err(_)) => {}
            other => wrong.push(format!("{binding:?}: expected an error, got {other:?}")),
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_call_reads_as_an_authentication_failure() {
    let mut wrong = Vec::new();
    for binding in bindings() {
        let peer = ScriptedPeer::new()
            .unauthorized()
            .on(binding)
            .start()
            .await
            .expect("peer");
        let outcome = match client(binding, peer.url()).await {
            Err(e) => Err(e),
            Ok(c) => tokio::time::timeout(GUARD, c.send_message(params()))
                .await
                .expect("a refusal is prompt")
                .map(|_| ()),
        };
        match outcome {
            Err(ClientError::UnexpectedStatus { status: 401, .. }) => {}
            other => wrong.push(format!(
                "{binding:?}: expected UnexpectedStatus 401, got {other:?}"
            )),
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn unary_calls_give_up_on_each_script() {
    let mut wrong = Vec::new();
    for binding in bindings() {
        for (name, peer) in [
            ("stall", ScriptedPeer::new().stall_after(0)),
            ("cut-off", ScriptedPeer::new().cut_off_after(0)),
            ("mis-frame", ScriptedPeer::new().misframe_after(0)),
        ] {
            let peer = peer.on(binding).start().await.expect("peer");
            let client = client(binding, peer.url()).await.expect("client");
            let Ok(outcome) = tokio::time::timeout(GUARD, client.send_message(params())).await
            else {
                wrong.push(format!("{binding:?} {name}: still waiting after {GUARD:?}"));
                continue;
            };
            match (name, outcome) {
                ("stall", Err(ClientError::Timeout(_))) => {}
                ("cut-off", Err(e))
                    if e.is_retryable() && !matches!(e, ClientError::Timeout(_)) => {}
                ("mis-frame", Err(e)) if !matches!(e, ClientError::Timeout(_)) => {}
                (name, other) => wrong.push(format!(
                    "{binding:?} {name}: expected {}, got {other:?}",
                    match name {
                        "stall" => "a Timeout",
                        "cut-off" => "a retryable error other than a timeout",
                        _ => "an error other than a timeout",
                    }
                )),
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The peer counts the connections it accepts, and stops listening when
/// dropped — both part of its public contract, and both what a test uses to
/// tell "the client never called" from "the client gave up".
#[tokio::test(flavor = "multi_thread")]
async fn the_peer_counts_connections_and_stops_when_dropped() {
    let peer = ScriptedPeer::new()
        .cut_off_after(0)
        .start()
        .await
        .expect("peer");
    let addr = peer.url().trim_start_matches("http://").to_owned();
    assert_eq!(peer.connections(), 0);
    for expected in 1..=2 {
        let _ = tokio::net::TcpStream::connect(&addr)
            .await
            .expect("the peer accepts");
        let deadline = tokio::time::Instant::now() + GUARD;
        while peer.connections() < expected && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert_eq!(peer.connections(), expected);
    }
    drop(peer);
    let deadline = tokio::time::Instant::now() + GUARD;
    loop {
        if tokio::net::TcpStream::connect(&addr).await.is_err() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "a dropped peer still accepts"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

/// A request whose body arrives in a later TCP segment than its head — as a
/// client that writes a large body separately sends it — is read whole before
/// the script answers, rather than sliced short.
#[tokio::test(flavor = "multi_thread")]
async fn the_peer_reads_a_body_that_arrives_after_its_head() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let peer = ScriptedPeer::new()
        .unauthorized()
        .start()
        .await
        .expect("peer");
    let addr = peer.url().trim_start_matches("http://").to_owned();
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{}}"#;
    let head = format!(
        "POST / HTTP/1.1\r\nhost: peer\r\ncontent-type: application/json\r\n\
         content-length: {}\r\n\r\n",
        body.len()
    );
    // All but the body's last five bytes, then — once the peer has read
    // that — the rest.
    let (first, rest) = body.split_at(body.len() - 5);
    let mut socket = tokio::net::TcpStream::connect(&addr)
        .await
        .expect("the peer accepts");
    socket
        .write_all(format!("{head}{first}").as_bytes())
        .await
        .expect("write head");
    tokio::time::sleep(Duration::from_millis(100)).await;
    socket.write_all(rest.as_bytes()).await.expect("write rest");

    let mut answer = Vec::new();
    tokio::time::timeout(GUARD, socket.read_to_end(&mut answer))
        .await
        .expect("the peer answers within the guard")
        .expect("read");
    let answer = String::from_utf8_lossy(&answer);
    assert!(
        answer.starts_with("HTTP/1.1 401"),
        "expected the scripted 401, got {answer:?}"
    );
}

/// A call made after the transport's socket dropped reconnects (audit N18)
/// rather than being refused: the call in flight when the socket dropped is
/// retryable (N13), and the next one opens a new connection. Against a peer
/// that cuts every connection, that call fails too — as retryable, with a
/// second connection made — which is what a retry loop needs to be told.
/// It used to fail at once, non-retryable, with no connection attempted.
#[cfg(feature = "websocket")]
#[tokio::test(flavor = "multi_thread")]
async fn a_call_after_a_dropped_websocket_reconnects() {
    let peer = ScriptedPeer::new()
        .cut_off_after(0)
        .on(Binding::WebSocket)
        .start()
        .await
        .expect("peer");
    let client = client(Binding::WebSocket, peer.url())
        .await
        .expect("client");
    let in_flight = tokio::time::timeout(GUARD, client.send_message(params()))
        .await
        .expect("the drop is prompt");
    assert!(
        matches!(&in_flight, Err(e) if e.is_retryable()),
        "the call the drop interrupted: {in_flight:?}"
    );
    let before = peer.connections();

    let next = tokio::time::timeout(GUARD, client.send_message(params()))
        .await
        .expect("the reconnect is bounded");
    assert!(
        peer.connections() > before,
        "no new connection was made for the call after the drop"
    );
    assert!(
        next.as_ref().is_err_and(ClientError::is_retryable),
        "a call cut off again must be retryable: {next:?}"
    );
}
