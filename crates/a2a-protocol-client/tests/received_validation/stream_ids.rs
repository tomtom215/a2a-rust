// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! idx 274 (spec §9.4.2): each JSON-RPC stream frame should echo the
//! request's id. One that does not is warned about, once per stream, and
//! delivered: the HTTP response it arrives in binds it to the request.

use std::sync::{Arc, Mutex};

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_types::{Message, MessageRole, MessageSendParams, Part};

use super::peers::{self, Answer, Http};

#[derive(Clone, Default)]
struct Buf(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Buf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Buf {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// The events delivered and the log written while streaming from a peer
/// whose frames carry `frame_id`. Current-thread runtime, so the
/// thread-local subscriber sees the stream's events.
async fn stream_with(frame_id: Option<serde_json::Value>) -> (usize, String) {
    let working = |state: &str| serde_json::json!({"statusUpdate": {"taskId": "t", "contextId": "c", "status": {"state": state}}});
    let peer = peers::http(
        Http::JsonRpc,
        Answer {
            events: vec![
                working("TASK_STATE_WORKING"),
                working("TASK_STATE_COMPLETED"),
            ],
            frame_id,
            ..Answer::default()
        },
    )
    .await;
    let buf = Buf::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(buf.clone())
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let client = ClientBuilder::new(&peer.url).build().expect("client");
    let params =
        MessageSendParams::new(Message::new("m", MessageRole::User, vec![Part::text("hi")]));
    let mut stream = client.stream_message(params).await.expect("stream");
    let mut delivered = 0;
    while let Some(event) = stream.next().await {
        event.expect("every frame is delivered");
        delivered += 1;
    }
    let log = String::from_utf8_lossy(&buf.0.lock().unwrap()).into_owned();
    (delivered, log)
}

#[tokio::test]
async fn a_stream_frame_with_another_id_is_warned_about_once_and_delivered() {
    let (delivered, log) = stream_with(Some(serde_json::json!("not-the-request-id"))).await;
    assert_eq!(delivered, 2);
    assert_eq!(log.matches("stream frame id mismatch").count(), 1, "{log}");
    assert!(log.contains("not-the-request-id"), "{log}");
}

#[tokio::test]
async fn a_stream_frame_echoing_the_request_id_is_not_warned_about() {
    let (delivered, log) = stream_with(None).await;
    assert_eq!(delivered, 2);
    assert!(!log.contains("stream frame id mismatch"), "{log}");
}
