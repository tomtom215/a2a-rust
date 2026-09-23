// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The gRPC binding of the scripted peer: `lf.a2a.v1.A2AService` over HTTP/2,
//! with the length-prefixed protobuf frames written by hand so a malformed
//! one, or a stream reset mid-body, is as easy to send as a good one.

use std::convert::Infallible;

use hyper::body::{Bytes, Frame};
use hyper::{Request, Response};
use prost::Message as _;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::proto as pb;
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

use super::{Script, hold_open};

type BodyFrame = Result<Frame<Bytes>, std::io::Error>;

/// One length-prefixed gRPC message: flag 0 (uncompressed), a big-endian
/// length, then the bytes.
fn grpc_frame(payload: &[u8]) -> Bytes {
    let len = u32::try_from(payload.len()).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(5 + payload.len());
    out.push(0);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(payload);
    Bytes::from(out)
}

fn working_frame() -> Bytes {
    let event = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: TaskId::new("t"),
        context_id: ContextId::new("c"),
        status: TaskStatus::new(TaskState::Working),
        metadata: None,
    });
    let encoded = pb::StreamResponse::try_from(event)
        .map(|m| m.encode_to_vec())
        .unwrap_or_default();
    grpc_frame(&encoded)
}

type Body = http_body_util::StreamBody<tokio_stream::wrappers::ReceiverStream<BodyFrame>>;

fn header(resp: &mut Response<Body>, name: &'static str, value: &'static str) {
    resp.headers_mut()
        .insert(name, hyper::header::HeaderValue::from_static(value));
}

fn respond(script: Script, req: &Request<hyper::body::Incoming>) -> Response<Body> {
    let (tx, rx) = mpsc::channel::<BodyFrame>(16);
    let mut resp = Response::new(http_body_util::StreamBody::new(
        tokio_stream::wrappers::ReceiverStream::new(rx),
    ));
    header(&mut resp, "content-type", "application/grpc");
    if script == Script::Unauthorized {
        // Trailers-only: the status travels in the headers, with no body.
        header(&mut resp, "grpc-status", "16");
        header(&mut resp, "grpc-message", "unauthorized");
        return resp;
    }
    let streaming = req.uri().path().ends_with("/SendStreamingMessage")
        || req.uri().path().ends_with("/SubscribeToTask");
    let after = match script {
        Script::Stall { after } | Script::CutOff { after } | Script::MisFrame { after } => after,
        Script::Unauthorized => 0,
    };
    tokio::spawn(async move {
        if streaming {
            for _ in 0..after {
                if tx.send(Ok(Frame::data(working_frame()))).await.is_err() {
                    return;
                }
            }
        }
        match script {
            Script::CutOff { .. } => {
                // The response body simply ends: END_STREAM with no trailers,
                // so no `grpc-status` — what a truncating proxy or a crashed
                // server leaves behind. (A body error would reset the stream
                // instead, and h2 may discard data still queued ahead of the
                // reset, so the events before it would not reliably arrive.)
            }
            Script::MisFrame { .. } => {
                // A frame whose payload is not a protobuf message: field 1,
                // wire type 7, which does not exist.
                let _ = tx
                    .send(Ok(Frame::data(grpc_frame(&[0x0f, 0xff, 0xff]))))
                    .await;
                hold_open().await;
            }
            _ => hold_open().await,
        }
        drop(tx);
    });
    resp
}

pub(super) async fn serve(script: Script, stream: TcpStream) {
    let svc = hyper::service::service_fn(move |req: Request<hyper::body::Incoming>| async move {
        Ok::<_, Infallible>(respond(script, &req))
    });
    let _ = hyper::server::conn::http2::Builder::new(hyper_util::rt::TokioExecutor::new())
        .serve_connection(hyper_util::rt::TokioIo::new(stream), svc)
        .await;
}
