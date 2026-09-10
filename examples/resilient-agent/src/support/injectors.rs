// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Fault injectors on real sockets: a webhook that refuses its first M
//! deliveries, and a proxy that faults its first K requests in front of a
//! real agent.
//!
//! Both count what they saw, so an act can put the SDK's report next to the
//! injector's own tally — "the sender says three deliveries failed" means
//! something only beside "the webhook refused exactly three".

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bytes::Bytes;
use http_body_util::{BodyExt as _, Full};
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};

/// Binds an ephemeral loopback port.
async fn bind() -> (tokio::net::TcpListener, String) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("binding a loopback port");
    let addr = listener.local_addr().expect("local_addr");
    (listener, format!("http://{addr}"))
}

fn text_response(status: StatusCode, body: &'static str) -> Response<Full<Bytes>> {
    let mut resp = Response::new(Full::new(Bytes::from_static(body.as_bytes())));
    *resp.status_mut() = status;
    resp
}

// ── Webhook sink ─────────────────────────────────────────────────────────────

/// Counts for a [`webhook_sink`].
#[derive(Debug, Default)]
pub struct SinkTally {
    refusals_left: AtomicU32,
    /// Requests answered with the refusal status.
    pub refused: AtomicU32,
    /// Requests answered `200`.
    pub accepted: AtomicU32,
}

impl SinkTally {
    pub fn refused(&self) -> u32 {
        self.refused.load(Ordering::SeqCst)
    }

    pub fn accepted(&self) -> u32 {
        self.accepted.load(Ordering::SeqCst)
    }
}

/// A push-notification receiver that answers its first `refusals` requests
/// with `status` and `200` thereafter.
///
/// Returns the webhook URL and the tally. `503` is the interesting status
/// because [`HttpPushSender`] classes it as transient and retries it inside
/// one delivery; a `4xx` would be refused once and not retried at all.
///
/// [`HttpPushSender`]: a2a_protocol_server::push::HttpPushSender
pub async fn webhook_sink(refusals: u32, status: StatusCode) -> (String, Arc<SinkTally>) {
    let (listener, url) = bind().await;
    let tally = Arc::new(SinkTally {
        refusals_left: AtomicU32::new(refusals),
        ..SinkTally::default()
    });
    let served = Arc::clone(&tally);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let tally = Arc::clone(&served);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req: Request<Incoming>| {
                    let tally = Arc::clone(&tally);
                    async move {
                        // Read the body so the sender's request completes
                        // normally either way.
                        let _ = req.into_body().collect().await;
                        let refuse = tally
                            .refusals_left
                            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                            .is_ok();
                        let resp = if refuse {
                            tally.refused.fetch_add(1, Ordering::SeqCst);
                            text_response(status, "not now")
                        } else {
                            tally.accepted.fetch_add(1, Ordering::SeqCst);
                            text_response(StatusCode::OK, "ok")
                        };
                        Ok::<_, std::convert::Infallible>(resp)
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await;
            });
        }
    });
    (format!("{url}/hook"), tally)
}

// ── Faulting proxy ───────────────────────────────────────────────────────────

/// How the proxy faults a request.
#[derive(Debug, Clone, Copy)]
pub enum Fault {
    /// Answer with this HTTP status and no upstream call.
    Status(StatusCode),
    /// Accept the connection and close it without answering — the transport
    /// error a dying peer or a mid-request network blip produces.
    DropConnection,
}

/// Counts for a [`faulting_proxy`].
#[derive(Debug, Default)]
pub struct ProxyTally {
    faults_left: AtomicU32,
    /// Requests that were faulted.
    pub faulted: AtomicU32,
    /// Requests forwarded to the upstream agent.
    pub forwarded: AtomicU32,
}

impl ProxyTally {
    /// Claims one of the remaining faults, counting it. `false` once they
    /// are spent.
    fn take_fault(&self) -> bool {
        let taken = self
            .faults_left
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok();
        if taken {
            self.faulted.fetch_add(1, Ordering::SeqCst);
        }
        taken
    }

    pub fn faulted(&self) -> u32 {
        self.faulted.load(Ordering::SeqCst)
    }

    pub fn forwarded(&self) -> u32 {
        self.forwarded.load(Ordering::SeqCst)
    }
}

/// A reverse proxy that faults its first `faults` requests as `fault` says and
/// forwards everything after that to `upstream`.
///
/// Injecting in front of a real agent, rather than hand-writing a JSON-RPC
/// response, means the success path is a genuine agent reply: a passing check
/// proves the retried request actually completed.
pub async fn faulting_proxy(
    upstream: String,
    faults: u32,
    fault: Fault,
) -> (String, Arc<ProxyTally>) {
    let (listener, url) = bind().await;
    let tally = Arc::new(ProxyTally {
        faults_left: AtomicU32::new(faults),
        ..ProxyTally::default()
    });
    let served = Arc::clone(&tally);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let tally = Arc::clone(&served);
            // A dropped connection is a per-connection fault by nature.
            if matches!(fault, Fault::DropConnection) && tally.take_fault() {
                // Close the accepted socket without reading or writing a
                // byte. hyper reports this as a connection error, which is
                // the transient transport failure the retry layer classes as
                // retryable.
                drop(stream);
                continue;
            }
            let io = hyper_util::rt::TokioIo::new(stream);
            let upstream = upstream.clone();
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req: Request<Incoming>| {
                    let upstream = upstream.clone();
                    let tally = Arc::clone(&tally);
                    async move {
                        // A status fault is decided per *request*, not per
                        // connection: the client keeps a `503`'d connection
                        // alive and sends its retry down the same socket. The
                        // first version decided at accept time and faulted
                        // every retry too.
                        if let Fault::Status(status) = fault
                            && tally.take_fault()
                        {
                            return Ok::<_, std::convert::Infallible>(text_response(
                                status,
                                "upstream unavailable",
                            ));
                        }
                        tally.forwarded.fetch_add(1, Ordering::SeqCst);
                        Ok(forward(&upstream, req).await)
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await;
            });
        }
    });
    (url, tally)
}

/// Replays one request against `upstream` and returns its response.
async fn forward(upstream: &str, req: Request<Incoming>) -> Response<Full<Bytes>> {
    let bad_gateway = || text_response(StatusCode::BAD_GATEWAY, "proxy could not reach upstream");

    let (parts, body) = req.into_parts();
    let Ok(body) = body.collect().await else {
        return bad_gateway();
    };
    let client: hyper_util::client::legacy::Client<_, Full<Bytes>> =
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build_http();
    let mut builder = Request::builder().method(parts.method).uri(upstream);
    // Every header is forwarded except `host`, which must match the new
    // authority and which hyper derives from the URI. Dropping `A2A-Version`
    // here would turn every forwarded call into a VERSION_NOT_SUPPORTED error
    // and make the check about the proxy instead of the retry layer.
    for (name, value) in parts
        .headers
        .iter()
        .filter(|(name, _)| *name != hyper::header::HOST)
    {
        builder = builder.header(name, value);
    }
    let Ok(outbound) = builder.body(Full::new(body.to_bytes())) else {
        return bad_gateway();
    };
    let Ok(resp) = client.request(outbound).await else {
        return bad_gateway();
    };
    let (parts, incoming) = resp.into_parts();
    let Ok(collected) = incoming.collect().await else {
        return bad_gateway();
    };
    Response::from_parts(parts, Full::new(collected.to_bytes()))
}
