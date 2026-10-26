// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What runs on workerd and what does not, one `/probe/<name>` route per primitive, on
//! `a2a-protocol-types` alone (ADR 0028).
use a2a_protocol_types::agent_card::{AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::jsonrpc::JsonRpcRequest;
use a2a_protocol_types::message::Part;
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::task::{Task, TaskState, TaskStatus};
use worker::*;

fn card(origin: &str) -> AgentCard {
    AgentCard::new("probe-worker", "0.0.0", AgentInterface::jsonrpc(origin))
        .with_description("Greets whoever sends it a message")
        .with_skill(
            AgentSkill::new("hello", "Hello", "Greets whoever sends it a message")
                .with_tags(["greeting"]),
        )
}

/// Milliseconds since the epoch from the JS clock, formatted by the types crate.
fn now_iso() -> String {
    a2a_protocol_types::unix_millis_to_iso8601(Date::now().as_millis() as i64)
}

// ── Probes: what runs on workerd, one route each ────────────────────────────
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore, broadcast, mpsc, oneshot};

async fn probe(name: &str) -> std::result::Result<String, String> {
    let e = |e: &dyn std::fmt::Display| e.to_string();
    match name {
        // Tokio's sync primitives need no runtime; the other side is a JS-driven task.
        "sync" => {
            let (tx, rx) = oneshot::channel::<u32>();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = tx.send(7);
            });
            let one = rx.await.map_err(|x| e(&x))?;
            let (mtx, mut mrx) = mpsc::channel::<u32>(4);
            wasm_bindgen_futures::spawn_local(async move {
                for i in 0..3 {
                    mtx.send(i).await.ok();
                }
            });
            let mut sum = 0;
            while let Some(v) = mrx.recv().await {
                sum += v;
            }
            let (btx, mut brx) = broadcast::channel::<u32>(4);
            btx.send(9).ok();
            let b = brx.recv().await.map_err(|x| e(&x))?;
            let m = Mutex::new(1);
            *m.lock().await += 1;
            let sem = Semaphore::new(1);
            let _permit = sem.acquire().await.map_err(|x| e(&x))?;
            Ok(format!(
                "oneshot={one} mpsc_sum={sum} broadcast={b} mutex={}",
                *m.lock().await
            ))
        }
        "std-instant" => Ok(format!("{:?}", std::time::Instant::now().elapsed())),
        "tokio-sleep" => {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok("slept".into())
        }
        "tokio-spawn" => Ok(format!("{:?}", tokio::spawn(async { 5 }).await)),
        "wasmtimer" => {
            let t = web_time::Instant::now();
            wasmtimer::tokio::sleep(Duration::from_millis(40)).await;
            let timed_out = wasmtimer::tokio::timeout(
                Duration::from_millis(30),
                futures::future::pending::<()>(),
            )
            .await
            .is_err();
            Ok(format!(
                "web_time elapsed over sleep+timeout={}ms timeout_fired={timed_out}",
                t.elapsed().as_millis()
            ))
        }
        "tokio-with-wasm" => {
            use tokio_with_wasm::alias as tw;
            let v = tw::spawn(async { 5 }).await.map_err(|x| e(&x))?;
            tw::time::sleep(Duration::from_millis(10)).await;
            let fired =
                tw::time::timeout(Duration::from_millis(20), futures::future::pending::<()>())
                    .await
                    .is_err();
            Ok(format!("spawn={v} timeout_fired={fired}"))
        }
        // The `Send` bound the server's traits carry, with a JS future inside:
        // `worker::send::SendFuture` is the sanctioned wrapper.
        "send-fetch" => {
            fn assert_send<T: Send>(t: T) -> T {
                t
            }
            let fut = assert_send(Box::pin(worker::send::SendFuture::new(async {
                let mut r = Fetch::Url(
                    "http://127.0.0.1:8787/.well-known/agent-card.json"
                        .parse()
                        .map_err(|x: url::ParseError| x.to_string())?,
                )
                .send()
                .await
                .map_err(|x| x.to_string())?;
                let t = r.text().await.map_err(|x| x.to_string())?;
                Ok::<_, String>(format!("status={} bytes={}", r.status_code(), t.len()))
            }))
                as std::pin::Pin<
                    Box<
                        dyn std::future::Future<Output = std::result::Result<String, String>>
                            + Send,
                    >,
                >);
            fut.await
        }
        other => Err(format!("no probe named {other}")),
    }
}

#[event(fetch)]
async fn fetch(mut req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();
    let url = req.url()?;
    let origin = format!(
        "{}://{}",
        url.scheme(),
        url.host_str().unwrap_or("localhost")
    );
    let origin = match url.port() {
        Some(p) => format!("{origin}:{p}"),
        None => origin,
    };
    match (req.method(), url.path()) {
        (Method::Get, "/.well-known/agent-card.json") => Response::from_json(&card(&origin)),
        // The types crate's own timestamp helper, which reads `SystemTime::now()`.
        (Method::Get, "/std-time") => {
            let s = TaskStatus::with_timestamp(TaskState::Completed);
            Response::from_json(&s)
        }
        (Method::Get, p) if p.starts_with("/probe/") => match probe(&p["/probe/".len()..]).await {
            Ok(m) => Response::ok(format!("ok: {m}")),
            Err(m) => Response::error(format!("err: {m}"), 500),
        },
        (Method::Post, "/") => {
            let body = req.text().await?;
            let rpc: JsonRpcRequest = match serde_json::from_str(&body) {
                Ok(r) => r,
                Err(e) => return Response::error(format!("bad request: {e}"), 400),
            };
            if rpc.method != "SendMessage" {
                return Response::from_json(&serde_json::json!({
                    "jsonrpc": "2.0", "id": rpc.id,
                    "error": { "code": -32601, "message": "method not found" }
                }));
            }
            let params: MessageSendParams = match rpc.params.clone().map(serde_json::from_value) {
                Some(Ok(p)) => p,
                _ => return Response::error("invalid params", 400),
            };
            let who = params.message.text().unwrap_or("world").to_owned();
            let mut task = Task::new(
                uuid::Uuid::new_v4().to_string(),
                uuid::Uuid::new_v4().to_string(),
                TaskStatus::new(TaskState::Completed),
            );
            task.status.timestamp = Some(now_iso());
            task.artifacts = Some(vec![Artifact::new(
                "greeting",
                vec![Part::text(format!("Hello, {who}!"))],
            )]);
            Response::from_json(&serde_json::json!({
                "jsonrpc": "2.0", "id": rpc.id,
                "result": { "task": task }
            }))
        }
        _ => Response::error("not found", 404),
    }
}
