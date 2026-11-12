// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `docs/readiness-bar.md`, measured: load profiles L1, L3 and L4 over HTTP
//! JSON-RPC on loopback against the real server, and the rows they decide
//! (adversarial review, Stage 5, 2026-11-12).
//!
//! Rows measured here: R1, R2, R3 (L1), R4 (L4), C1, C2, C3, C4 (L1) and C2, C3, C5
//! (L3). Not measured here, and not claimed: L2, L5, R5, R6, R7, F1 to F5, and
//! anything with two replicas or a real PostgreSQL. A row this file does not name is
//! unmet, as the bar says.
//!
//! The thresholds are the bar's, written before any number existed; none is
//! tuned here. Each row prints its number and its verdict **before** anything
//! is asserted, so a failure still shows what was measured. `#[ignore]`d: it runs
//! for `A2A_BAR_SECS` seconds (default 20) of load.
//!
//! ```bash
//! cargo test -p a2a-protocol-server --release --test readiness_bar -- --ignored --nocapture
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use a2a_protocol_server::store::InMemoryTaskStore;
use a2a_protocol_server::store::task_store::TaskStoreConfig;
use a2a_protocol_server::{RequestHandler, RequestHandlerBuilder};
use http_body_util::BodyExt as _;
use tokio::io::AsyncWriteExt as _;

mod support;

use support::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "load; run explicitly with --ignored --release (see the module docs)"]
async fn the_readiness_bar_rows_l1_l3_l4() {
    let seconds: u64 = std::env::var("A2A_BAR_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let mut rows = Rows(Vec::new());

    let handler = Arc::new(
        RequestHandlerBuilder::new(OneArtifact)
            .without_agent_card()
            .with_task_store(InMemoryTaskStore::with_config(
                TaskStoreConfig::default().with_max_capacity(Some(STORE_CAPACITY)),
            ))
            .build()
            .unwrap(),
    );
    let (addr, stop_server) = serve(Arc::clone(&handler)).await;

    // ── baseline, after a warm-up the bar does not count ────────────────────────
    {
        let c = client();
        for n in 0..200 {
            let r = c.request(request(addr, n)).await.unwrap();
            let _ = r.into_body().collect().await;
        }
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    let (fds0, tasks0) = (open_fds(), alive_tasks());

    // ── L1: steady blocking, 8 clients ──────────────────────────────────────────
    let stop = Arc::new(AtomicBool::new(false));
    let errors = Arc::new(AtomicU64::new(0));
    let samples = Arc::new(std::sync::Mutex::new(Vec::<(u64, u64)>::new()));
    let rss_start = resident_bytes();
    let began = Instant::now();
    let mut workers = Vec::new();
    for w in 0..L1_CLIENTS {
        let (stop, errors, samples) =
            (Arc::clone(&stop), Arc::clone(&errors), Arc::clone(&samples));
        workers.push(tokio::spawn(async move {
            let c = client();
            let mut n = 1_000 + w as u64;
            while !stop.load(Ordering::Relaxed) {
                n += L1_CLIENTS as u64;
                let sent = Instant::now();
                match c.request(request(addr, n)).await {
                    Ok(r) => {
                        let ok = r.status().is_success();
                        let text = r.into_body().collect().await.map(|b| b.to_bytes());
                        let answered = ok
                            && text
                                .is_ok_and(|b| !String::from_utf8_lossy(&b).contains("\"error\""));
                        let micros = u64::try_from(sent.elapsed().as_micros()).unwrap_or(u64::MAX);
                        if answered {
                            samples
                                .lock()
                                .unwrap()
                                .push((began.elapsed().as_millis() as u64, micros));
                        } else {
                            errors.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(_) => {
                        errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }));
    }
    tokio::time::sleep(Duration::from_secs(seconds)).await;
    stop.store(true, Ordering::Relaxed);
    for w in workers {
        w.await.unwrap();
    }
    let elapsed_ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX);
    let rss_end = resident_bytes();
    let mut all = samples.lock().unwrap().clone();
    let completed = all.len() as u64;
    let err = errors.load(Ordering::Relaxed);
    println!(
        "L1: {completed} requests in {:.1}s ({:.0}/s), {err} errors",
        elapsed_ms as f64 / 1000.0,
        completed as f64 * 1000.0 / elapsed_ms as f64
    );

    rows.row(
        "R1",
        err == 0,
        format!("{err} errors in {completed} requests under L1 (L2 and L5 not run)"),
    );

    let quarter = elapsed_ms / 4;
    let mut first: Vec<u64> = all
        .iter()
        .filter(|(t, _)| *t < quarter)
        .map(|(_, l)| *l)
        .collect();
    let mut last: Vec<u64> = all
        .iter()
        .filter(|(t, _)| *t >= elapsed_ms - quarter)
        .map(|(_, l)| *l)
        .collect();
    let (p99_first, p99_last) = (percentile(&mut first, 0.99), percentile(&mut last, 0.99));
    let ratio = p99_last as f64 / p99_first.max(1) as f64;
    rows.row("R2", ratio <= 1.5, format!("last-quarter p99 {p99_last} us / first-quarter p99 {p99_first} us = {ratio:.2} (bar: <= 1.5)"));

    let mut lat: Vec<u64> = all.drain(..).map(|(_, l)| l).collect();
    let (p50, p99) = (percentile(&mut lat, 0.5), percentile(&mut lat, 0.99));
    rows.row("R3", p99 < 25_000, format!("p50 {p50} us, p99 {p99} us over {completed} sends, 8 concurrent clients (bar: p99 < 25000 us)"));

    let growth = rss_end.saturating_sub(rss_start) as f64 / completed.max(1) as f64;
    rows.row("C1", growth <= 1024.0, format!("RSS {rss_start} -> {rss_end} bytes over {completed} requests = {growth:.1} bytes/request (bar: <= 1024)"));

    let fds = settles(Duration::from_secs(10), open_fds, |v| v <= fds0 + 8).await;
    rows.row(
        "C2",
        fds <= fds0 + 8,
        format!("after L1: {fds} fds against {fds0} before (bar: within +8, 10 s)"),
    );
    let tasks = settles(Duration::from_secs(10), alive_tasks, |v| v <= tasks0).await;
    rows.row("C3", tasks <= tasks0, format!("after L1: {tasks} tokio tasks against {tasks0} before (bar: back to the pre-load count, 10 s)"));
    let open = non_terminal(&handler).await;
    rows.row(
        "C4",
        open == 0,
        format!("{open} tasks non-terminal after L1"),
    );

    // ── L3: churn, half dropped before the response arrives ─────────────────────
    let churn: u64 = std::env::var("A2A_BAR_CHURN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2_000);
    for n in 0..churn {
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        let payload = body(500_000 + n);
        let head = format!(
            "POST / HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nA2A-Version: 1.0\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        s.write_all(head.as_bytes()).await.unwrap();
        s.write_all(payload.as_bytes()).await.unwrap();
        if n % 2 == 0 {
            // Dropped before the response: the server's write finds no reader.
            drop(s);
        } else {
            let mut out = Vec::new();
            let _ = tokio::io::AsyncReadExt::read_to_end(&mut s, &mut out).await;
        }
    }
    let fds = settles(Duration::from_secs(10), open_fds, |v| v <= fds0 + 8).await;
    rows.row("C2", fds <= fds0 + 8, format!("after L3 ({churn} connections, half dropped early): {fds} fds against {fds0} before (bar: within +8, 10 s)"));
    let tasks = settles(Duration::from_secs(10), alive_tasks, |v| v <= tasks0).await;
    rows.row(
        "C3",
        tasks <= tasks0,
        format!("after L3: {tasks} tokio tasks against {tasks0} before"),
    );
    let running = settles(
        Duration::from_secs(10),
        || handler.load().running,
        |v| v == 0,
    )
    .await;
    rows.row(
        "C5",
        running == 0,
        format!("after L3: {running} live event queues (bar: 0)"),
    );
    // The bar names C4 after L1 only. After L3 it is read as well, and for longer,
    // because a task a dropped client left behind is a different question: does
    // anything end it, and when?
    let waited = Instant::now();
    let open = settles(
        Duration::from_secs(120),
        || futures_lite_count(&handler),
        |v| v == 0,
    )
    .await;
    let after = waited.elapsed();
    let left = stuck(&handler).await;
    rows.row(
        "C4",
        open == 0,
        format!(
            "{open} tasks non-terminal after L3, read for up to 120 s ({:.1} s waited); left: {left:?}",
            after.as_secs_f64(),
        ),
    );

    let _ = handler.shutdown().await;
    stop_server.send(()).ok();

    // ── L4: 4x the concurrency limit, executor holds its slot ───────────────────
    const LIMIT: usize = 4;
    let overloaded = Arc::new(
        RequestHandlerBuilder::new(Slow)
            .without_agent_card()
            .with_max_concurrent_streams(LIMIT)
            .build()
            .unwrap(),
    );
    let (addr, stop_server) = serve(Arc::clone(&overloaded)).await;
    let mut calls = Vec::new();
    for n in 0..(LIMIT * 4) as u64 {
        calls.push(tokio::spawn(async move {
            let c = client();
            let sent = Instant::now();
            let outcome = tokio::time::timeout(
                Duration::from_secs(5),
                c.request(request(addr, 900_000 + n)),
            )
            .await;
            match outcome {
                Err(_) => ("client timeout".to_owned(), sent.elapsed()),
                Ok(Err(e)) => (format!("transport error: {e}"), sent.elapsed()),
                Ok(Ok(r)) => {
                    let status = r.status().as_u16();
                    let text = r
                        .into_body()
                        .collect()
                        .await
                        .map(|b| String::from_utf8_lossy(&b.to_bytes()).into_owned())
                        .unwrap_or_default();
                    let refusal = text.contains("\"error\"");
                    (
                        format!("{status} {}", if refusal { text } else { "ok".to_owned() }),
                        sent.elapsed(),
                    )
                }
            }
        }));
    }
    let mut served = 0;
    let mut refused = Vec::new();
    let mut other = Vec::new();
    for c in calls {
        let (what, took) = c.await.unwrap();
        if what.ends_with(" ok") {
            served += 1;
        } else if what.starts_with("503") || what.starts_with("429") || what.contains("RetryInfo") {
            // The binding's overload signal: HTTP 503/429, or a JSON-RPC error
            // that carries the retry delay (`google.rpc.RetryInfo`).
            refused.push((what, took));
        } else {
            other.push(what);
        }
    }
    let slowest_refusal = refused.iter().map(|(_, t)| *t).max().unwrap_or_default();
    rows.row(
        "R4",
        other.is_empty() && !refused.is_empty(),
        format!(
            "{} sends against a limit of {LIMIT}: {served} served, {} refused with an overload signal (slowest refusal {:?}), {} neither: {:?}",
            LIMIT * 4, refused.len(), slowest_refusal, other.len(), other.first()
        ),
    );
    println!("L4 first refusal observed: {:?}", refused.first());
    let _ = overloaded.shutdown().await;
    stop_server.send(()).ok();

    let unmet: Vec<_> = rows
        .0
        .iter()
        .filter(|(_, ok, _)| !ok)
        .map(|(id, _, said)| format!("{id}: {said}"))
        .collect();
    assert!(
        unmet.is_empty(),
        "rows of the readiness bar not met: {unmet:#?}"
    );
}

fn futures_lite_count(handler: &RequestHandler) -> u64 {
    // Read the non-terminal count without awaiting: the store is in memory.
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(non_terminal(handler))
    })
}
