// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Peers that answer every call with one fixed object, on each binding, and
//! record what each call sent.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// What a peer answers: `unary` to every unary call, `events` (then the end
/// of the stream) to every streaming one.
#[derive(Clone, Default)]
pub struct Answer {
    pub unary: serde_json::Value,
    pub events: Vec<serde_json::Value>,
}

/// A running peer: its URL, and each request's target and body.
#[allow(dead_code)] // `seen` is for the wire-form tests
pub struct Peer {
    pub url: String,
    pub seen: Arc<Mutex<Vec<String>>>,
}

#[allow(dead_code)] // for the wire-form tests
impl Peer {
    pub fn last_request(&self) -> String {
        self.seen
            .lock()
            .unwrap()
            .last()
            .cloned()
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Http {
    JsonRpc,
    Rest,
}

async fn listen() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    (listener, addr.to_string())
}

/// A JSON-RPC or HTTP+JSON peer over HTTP/1.1.
pub async fn http(flavour: Http, answer: Answer) -> Peer {
    let (listener, addr) = listen().await;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(serve_http(
                flavour,
                answer.clone(),
                stream,
                Arc::clone(&log),
            ));
        }
    });
    Peer {
        url: format!("http://{addr}"),
        seen,
    }
}

async fn serve_http(
    flavour: Http,
    answer: Answer,
    mut stream: TcpStream,
    log: Arc<Mutex<Vec<String>>>,
) {
    let mut acc = Vec::new();
    let mut buf = [0_u8; 8192];
    let (target, body) = loop {
        let Ok(n) = stream.read(&mut buf).await else {
            return;
        };
        if n == 0 {
            return;
        }
        acc.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&acc).into_owned();
        let Some(end) = text.find("\r\n\r\n") else {
            continue;
        };
        let length = text[..end]
            .lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.eq_ignore_ascii_case("content-length")
                    .then(|| v.trim().parse::<usize>().ok())?
            })
            .unwrap_or(0);
        if acc.len() >= end + 4 + length {
            let target = text.lines().next().unwrap_or_default().to_owned();
            break (
                target,
                String::from_utf8_lossy(&acc[end + 4..end + 4 + length]).into_owned(),
            );
        }
    };
    log.lock().unwrap().push(format!("{target} {body}"));
    let rpc: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    let id = rpc.get("id").cloned().unwrap_or(serde_json::Value::Null);
    let streaming = match flavour {
        Http::JsonRpc => matches!(
            rpc.get("method").and_then(serde_json::Value::as_str),
            Some("SendStreamingMessage" | "SubscribeToTask")
        ),
        Http::Rest => target.contains(":stream") || target.contains(":subscribe"),
    };
    let wrap = |result: &serde_json::Value| match flavour {
        Http::JsonRpc => serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Http::Rest => result.clone(),
    };
    let response = if streaming {
        let mut out = String::from(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
        );
        for event in &answer.events {
            out.push_str(&format!("data: {}\n\n", wrap(event)));
        }
        out
    } else {
        let body = wrap(&answer.unary).to_string();
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\
             connection: close\r\n\r\n{body}",
            body.len()
        )
    };
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// A WebSocket peer: JSON-RPC frames, answered by request id.
#[cfg(feature = "websocket")]
pub async fn ws(answer: Answer) -> Peer {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let (listener, addr) = listen().await;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let (answer, log) = (answer.clone(), Arc::clone(&log));
            tokio::spawn(async move {
                let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await else {
                    return;
                };
                while let Some(Ok(Message::Text(text))) = socket.next().await {
                    log.lock().unwrap().push(text.to_string());
                    let rpc: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
                    let id = rpc.get("id").cloned().unwrap_or(serde_json::Value::Null);
                    let streaming = matches!(
                        rpc.get("method").and_then(serde_json::Value::as_str),
                        Some("SendStreamingMessage" | "SubscribeToTask")
                    );
                    let results = if streaming {
                        answer.events.clone()
                    } else {
                        vec![answer.unary.clone()]
                    };
                    for result in results {
                        let frame =
                            serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result});
                        let _ = socket.send(Message::Text(frame.to_string().into())).await;
                    }
                }
            });
        }
    });
    Peer {
        url: format!("ws://{addr}"),
        seen,
    }
}

/// A gRPC peer over HTTP/2: `unary` (an encoded protobuf message) to every
/// unary call, `events` to every streaming one, then status OK.
#[cfg(feature = "grpc")]
pub async fn grpc(unary: Vec<u8>, events: Vec<Vec<u8>>) -> Peer {
    use std::convert::Infallible;

    use hyper::body::{Bytes, Frame};

    let (listener, addr) = listen().await;
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let (unary, events) = (unary.clone(), events.clone());
            tokio::spawn(async move {
                let svc = hyper::service::service_fn(move |req: hyper::Request<_>| {
                    let streaming = req.uri().path().ends_with("SendStreamingMessage")
                        || req.uri().path().ends_with("SubscribeToTask");
                    let messages = if streaming {
                        events.clone()
                    } else {
                        vec![unary.clone()]
                    };
                    async move {
                        let _ = http_body_util::BodyExt::collect(req.into_body()).await;
                        let mut frames: Vec<Result<Frame<Bytes>, Infallible>> = messages
                            .into_iter()
                            .map(|payload| {
                                let mut frame = vec![0_u8];
                                frame.extend_from_slice(
                                    &u32::try_from(payload.len()).unwrap().to_be_bytes(),
                                );
                                frame.extend_from_slice(&payload);
                                Ok(Frame::data(Bytes::from(frame)))
                            })
                            .collect();
                        let mut trailers = hyper::HeaderMap::new();
                        trailers
                            .insert("grpc-status", hyper::header::HeaderValue::from_static("0"));
                        frames.push(Ok(Frame::trailers(trailers)));
                        let body = http_body_util::StreamBody::new(tokio_stream::iter(frames));
                        let mut resp = hyper::Response::new(body);
                        resp.headers_mut().insert(
                            "content-type",
                            hyper::header::HeaderValue::from_static("application/grpc"),
                        );
                        Ok::<_, Infallible>(resp)
                    }
                });
                let _ =
                    hyper::server::conn::http2::Builder::new(hyper_util::rt::TokioExecutor::new())
                        .serve_connection(hyper_util::rt::TokioIo::new(stream), svc)
                        .await;
            });
        }
    });
    Peer {
        url: format!("http://{addr}"),
        seen: Arc::default(),
    }
}
