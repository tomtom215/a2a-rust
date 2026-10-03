// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Neutral closed-loop load generator for A2A JSON-RPC servers. SDK-agnostic:
//! sends identical raw bytes to either server and validates each response.
//!
//! usage: loadgen <host:port> <mode: send|stream> <connections> <warmup_s> <measure_s> [payload_bytes]
//! Prints one JSON object with throughput, latency percentiles (µs), errors.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use hdrhistogram::Histogram;
use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper_util::rt::TokioIo;

struct Stats { lat: Histogram<u64>, ttfe: Histogram<u64>, ok: u64, err: u64, bytes: u64, events: u64, first_err: Option<String> }

fn body(mode: &str, id: u64, text: &str) -> String {
    let method = if mode == "stream" { "SendStreamingMessage" } else { "SendMessage" };
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{{"message":{{"role":"ROLE_USER","messageId":"lg-{id}","parts":[{{"text":"{text}"}}]}}}}}}"#)
}

async fn conn_loop(addr: String, mode: String, text: Arc<String>, counter: Arc<AtomicU64>, measuring: Arc<AtomicBool>, stop: Arc<AtomicBool>) -> Stats {
    let mut st = Stats { lat: Histogram::new(3).unwrap(), ttfe: Histogram::new(3).unwrap(), ok: 0, err: 0, bytes: 0, events: 0, first_err: None };
    let stream = tokio::net::TcpStream::connect(&addr).await.expect("connect");
    stream.set_nodelay(true).unwrap();
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await.expect("handshake");
    tokio::spawn(conn);
    while !stop.load(Ordering::Relaxed) {
        let id = counter.fetch_add(1, Ordering::Relaxed);
        let req = Request::post("/")
            .header("host", addr.as_str())
            .header("content-type", "application/json")
            .header("a2a-version", "1.0")
            .header("accept", if mode == "stream" { "text/event-stream" } else { "application/json" })
            .body(Full::new(Bytes::from(body(&mode, id, &text)))).unwrap();
        let t0 = Instant::now();
        let res = match sender.send_request(req).await { Ok(r) => r, Err(e) => { st.err += 1; st.first_err.get_or_insert(e.to_string()); break; } };
        let status = res.status();
        let mut b = res.into_body();
        let mut buf: Vec<u8> = Vec::new();
        let mut first: Option<Duration> = None;
        while let Some(frame) = b.frame().await {
            match frame { Ok(f) => if let Some(d) = f.data_ref() { if first.is_none() { first = Some(t0.elapsed()); } buf.extend_from_slice(d); }, Err(e) => { st.first_err.get_or_insert(e.to_string()); break; } }
        }
        let dt = t0.elapsed();
        let s = String::from_utf8_lossy(&buf);
        let good = status.is_success() && s.contains("TASK_STATE_COMPLETED") && !s.contains("\"error\"");
        if measuring.load(Ordering::Relaxed) {
            st.bytes += buf.len() as u64;
            if good {
                st.ok += 1;
                st.lat.record(dt.as_micros() as u64).ok();
                if let Some(f) = first { st.ttfe.record(f.as_micros() as u64).ok(); }
                if mode == "stream" { st.events += s.matches("data:").count() as u64; }
            } else { st.err += 1; st.first_err.get_or_insert(format!("http {status}: {}", &s[..s.len().min(200)])); }
        }
    }
    st
}

#[tokio::main]
async fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (addr, mode, conns) = (a[1].clone(), a[2].clone(), a[3].parse::<usize>().unwrap());
    let (warm, meas) = (a[4].parse::<u64>().unwrap(), a[5].parse::<u64>().unwrap());
    let payload = a.get(6).map(|s| s.parse::<usize>().unwrap()).unwrap_or(5);
    let text = Arc::new("x".repeat(payload));
    let counter = Arc::new(AtomicU64::new(1));
    let measuring = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let hs: Vec<_> = (0..conns).map(|_| tokio::spawn(conn_loop(addr.clone(), mode.clone(), text.clone(), counter.clone(), measuring.clone(), stop.clone()))).collect();
    tokio::time::sleep(Duration::from_secs(warm)).await;
    measuring.store(true, Ordering::Relaxed);
    let t0 = Instant::now();
    tokio::time::sleep(Duration::from_secs(meas)).await;
    measuring.store(false, Ordering::Relaxed);
    let elapsed = t0.elapsed().as_secs_f64();
    stop.store(true, Ordering::Relaxed);
    let mut lat = Histogram::<u64>::new(3).unwrap(); let mut ttfe = Histogram::<u64>::new(3).unwrap();
    let (mut ok, mut err, mut bytes, mut events) = (0, 0, 0, 0); let mut first_err = None;
    for h in hs { let s = h.await.unwrap(); lat.add(&s.lat).unwrap(); ttfe.add(&s.ttfe).unwrap(); ok += s.ok; err += s.err; bytes += s.bytes; events += s.events; if first_err.is_none() { first_err = s.first_err; } }
    let q = |h: &Histogram<u64>, p: f64| h.value_at_quantile(p);
    println!("{}", serde_json::json!({
        "addr": addr, "mode": mode, "connections": conns, "payload_bytes": payload, "measure_s": elapsed,
        "ok": ok, "errors": err, "rps": ok as f64 / elapsed, "resp_bytes_avg": if ok > 0 { bytes / (ok + err).max(1) } else { 0 },
        "events_per_req": if ok > 0 { events as f64 / ok as f64 } else { 0.0 },
        "lat_us": {"p50": q(&lat,0.5), "p90": q(&lat,0.9), "p99": q(&lat,0.99), "p999": q(&lat,0.999), "max": lat.max(), "mean": lat.mean()},
        "ttfe_us": {"p50": q(&ttfe,0.5), "p99": q(&ttfe,0.99)},
        "first_error": first_err,
    }));
}
