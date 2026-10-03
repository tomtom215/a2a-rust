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
use a2a_protocol_server::handler::HandlerLimits;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::jsonrpc::JsonRpcRequest;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

/// The message id of the request the stalled peer sends.
const FLOOD_MESSAGE_ID: &str = "msg-flood";

/// Answers the stalled peer's request with artifact chunks of `chunk_bytes`
/// each, without end, until the task is cancelled or a write fails; answers
/// any other request by completing at once.
///
/// Without end because no fixed burst is guaranteed to outrun the peer's
/// buffers: Windows loopback autotuning grows them past what the 4 KiB
/// receive buffer asks for, and while the server's writes keep succeeding
/// the connection is not idle. An endless stream fills any buffer, so the
/// server's send is sure to block, which is the case under test.
struct FloodExecutor {
    chunk_bytes: usize,
}

impl AgentExecutor for FloodExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            if ctx.message.id.as_ref() == FLOOD_MESSAGE_ID {
                let text = "x".repeat(self.chunk_bytes);
                let mut first = true;
                while !ctx.cancellation_token.is_cancelled() {
                    queue
                        .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                            task_id: ctx.task_id.clone(),
                            context_id: ContextId::new(ctx.context_id.clone()),
                            artifact: a2a_protocol_types::artifact::Artifact::new(
                                "flood",
                                vec![Part::text(text.clone())],
                            ),
                            append: Some(!first),
                            last_chunk: Some(false),
                            metadata: None,
                        }))
                        .await?;
                    first = false;
                    // The test runtime has one thread, shared with the
                    // server; a write that never waits must not starve it.
                    tokio::task::yield_now().await;
                }
                return Ok(());
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
    // The stream never ends, so cap what the store keeps of it: appends past
    // the cap are still streamed, only not persisted.
    let handler = Arc::new(
        RequestHandlerBuilder::new(FloodExecutor {
            chunk_bytes: 128 * 1024,
        })
        .with_handler_limits(HandlerLimits::default().with_max_parts_per_artifact(16))
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

    // And the stalled peer's connection was closed, not merely parked.
    assert_closed(&mut stalled).await;
}

/// Reads what the server managed to send a stalled peer, asserting the
/// connection then ends.
///
/// The stream is endless, so a server still sending to this peer — one that
/// parked the stream rather than closing the connection — would keep it
/// supplied once reading resumed, and it would never end.
async fn assert_closed(stalled: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>) {
    let drained = tokio::time::timeout(Duration::from_secs(20), async {
        while let Some(Ok(_)) = stalled.next().await {}
    })
    .await;
    assert!(
        drained.is_ok(),
        "the server was still streaming to a peer it should have closed as idle"
    );
}
