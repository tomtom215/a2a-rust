// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `a2a-hostile-client MODE URL [ID]`: one client call against a server the
//! campaign controls, bounded by `MAX_SECS` (default 30), printing one line
//! `RESULT {json}` with the outcome, the number of stream events, the seconds
//! taken and this process's peak RSS. Settings from the environment:
//! `BOS=1` (`with_blocking_over_stream`), `BINDING` (`JSONRPC`, `HTTP+JSON`).
//!
//! Modes: `send`, `stream`, `get ID`, `list`, `cancel ID`, `subscribe ID`,
//! `follow ID`, `card`, `extended`.

use std::time::{Duration, Instant};

use a2a_protocol_client::{ClientBuilder, discovery::resolve_agent_card};
use a2a_protocol_types::params::{ListTasksParams, TaskQueryParams};
use a2a_protocol_types::{Message, MessageId, MessageRole, MessageSendParams, Part};

fn peak_rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(0)
}

fn clip(s: String) -> String {
    s.chars().take(300).collect()
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (mode, url) = (args[1].clone(), args[2].clone());
    let id = args.get(3).cloned().unwrap_or_default();
    let max = Duration::from_secs(
        std::env::var("MAX_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30),
    );
    let started = Instant::now();
    let mut events = 0u64;
    let mut last = String::new();
    let work = async {
        let mut b = ClientBuilder::new(&url);
        if let Ok(binding) = std::env::var("BINDING") {
            b = b.with_protocol_binding(binding);
        }
        if std::env::var("BOS").is_ok() {
            b = b.with_blocking_over_stream(true);
        }
        let client = b.build().map_err(|e| format!("build: {e}"))?;
        let params = || {
            MessageSendParams::new(Message::new(
                MessageId::new(uuid_like()),
                MessageRole::User,
                vec![Part::text("hello")],
            ))
        };
        match mode.as_str() {
            "send" => {
                let r = client.send_message(params()).await.map_err(|e| e.to_string())?;
                last = clip(format!("{r:?}"));
            }
            "stream" | "subscribe" => {
                let mut s = if mode == "stream" {
                    client.stream_message(params()).await
                } else {
                    client.subscribe_to_task(id.clone()).await
                }
                .map_err(|e| e.to_string())?;
                while let Some(ev) = s.next().await {
                    match ev {
                        Ok(e) => {
                            events += 1;
                            if events % 1000 == 1 {
                                last = clip(format!("{e:?}"));
                            }
                        }
                        Err(e) => return Err(format!("event error after {events}: {e}")),
                    }
                }
            }
            "follow" => {
                let mut f = client
                    .follow_task(id.clone(), Duration::from_millis(200))
                    .await
                    .map_err(|e| e.to_string())?;
                while let Some(ev) = f.next().await {
                    match ev {
                        Ok(e) => {
                            events += 1;
                            last = clip(format!("{e:?}"));
                        }
                        Err(e) => return Err(format!("follow error after {events}: {e}")),
                    }
                }
            }
            "get" => {
                let t = client
                    .get_task(TaskQueryParams::new(id.clone()))
                    .await
                    .map_err(|e| e.to_string())?;
                last = clip(format!("{t:?}"));
            }
            "list" => {
                let t = client
                    .list_tasks(ListTasksParams::new())
                    .await
                    .map_err(|e| e.to_string())?;
                last = clip(format!("tasks={} next={:?}", t.tasks.len(), t.next_page_token));
            }
            "cancel" => {
                let t = client.cancel_task(id.clone()).await.map_err(|e| e.to_string())?;
                last = clip(format!("{t:?}"));
            }
            "extended" => {
                let t = client.get_extended_agent_card().await.map_err(|e| e.to_string())?;
                last = clip(format!("{t:?}"));
            }
            "card" => {
                let c = resolve_agent_card(&url).await.map_err(|e| e.to_string())?;
                last = clip(format!("{} interfaces={}", c.name, c.supported_interfaces.len()));
            }
            other => return Err(format!("unknown mode {other}")),
        }
        Ok::<(), String>(())
    };
    let outcome = match tokio::time::timeout(max, work).await {
        Ok(Ok(())) => "ok".to_owned(),
        Ok(Err(e)) => format!("err: {}", clip(e)),
        Err(_) => "timeout".to_owned(),
    };
    println!(
        "RESULT {{\"mode\":{:?},\"outcome\":{:?},\"events\":{events},\"seconds\":{:.2},\"peak_rss_kb\":{},\"last\":{:?}}}",
        mode,
        outcome,
        started.elapsed().as_secs_f64(),
        peak_rss_kb(),
        last
    );
}

fn uuid_like() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("msg-{n:032x}")
}
