// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The hostile-campaign target. Everything is chosen from the environment so
//! one binary can stand in for each deployment shape the campaign attacks:
//!
//! | variable | values |
//! |---|---|
//! | `PORT` | HTTP port (JSON-RPC at `/`, HTTP+JSON, WebSocket upgrades), on 127.0.0.1 |
//! | `GRPC_PORT` | serve gRPC there too (same handler) |
//! | `STORE` | `mem`, `sqlite`, `pg`, `tmem`, `tsqlite`, `tpg`, `enc-mem`, `enc-pg` |
//! | `DATABASE_URL`, `SQLITE_URL` | for the SQL stores |
//! | `AUTH` | `none`, `bearer` (`TOKENS=alice=tokA,bob=tokB`), `apikey`, `jwt` (`JWT_SECRET`, `JWT_ISS`, `JWT_AUD`) |
//! | `TENANT` | `none`, `path`, `header`, `token` (digest of the bearer) |
//! | `ISOLATE=1` | `isolate_tasks_by_caller` |
//! | `STEER=1` | `with_steering` (`STEER_CAP`, `STEER_MAX`) |
//! | `PUSH` | `guard` (default SSRF guard), `private` (guard off), `off` |
//! | `THROTTLE` | `N`: `AuthFailureThrottle` with N failures a window (`THROTTLE_PG=1`: shared counter) |
//! | `EXEC_TIMEOUT_SECS`, `MAX_CONN`, `QUEUE_CAP` | handler and listener bounds |
//! | `PARKED_MS`, `PURGE_MS` | parked-task expiry run every `PURGE_MS` |
//! | `NAME` | replica name for the log |
//! | `POOL` | pool size of the PostgreSQL task store (default 8; the push store gets at most 4) |
//! | `CLAIM_TTL_MS`, `CLAIM_RENEW_MS` | turn-claim timing (defaults 15 s and 1 s) |
//! | `LOG_NOTIFY` | `pool` or `direct`: `PostgresTaskStore::with_log_notifications` |

mod exec;
mod wiring;

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_server::dispatch::{CombinedDispatcher, WebSocketDispatcher};
use a2a_protocol_server::serve::{ServeConfig, Server};
use a2a_protocol_server::{GrpcConfig, GrpcDispatcher};

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

fn secs(k: &str) -> Option<Duration> {
    env(k).and_then(|v| v.parse().ok()).map(Duration::from_secs)
}

#[tokio::main]
async fn main() {
    let name = env("NAME").unwrap_or_else(|| "target".into());
    let handler = Arc::new(wiring::handler().await);
    wiring::spawn_purger().await;
    let port = env("PORT").expect("PORT");
    let mut serve = ServeConfig::new();
    if let Some(m) = env("MAX_CONN").and_then(|v| v.parse().ok()) {
        serve = serve.with_max_connections(m);
    }
    let ws = WebSocketDispatcher::new(Arc::clone(&handler)).accept_missing_version_header();
    let dispatcher = CombinedDispatcher::new(Arc::clone(&handler)).with_websocket(ws);
    if let Some(gp) = env("GRPC_PORT") {
        let g = GrpcDispatcher::new(Arc::clone(&handler), GrpcConfig::default());
        let l = tokio::net::TcpListener::bind(format!("127.0.0.1:{gp}"))
            .await
            .expect("grpc bind");
        tokio::spawn(async move {
            let r = g.serve_with_shutdown(l, std::future::pending::<()>()).await;
            eprintln!("[{name}] grpc ended: {r:?}");
        });
    }
    let server = Server::bind(&format!("127.0.0.1:{port}"))
        .await
        .expect("bind")
        .with_config(serve);
    eprintln!("[{}] serving on {port}", env("NAME").unwrap_or_default());
    let report = server
        .serve_with_shutdown(dispatcher, async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    eprintln!("[target] serve ended: {report:?}");
    let _ = handler.shutdown().await;
}
