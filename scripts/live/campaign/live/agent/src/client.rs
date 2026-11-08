// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A command-line A2A client for the campaign drivers, for the bindings
//! Python cannot speak without a gRPC library (gRPC) and for the SDK's own
//! client code paths (JSON-RPC, HTTP+JSON, gRPC alike). Prints JSON.
//!
//! ```text
//! a2a-campaign-client <jsonrpc|rest|grpc> <base-url> <command> [args]
//!   send   TEXT [stream] [budget=N]  final task (and with `stream`, each event on its own line)
//!   get    TASK
//!   list   [token=T] [size=N]
//!   steer  TASK TEXT
//!   cancel TASK
//! ```
//!
//! Every result is one line `{"ok": <value>}` or `{"error": "<Display>", "debug": "<Debug>"}`.

use std::time::Duration;

use a2a_protocol_sdk::client::transport::grpc::GrpcTransport;
use a2a_protocol_sdk::client::ClientBuilder;
use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::types::budget::Budget;
use a2a_protocol_sdk::types::params::ListTasksParams;
use a2a_protocol_sdk::types::steering;

fn out<T: serde::Serialize, E: std::fmt::Display + std::fmt::Debug>(r: Result<T, E>) {
    match r {
        Ok(v) => println!("{}", serde_json::json!({ "ok": v })),
        Err(e) => println!(
            "{}",
            serde_json::json!({ "error": e.to_string(), "debug": format!("{e:?}") })
        ),
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [binding, url, command, rest @ ..] = args.as_slice() else {
        eprintln!("usage: a2a-campaign-client <jsonrpc|rest|grpc> <base-url> <command> [args]");
        std::process::exit(2);
    };
    let built = match binding.as_str() {
        "jsonrpc" => ClientBuilder::new(url).with_protocol_binding("JSONRPC"),
        "rest" => ClientBuilder::new(url).with_protocol_binding("HTTP+JSON"),
        "grpc" => match GrpcTransport::connect(url).await {
            Ok(t) => ClientBuilder::new(url).with_custom_transport(t),
            Err(e) => return out::<(), _>(Err(e)),
        },
        other => {
            eprintln!("unknown binding {other}");
            std::process::exit(2);
        }
    }
    .with_timeout(Duration::from_secs(
        std::env::var("C_CLIENT_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(900),
    ))
    .build();
    let client = match built {
        Ok(c) => c,
        Err(e) => return out::<(), _>(Err(e)),
    };
    match command.as_str() {
        "send" => {
            let text = rest.first().cloned().unwrap_or_default();
            let mut message = Message::user_text(uuid_like(), text);
            if let Some(n) = rest.iter().find_map(|a| a.strip_prefix("budget=")) {
                let meter = n.parse().unwrap_or(0);
                if let Ok(b) = Budget::new().with_meter("tokens", meter) {
                    b.attach(&mut message);
                }
            }
            let params = MessageSendParams::new(message);
            if rest.iter().any(|a| a == "stream") {
                let mut task_id = None;
                match client.stream_message(params).await {
                    Err(e) => return out::<(), _>(Err(e)),
                    Ok(mut stream) => {
                        while let Some(event) = stream.next().await {
                            match event {
                                Ok(event) => {
                                    if let StreamResponse::StatusUpdate(u) = &event {
                                        task_id.get_or_insert(u.task_id.clone());
                                    }
                                    if let StreamResponse::Task(t) = &event {
                                        task_id.get_or_insert(t.id.clone());
                                    }
                                    println!("{}", serde_json::json!({ "event": event }));
                                }
                                Err(e) => {
                                    println!("{}", serde_json::json!({ "stream_error": e.to_string() }));
                                    break;
                                }
                            }
                        }
                    }
                }
                if let Some(id) = task_id {
                    out(client.get_task(TaskQueryParams::new(id.0)).await);
                }
            } else {
                out(client.send_message(params).await);
            }
        }
        "get" => out(client
            .get_task(TaskQueryParams::new(rest.first().cloned().unwrap_or_default()))
            .await),
        "list" => {
            let mut p = ListTasksParams::default();
            p.page_token = rest.iter().find_map(|a| a.strip_prefix("token=")).map(str::to_owned);
            p.page_size = rest.iter().find_map(|a| a.strip_prefix("size=")).and_then(|v| v.parse().ok());
            out(client.list_tasks(p).await);
        }
        "steer" => {
            let (Some(task), Some(text)) = (rest.first(), rest.get(1)) else {
                eprintln!("steer TASK TEXT");
                std::process::exit(2);
            };
            let mut message = Message::user_text(uuid_like(), text.clone());
            message.task_id = Some(TaskId(task.clone()));
            steering::declare(&mut message);
            out(client.send_message(MessageSendParams::new(message)).await);
        }
        "cancel" => out(client.cancel_task(rest.first().cloned().unwrap_or_default()).await),
        other => {
            eprintln!("unknown command {other}");
            std::process::exit(2);
        }
    }
}

fn uuid_like() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("c-{t}-{}", N.fetch_add(1, Ordering::Relaxed))
}
