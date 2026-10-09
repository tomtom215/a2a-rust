// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! **Swarm** — a coordinator, a tier of supervisor agents, and a tier of worker
//! agents, every hop a real A2A call between real servers on loopback.
//!
//! ```text
//! coordinator ──SendMessage──▶ supervisor ×S ──SendStreamingMessage──▶ worker ×W
//! ```
//!
//! Four scenarios, each printed as a table row and one JSON line:
//!
//! | scenario | question it answers |
//! |---|---|
//! | `fanout` | jobs/s and per-job latency through two delegation hops |
//! | `faults` | does retry-by-class recover exactly the transient faults, and only those |
//! | `cancel` | cancelling the root: how long until no worker is still executing |
//! | `cancel-control` | the same, with the supervisor's hand-written cancel fan-out off |
//! | `llm` | the same tree with a real model in every leaf (needs `SWARM_LLM_URL`) |
//!
//! Exit status is non-zero if any scenario's accounting does not add up, so the
//! numbers cannot be reported off a run that silently lost work.
//!
//! Run: `cargo run --release -p swarm` (env: `SWARM_WORKERS`, `SWARM_SUPERVISORS`,
//! `SWARM_JOBS`, `SWARM_INFLIGHT`, `SWARM_LLM_URL`, `SWARM_LLM_JOBS`).

mod llm;
mod supervisor;
mod topology;
mod worker;

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use a2a_protocol_client::A2aClient;
use a2a_protocol_types::{
    Message, MessageSendParams, Part, SendMessageConfiguration, SendMessageResponse, TaskState,
};

use supervisor::Supervisor;
use worker::Worker;

fn env(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

struct Swarm {
    supervisors: Vec<Arc<A2aClient>>,
    live: Arc<AtomicI64>,
}

/// `w` workers split evenly into `s` disjoint pools, one per supervisor.
async fn build(
    w: usize,
    s: usize,
    inflight: usize,
    propagate: bool,
    llm: Option<llm::Llm>,
) -> Swarm {
    let live = Arc::new(AtomicI64::new(0));
    let mut supervisors = Vec::with_capacity(s);
    for si in 0..s {
        let mut pool = Vec::new();
        for wi in (0..w).filter(|wi| wi % s == si) {
            let url = topology::spawn_agent(
                &format!("worker-{wi}"),
                Worker {
                    live: live.clone(),
                    llm: llm.clone(),
                },
            )
            .await;
            pool.push(Arc::new(topology::client(&url)));
        }
        let sup = Supervisor {
            workers: Arc::new(pool),
            max_inflight: inflight,
            next: Arc::new(AtomicUsize::new(0)),
            propagate,
        };
        let url = topology::spawn_agent(&format!("supervisor-{si}"), sup).await;
        supervisors.push(Arc::new(topology::client(&url)));
    }
    Swarm { supervisors, live }
}

fn batch(jobs: &[String]) -> MessageSendParams {
    let text = format!(
        "batch:{}",
        serde_json::to_string(jobs).expect("strings serialize")
    );
    MessageSendParams::new(Message::user(
        uuid::Uuid::new_v4().to_string(),
        vec![Part::text(text)],
    ))
}

/// Deals `jobs` round-robin to the supervisors, runs every batch to completion
/// concurrently, and merges their summaries.
async fn run_batches(
    swarm: &Swarm,
    jobs: Vec<String>,
) -> (
    serde_json::Map<String, serde_json::Value>,
    u64,
    Vec<u128>,
    f64,
) {
    let n = swarm.supervisors.len();
    let mut per: Vec<Vec<String>> = vec![Vec::new(); n];
    for (i, j) in jobs.into_iter().enumerate() {
        per[i % n].push(j);
    }
    let t0 = Instant::now();
    let mut set = tokio::task::JoinSet::new();
    for (sup, jobs) in swarm.supervisors.iter().cloned().zip(per) {
        set.spawn(async move { sup.send_message(batch(&jobs)).await });
    }
    let (mut tally, mut retried, mut lat) = (serde_json::Map::new(), 0u64, Vec::new());
    while let Some(r) = set.join_next().await {
        let task = match r.expect("join").expect("supervisor answered") {
            SendMessageResponse::Task(t) => t,
            other => panic!("supervisor answered with a non-task: {other:?}"),
        };
        assert_eq!(
            task.status.state,
            TaskState::Completed,
            "supervisor task did not complete"
        );
        let summary: serde_json::Value =
            serde_json::from_str(task.text().expect("summary artifact")).expect("summary json");
        for (k, v) in summary["outcomes"].as_object().expect("outcomes") {
            let e = tally.entry(k.clone()).or_insert(serde_json::json!(0));
            *e = serde_json::json!(e.as_u64().unwrap_or(0) + v.as_u64().unwrap_or(0));
        }
        retried += summary["retried"].as_u64().unwrap_or(0);
        lat.extend(
            summary["job_latency_us"]
                .as_array()
                .expect("latencies")
                .iter()
                .filter_map(serde_json::Value::as_u64)
                .map(u128::from),
        );
    }
    lat.sort_unstable();
    (tally, retried, lat, t0.elapsed().as_secs_f64())
}

fn pct(sorted: &[u128], p: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn count(t: &serde_json::Map<String, serde_json::Value>, k: &str) -> u64 {
    t.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0)
}

/// Cancels each supervisor's root task once every job is executing, and
/// measures how long until no worker execution is live.
async fn cancel_run(swarm: &Swarm, jobs: usize) -> serde_json::Value {
    let n = swarm.supervisors.len();
    let per = jobs / n;
    let mut roots = Vec::new();
    for sup in &swarm.supervisors {
        let mut p = batch(&vec!["sleep:20000".to_owned(); per]);
        p.configuration = Some(SendMessageConfiguration {
            return_immediately: Some(true),
            ..Default::default()
        });
        match sup.send_message(p).await.expect("submit") {
            SendMessageResponse::Task(t) => roots.push((sup.clone(), t.id.to_string())),
            other => panic!("expected a task, got {other:?}"),
        }
    }
    let expect = (per * n) as i64;
    let ramp = Instant::now();
    while swarm.live.load(Ordering::SeqCst) < expect && ramp.elapsed() < Duration::from_secs(20) {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let before = swarm.live.load(Ordering::SeqCst);
    let t0 = Instant::now();
    for (sup, id) in &roots {
        sup.cancel_task(id.clone()).await.expect("cancel root");
    }
    let cancel_rpcs_ms = t0.elapsed().as_secs_f64() * 1e3;
    let mut quiesce_ms = None;
    while t0.elapsed() < Duration::from_secs(5) {
        if swarm.live.load(Ordering::SeqCst) == 0 {
            quiesce_ms = Some(t0.elapsed().as_secs_f64() * 1e3);
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    serde_json::json!({
        "jobs_live_before_cancel": before, "expected_live": expect,
        "root_cancel_rpcs_ms": cancel_rpcs_ms, "quiesce_ms": quiesce_ms,
        "still_executing_after_5s": swarm.live.load(Ordering::SeqCst),
    })
}

#[tokio::main]
async fn main() {
    let (w, s) = (env("SWARM_WORKERS", 64), env("SWARM_SUPERVISORS", 8));
    let (jobs, inflight) = (env("SWARM_JOBS", 4000), env("SWARM_INFLIGHT", 32));
    let mut ok = true;
    println!("swarm: {w} workers under {s} supervisors, {inflight} in flight per supervisor");

    let swarm = build(w, s, inflight, true, None).await;
    let (t, retried, lat, secs) = run_batches(&swarm, vec!["sleep:0".to_owned(); jobs]).await;
    let fan = serde_json::json!({"scenario": "fanout", "jobs": jobs, "outcomes": t, "retried": retried, "wall_s": secs,
        "jobs_per_s": jobs as f64 / secs, "job_us": {"p50": pct(&lat, 0.5), "p90": pct(&lat, 0.9), "p99": pct(&lat, 0.99), "max": pct(&lat, 1.0)}});
    if count(&t, "completed") != jobs as u64 {
        eprintln!(
            "swarm: fan-out lost work: {} of {jobs} completed",
            count(&t, "completed")
        );
        ok = false;
    }
    println!("{fan}");

    // 10% transient, 2% invalid: retry must recover exactly the transient ones.
    let mix: Vec<String> = (0..jobs)
        .map(|i| match i % 50 {
            0 => "bad:0".to_owned(),
            x if x % 10 == 1 => "flaky:0".to_owned(),
            _ => "sleep:5".to_owned(),
        })
        .collect();
    let (bad, flaky) = (
        mix.iter().filter(|j| j.starts_with("bad")).count() as u64,
        mix.iter().filter(|j| j.starts_with("flaky")).count() as u64,
    );
    let (t, retried, _, secs) = run_batches(&swarm, mix).await;
    let exact = count(&t, "completed") == jobs as u64 - bad
        && count(&t, "failed:InvalidRequest") == bad
        && retried == flaky;
    if !exact {
        eprintln!("swarm: retry-by-class accounting is not exact");
        ok = false;
    }
    println!(
        "{}",
        serde_json::json!({"scenario": "faults", "jobs": jobs, "injected_transient": flaky, "injected_invalid": bad,
        "outcomes": t, "retried": retried, "wall_s": secs, "accounting_exact": exact})
    );

    let c = cancel_run(&swarm, s * inflight).await;
    if c["still_executing_after_5s"] != 0 {
        eprintln!(
            "swarm: cancel did not reach every worker: {} still executing 5 s after the roots were cancelled",
            c["still_executing_after_5s"]
        );
        ok = false;
    }
    println!("{}", serde_json::json!({"scenario": "cancel", "result": c}));

    let control = build(w, s, inflight, false, None).await;
    let c = cancel_run(&control, s * inflight).await;
    println!(
        "{}",
        serde_json::json!({"scenario": "cancel-control", "result": c})
    );

    if let Some(model) = llm::Llm::from_env() {
        let n = env("SWARM_LLM_JOBS", 32);
        let lw = env("SWARM_LLM_WORKERS", 8);
        let ls = env("SWARM_LLM_SUPERVISORS", 2);
        let li = env("SWARM_LLM_INFLIGHT", 2);
        let lswarm = build(lw, ls, li, true, Some(model)).await;
        let jobs: Vec<String> = (0..n)
            .map(|i| format!("llm:What is {i} plus {i}? Reply with the number only."))
            .collect();
        let (t, retried, lat, secs) = run_batches(&lswarm, jobs).await;
        ok &= count(&t, "completed") == n as u64;
        println!(
            "{}",
            serde_json::json!({"scenario": "llm", "jobs": n, "workers": lw, "supervisors": ls, "inflight_per_supervisor": li,
            "outcomes": t, "retried": retried, "wall_s": secs, "jobs_per_s": n as f64 / secs,
            "job_ms": {"p50": pct(&lat, 0.5) as f64 / 1e3, "p90": pct(&lat, 0.9) as f64 / 1e3, "max": pct(&lat, 1.0) as f64 / 1e3}})
        );
    } else {
        println!(
            "{}",
            serde_json::json!({"scenario": "llm", "skipped": "SWARM_LLM_URL unset"})
        );
    }
    if !ok {
        eprintln!("swarm: accounting did not add up — see the JSON lines above");
        std::process::exit(1);
    }
}
