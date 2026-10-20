// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for [`serve`] and [`serve_with_addr`].

use super::*;

use http_body_util::{BodyExt, Empty};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

#[test]
fn accept_retry_backoff_pauses_only_on_fd_exhaustion() {
    use std::io::{Error, ErrorKind};
    // EMFILE (24) / ENFILE (23): back off so the accept loop doesn't
    // busy-spin while the descriptor table is full.
    assert!(!accept_retry_backoff(&Error::from_raw_os_error(24)).is_zero());
    assert!(!accept_retry_backoff(&Error::from_raw_os_error(23)).is_zero());
    // A per-connection abort (ECONNABORTED = 103 on Linux) retries at once.
    assert!(accept_retry_backoff(&Error::from_raw_os_error(103)).is_zero());
    // A synthetic error carrying no OS code also retries at once (no panic).
    assert!(accept_retry_backoff(&Error::new(ErrorKind::ConnectionAborted, "aborted")).is_zero());
}

struct MockDispatcher;

impl Dispatcher for MockDispatcher {
    fn dispatch(
        &self,
        _req: hyper::Request<Incoming>,
    ) -> Pin<Box<dyn Future<Output = DispatchResponse> + Send + '_>> {
        Box::pin(async {
            let body = http_body_util::Full::new(Bytes::from("ok"));
            hyper::Response::new(BoxBody::new(body.map_err(|e| match e {})))
        })
    }
}

#[tokio::test]
async fn serve_with_addr_returns_bound_address() {
    let addr = serve_with_addr("127.0.0.1:0", MockDispatcher)
        .await
        .expect("server should bind");

    assert_ne!(addr.port(), 0, "should bind to a real port");
    assert!(addr.ip().is_loopback());

    let client = Client::builder(TokioExecutor::new()).build_http::<Empty<Bytes>>();
    let resp = client
        .get(format!("http://{addr}/").parse().unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], b"ok");
}

#[tokio::test]
async fn serve_with_addr_handles_multiple_connections() {
    let addr = serve_with_addr("127.0.0.1:0", MockDispatcher)
        .await
        .expect("server should bind");

    let client = Client::builder(TokioExecutor::new()).build_http::<Empty<Bytes>>();

    for i in 0..3 {
        let resp = client
            .get(format!("http://{addr}/").parse().unwrap())
            .await
            .unwrap_or_else(|e| panic!("request {i} failed: {e}"));
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"ok", "request {i} returned unexpected body");
    }
}

/// Kills `delete !` in both accept loops — now a single `!` inside
/// `pause_after_accept_error`.
///
/// Driven on a paused clock, so the assertion is on *virtual* elapsed
/// time: deterministic, instant, and immune to scheduler noise. A
/// wall-clock version would be either flaky at 20 ms or slow enough that
/// nobody keeps it.
///
/// Inverted, the guard sleeps zero when the backoff is zero (a no-op) and
/// skips the sleep when it is not — so the EMFILE pause disappears and the
/// accept loop spins against a full descriptor table. That is what the
/// first assertion pins.
#[tokio::test(start_paused = true)]
async fn backoff_pauses_only_on_fd_exhaustion() {
    use std::io::Error;
    use std::time::Duration;

    let start = tokio::time::Instant::now();
    pause_after_accept_error(&Error::from_raw_os_error(24)).await;
    assert!(
        start.elapsed() >= Duration::from_millis(20),
        "fd exhaustion (EMFILE) must pause the accept loop; no elapsed \
         time means it would busy-spin while the descriptor table is full"
    );

    let start = tokio::time::Instant::now();
    pause_after_accept_error(&Error::from_raw_os_error(23)).await;
    assert!(
        start.elapsed() >= Duration::from_millis(20),
        "ENFILE must pause for the same reason"
    );

    // A per-connection abort retries immediately. Asserted so the test
    // cannot pass against a body that simply always sleeps.
    let start = tokio::time::Instant::now();
    pause_after_accept_error(&Error::from_raw_os_error(103)).await;
    assert_eq!(
        start.elapsed(),
        Duration::ZERO,
        "a transient per-connection error must retry at once"
    );
}

/// A peer that never finishes its headers is cut off by the default
/// header-read timeout on `serve_with_addr`, which until 2026-10-18 ran
/// an accept loop with no hyper timer and so no header timeout at all.
///
/// On the real clock, so it takes the full 30-second default. Two paused-clock
/// versions failed on macOS CI. Auto-advance jumps to the next timer whether
/// or not a loopback readiness event has arrived (see
/// `graceful/idle/tests.rs`). The first lost to the client's own timeout;
/// the second, with no client timer, lost to the server's: run 39308035477
/// closed after 103.7 virtual seconds, so the server read the partial
/// headers only after the clock had jumped. A real socket is what this test
/// is about, so the clock has to be real too.
///
/// The two defaults tell the outcomes apart: with the header timeout the
/// connection closes at about 30 s; without it, the 75 s idle timeout would,
/// and the 40 s ceiling (inside nextest's 45 s limit) fails first.
#[tokio::test]
async fn serve_with_addr_cuts_off_a_peer_that_never_finishes_its_headers() {
    use std::time::{Duration, Instant};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const CEILING: Duration = Duration::from_secs(40);
    assert!(DEFAULT_HEADER_READ_TIMEOUT < CEILING && CEILING < DEFAULT_IDLE_TIMEOUT);

    let addr = serve_with_addr("127.0.0.1:0", MockDispatcher)
        .await
        .expect("server should bind");
    // Before the connect: the header timer may start at accept, and the
    // lower bound must not count from after it.
    let started = Instant::now();
    let mut sock = tokio::net::TcpStream::connect(addr).await.expect("connect");
    sock.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n")
        .await
        .expect("partial headers");

    let mut sink = Vec::new();
    let closed = tokio::time::timeout(CEILING, sock.read_to_end(&mut sink)).await;
    let elapsed = started.elapsed();
    assert!(
        closed.is_ok(),
        "a half-sent request was held open for {CEILING:?}, past the \
         {DEFAULT_HEADER_READ_TIMEOUT:?} header timeout: serve_with_addr is not \
         applying it"
    );
    assert!(
        elapsed >= DEFAULT_HEADER_READ_TIMEOUT,
        "closed after {elapsed:?}, before the {DEFAULT_HEADER_READ_TIMEOUT:?} \
         header timeout: something dropped it rather than timing it out"
    );
}

/// Kills `replace serve -> std::io::Result<()> with Ok(())`.
///
/// Every other test in this module drives `serve_with_addr`, a different
/// function. `serve` itself was never called, so a body that bound nothing
/// and returned success was indistinguishable from a working one.
#[tokio::test]
async fn serve_binds_and_answers_requests() {
    // Reserve an ephemeral port and release it: `serve` takes an address
    // rather than a listener, so it needs a concrete one to bind.
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve a port");
    let addr = probe.local_addr().expect("addr");
    drop(probe);

    let server = tokio::spawn(async move { serve(addr, MockDispatcher).await });

    // Binding is asynchronous, so the first connect can lose the race
    // legitimately. Bounded, so the mutant fails fast rather than hanging.
    let client = Client::builder(TokioExecutor::new()).build_http::<Empty<Bytes>>();
    let mut last_err = None;
    let mut response = None;
    for _ in 0..50 {
        match client
            .get(format!("http://{addr}/").parse().expect("uri"))
            .await
        {
            Ok(resp) => {
                response = Some(resp);
                break;
            }
            Err(e) => {
                last_err = Some(e);
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }
    }

    let resp = response.unwrap_or_else(|| {
        panic!(
            "nothing served on {addr} after ~1s; `serve` never bound it. \
             last error: {last_err:?}"
        )
    });
    assert_eq!(
        resp.status(),
        200,
        "the dispatcher's response must come back"
    );
    let body = resp.into_body().collect().await.expect("body").to_bytes();
    assert_eq!(&body[..], b"ok", "the body must come from MockDispatcher");

    server.abort();
}
