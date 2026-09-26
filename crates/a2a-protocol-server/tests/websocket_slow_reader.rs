// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A WebSocket peer that stops reading its socket (N30).
//!
//! `WebSocketDispatcher::with_idle_timeout` says what it is for: at half the
//! budget the server pings, and "only a client that has stopped reading its
//! socket, or gone away without a close frame, fails to answer" — so the
//! bound closes it. Against a peer that stopped reading *while being
//! streamed to*, it could not. The stream's send blocked on the full socket
//! while holding the sink lock; the keepalive waited for that lock to send
//! its ping, so the idle check never ran again; and once the read loop did
//! end, closing the connection took the same lock. The connection, its task
//! and its `max_connections` slot were held for as long as the peer chose.

#![cfg(feature = "websocket")]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message as WsMessage;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::WebSocketDispatcher;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::jsonrpc::JsonRpcRequest;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

/// Streams `chunks` artifact chunks of `chunk_bytes` each, then completes:
/// far more than a socket's buffers hold.
struct FloodExecutor {
    chunks: usize,
    chunk_bytes: usize,
}

impl AgentExecutor for FloodExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let text = "x".repeat(self.chunk_bytes);
            for i in 0..self.chunks {
                queue
                    .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        artifact: a2a_protocol_types::artifact::Artifact::new(
                            "flood",
                            vec![Part::text(text.clone())],
                        ),
                        append: Some(i > 0),
                        last_chunk: Some(i + 1 == self.chunks),
                        metadata: None,
                    }))
                    .await?;
            }
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Completed),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

fn request(id: &str, method: &str) -> WsMessage {
    let params = MessageSendParams {
        tenant: None,
        message: Message {
            id: MessageId::new(format!("msg-{id}")),
            role: MessageRole::User,
            parts: vec![Part::text("go")],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    };
    let rpc = JsonRpcRequest::with_params(
        serde_json::json!(id),
        method,
        serde_json::to_value(params).expect("params"),
    );
    WsMessage::Text(serde_json::to_string(&rpc).expect("rpc").into())
}

fn upgrade(
    addr: std::net::SocketAddr,
) -> tokio_tungstenite::tungstenite::handshake::client::Request {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
    let mut req = format!("ws://{addr}")
        .into_client_request()
        .expect("ws url");
    req.headers_mut()
        .insert("a2a-version", "1.0".parse().expect("header value"));
    req
}

/// A peer streamed to that stops reading is closed by the idle bound, and
/// its connection slot goes back: with a ceiling of one connection, a second
/// client is served.
#[tokio::test]
async fn a_peer_that_stops_reading_mid_stream_is_closed_by_the_idle_bound() {
    let idle = Duration::from_secs(1);
    let handler = Arc::new(
        RequestHandlerBuilder::new(FloodExecutor {
            chunks: 96,
            chunk_bytes: 128 * 1024,
        })
        .build()
        .expect("build handler"),
    );
    let addr = Arc::new(
        WebSocketDispatcher::new(handler)
            .with_max_connections(1)
            .with_idle_timeout(idle),
    )
    .serve_with_addr("127.0.0.1:0")
    .await
    .expect("start WS server");

    // The stalled peer: a small receive buffer, a stream opened, and then no
    // more reading. Kept alive for the whole test, so its socket stays open.
    let socket = tokio::net::TcpSocket::new_v4().expect("socket");
    socket.set_recv_buffer_size(4096).expect("rcvbuf");
    let tcp = socket.connect(addr).await.expect("connect");
    let (mut stalled, _) = tokio_tungstenite::client_async(upgrade(addr), tcp)
        .await
        .expect("handshake");
    stalled
        .send(request("flood", "SendStreamingMessage"))
        .await
        .expect("send");

    // Several idle budgets with the peer reading nothing.
    tokio::time::sleep(idle * 4).await;

    // The only slot must be free again.
    // Well inside the default 10 s handshake bound, so a close that waited
    // that long for a peer that cannot read would fail this too.
    let served = tokio::time::timeout(Duration::from_secs(5), async {
        let (mut ws, _) = tokio_tungstenite::connect_async(upgrade(addr)).await?;
        ws.send(request("second", "SendMessage")).await?;
        ws.next().await.transpose().map(|frame| frame.is_some())
    })
    .await;
    assert!(
        matches!(served, Ok(Ok(true))),
        "a second client was not served while a peer that stopped reading held the only \
         connection slot: {served:?}"
    );

    // And the stalled peer's stream was abandoned, not merely parked. Not on
    // Windows: whether a 12 MB burst overruns the peer's buffers there depends
    // on loopback autotuning the 4 KiB receive buffer does not bound, and a
    // server that finished the stream into buffers that took it all has done
    // nothing wrong. The slot assertion above holds everywhere.
    #[cfg(not(windows))]
    assert_abandoned(&mut stalled).await;
}

/// Reads what the server managed to send a stalled peer, asserting the
/// stream never finished.
#[cfg(not(windows))]
async fn assert_abandoned(stalled: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>) {
    // A task still blocked in its send would deliver everything once reading
    // resumed, completion included, having held the socket all along.
    let mut finished = false;
    while let Ok(Some(Ok(frame))) =
        tokio::time::timeout(Duration::from_secs(10), stalled.next()).await
    {
        if let WsMessage::Text(text) = frame
            && (text.contains("TASK_STATE_COMPLETED") || text.contains("stream_complete"))
        {
            finished = true;
        }
    }
    assert!(
        !finished,
        "the server finished streaming to a peer it had closed as idle"
    );
}
