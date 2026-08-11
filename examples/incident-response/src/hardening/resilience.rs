// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Riding out a transient failure — without double-executing real work.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_client::{ClientBuilder, RetryPolicy};
use a2a_protocol_server::builder::RequestHandlerBuilder;

use super::{bind, plain_card, serve, Check};
use crate::agents::LogSearchExecutor;
use crate::{send_params, user_message};

const LABEL: &str = "Client retry (503 retried, 502 not — no double execution)";

/// How many times the proxy faults before letting a request through.
const FAULTS: u32 = 2;

/// A reverse proxy that fails the first [`FAULTS`] requests with a fixed status
/// and forwards everything after that.
///
/// Injecting the fault in front of a real agent, rather than hand-writing a
/// JSON-RPC response, means the success path is a genuine agent reply — so a
/// passing check proves the retried request actually completed, not that a stub
/// returned something shaped like success.
async fn faulting_proxy(upstream: String, status: u16) -> String {
    let (listener, url) = bind().await;
    let seen = Arc::new(AtomicU32::new(0));
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let upstream = upstream.clone();
            let seen = Arc::clone(&seen);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req: hyper::Request<_>| {
                    let upstream = upstream.clone();
                    let seen = Arc::clone(&seen);
                    async move {
                        if seen.fetch_add(1, Ordering::SeqCst) < FAULTS {
                            let mut resp = hyper::Response::new(http_body_util::Full::new(
                                bytes::Bytes::from_static(b"upstream unavailable"),
                            ));
                            *resp.status_mut() = hyper::StatusCode::from_u16(status)
                                .unwrap_or(hyper::StatusCode::SERVICE_UNAVAILABLE);
                            return Ok::<_, std::convert::Infallible>(resp);
                        }
                        Ok(forward(&upstream, req).await)
                    }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, service)
                .await;
            });
        }
    });
    url
}

/// Replays one request against `upstream` and returns its response.
async fn forward(
    upstream: &str,
    req: hyper::Request<hyper::body::Incoming>,
) -> hyper::Response<http_body_util::Full<bytes::Bytes>> {
    use http_body_util::BodyExt as _;

    let bad_gateway = || {
        let mut resp = hyper::Response::new(http_body_util::Full::new(bytes::Bytes::from_static(
            b"proxy could not reach upstream",
        )));
        *resp.status_mut() = hyper::StatusCode::BAD_GATEWAY;
        resp
    };

    let (parts, body) = req.into_parts();
    let Ok(body) = body.collect().await else {
        return bad_gateway();
    };
    let client: hyper_util::client::legacy::Client<_, http_body_util::Full<bytes::Bytes>> =
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build_http();
    let mut builder = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(upstream);
    // Forward every request header. Dropping them is not a neutral
    // simplification: without `A2A-Version` the server answers
    // `-32009 VERSION_NOT_SUPPORTED`, which this check first reported as a
    // failed retry. A proxy that rewrites the request is testing the proxy.
    // `host` is excluded because it must match the new authority, and hyper
    // derives it from the URI.
    for (name, value) in parts
        .headers
        .iter()
        .filter(|(name, _)| *name != hyper::header::HOST)
    {
        builder = builder.header(name, value);
    }
    let Ok(outbound) = builder.body(http_body_util::Full::new(body.to_bytes())) else {
        return bad_gateway();
    };
    let Ok(resp) = client.request(outbound).await else {
        return bad_gateway();
    };
    let (parts, incoming) = resp.into_parts();
    let Ok(collected) = incoming.collect().await else {
        return bad_gateway();
    };
    hyper::Response::from_parts(parts, http_body_util::Full::new(collected.to_bytes()))
}

/// Transient failures must be ridden out — and ambiguous ones must not be.
///
/// Three assertions, because the interesting property is not "retries happen"
/// but "retries happen *only* where a re-send cannot duplicate work":
///
/// 1. **No policy, `503` → fails.** Establishes the injector is really
///    faulting. Without this, a retrying client that succeeded would prove
///    nothing: maybe nothing ever failed.
/// 2. **Policy, `503` → succeeds.** `503` means the server refused the request
///    up front, so re-sending a non-idempotent `SendMessage` cannot
///    double-execute it.
/// 3. **Policy, `502` → still fails.** A gateway returning `502` may already
///    have handed the request to a backend that ran it. Retrying there would
///    silently create a second task. A retry layer that treats every 5xx alike
///    passes (1) and (2) and fails only this.
pub(super) async fn client_retry() -> Check {
    let (listener, agent_url) = bind().await;
    let handler = match RequestHandlerBuilder::new(LogSearchExecutor)
        .with_agent_card(plain_card(&agent_url, "Flaky-Fronted Agent"))
        .build()
    {
        Ok(handler) => Arc::new(handler),
        Err(e) => return Check::fail(LABEL, format!("building the handler: {e}")),
    };
    serve(listener, handler);

    // Short backoff: the point is that a retry happens, not how long it waits.
    // Built per client rather than cloned so both legs demonstrably use the
    // same configuration.
    let policy = || {
        RetryPolicy::default()
            .with_max_retries(FAULTS + 1)
            .with_initial_backoff(Duration::from_millis(10))
            .with_max_backoff(Duration::from_millis(50))
    };

    // (1) 503 with no retry policy.
    let plain_url = faulting_proxy(agent_url.clone(), 503).await;
    let plain = match ClientBuilder::new(&plain_url).build() {
        Ok(client) => client,
        Err(e) => return Check::fail(LABEL, format!("building the plain client: {e}")),
    };
    if plain
        .send_message(send_params(user_message("payments-api")))
        .await
        .is_ok()
    {
        return Check::fail(
            LABEL,
            format!("a client with no retry policy survived {FAULTS} injected 503s — the fault injector is not faulting, so the rest of this check would be vacuous"),
        );
    }

    // (2) 503 with one.
    let retry_url = faulting_proxy(agent_url.clone(), 503).await;
    let retrying = match ClientBuilder::new(&retry_url)
        .with_retry_policy(policy())
        .build()
    {
        Ok(client) => client,
        Err(e) => return Check::fail(LABEL, format!("building the retrying client: {e}")),
    };
    if let Err(e) = retrying
        .send_message(send_params(user_message("payments-api")))
        .await
    {
        return Check::fail(
            LABEL,
            format!(
                "{FAULTS} injected 503s were not ridden out by a {}-retry policy: {e}",
                FAULTS + 1
            ),
        );
    }

    // (3) 502 with the same policy.
    let ambiguous_url = faulting_proxy(agent_url, 502).await;
    let ambiguous = match ClientBuilder::new(&ambiguous_url)
        .with_retry_policy(policy())
        .build()
    {
        Ok(client) => client,
        Err(e) => return Check::fail(LABEL, format!("building the 502 client: {e}")),
    };
    match ambiguous
        .send_message(send_params(user_message("payments-api")))
        .await
    {
        Ok(_) => Check::fail(
            LABEL,
            "a 502 on SendMessage was retried — an ambiguous failure on a non-idempotent \
             method can double-execute the work",
        ),
        Err(_) => Check::pass(
            LABEL,
            format!("{FAULTS} x 503 ridden out; 502 on a non-idempotent method not retried"),
        ),
    }
}
