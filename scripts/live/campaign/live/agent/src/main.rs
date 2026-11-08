// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The campaign agent: a `ToolLoopExecutor` over an OpenAI-compatible model,
//! configured entirely from the environment, served on JSON-RPC, HTTP+JSON
//! and gRPC. It prints one `READY` line with the ports and logs every tool
//! effect (`tools.rs`). The drivers are `scripts/live/campaign/live/*.py`.
//!
//! | Variable | Meaning (default) |
//! |---|---|
//! | `C_MODEL_URL`, `C_MODEL` | the model's base URL (`http://127.0.0.1:11434/v1`) and name |
//! | `C_THINKING` | `1`: send no `enable_thinking: false` |
//! | `C_EXTRA_BODY` | a JSON object merged into every request |
//! | `C_TIMEOUT_SECS`, `C_MAX_RESPONSE_BYTES` | the model client's bounds |
//! | `C_PORT_RPC`, `C_PORT_REST`, `C_PORT_GRPC` | 0 (ephemeral); `-1` leaves a binding off |
//! | `C_TOOLS` | `inventory,evil,big,slow` subsets (`tools.rs`); default `inventory` |
//! | `C_PREAMBLE` | the system preamble |
//! | `C_MAX_TURNS` | the loop's turn bound |
//! | `C_RETRY` | `<retries>:<max delay seconds>`: rate-limit retry (default off) |
//! | `C_DRAFTS` | `1`: stream each turn's text |
//! | `C_STEER` | inbox capacity, `0` or unset: steering off; `C_STEER_MAX_BYTES` |
//! | `C_SKILLS_OPERATOR`, `C_SKILLS_PROJECT` | skills directories (project: marked trusted) |
//! | `C_AUTH` | `token:identity,...`: bearer authentication; tasks isolated by caller |
//! | `C_EXEC_TIMEOUT_SECS` | the handler's executor timeout |
//! | `C_TOOL_LOG` | a file the tool effects are appended to |
//! | `C_MODE`, `C_WORKER_URL`, `C_GRANT_METER`, `C_GRANT_TIME_MS` | `coordinator`: delegate to the worker (`coordinator.rs`) |
//! | `C_TELEMETRY` | `1`: `Telemetry` from the `OTEL_*` environment, metrics through `OtelMetrics` |

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::dispatch::RestDispatcher;
use a2a_protocol_sdk::server::skills::{SkillCatalog, SkillSource, SkillsConfig};
use a2a_protocol_sdk::server::tool_loop::{ModelRetry, OpenAiChatModel, ToolLoop, ToolLoopExecutor};
use a2a_protocol_sdk::server::{
    BearerTokenAuthInterceptor, GrpcConfig, GrpcDispatcher, SteeringConfig,
};

mod coordinator;
mod tools;

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn num<T: std::str::FromStr>(key: &str) -> Option<T> {
    env(key).and_then(|v| v.parse().ok())
}

fn model() -> Result<OpenAiChatModel, Box<dyn std::error::Error>> {
    let base = env("C_MODEL_URL").unwrap_or_else(|| "http://127.0.0.1:11434/v1".to_owned());
    let name = env("C_MODEL").unwrap_or_else(|| "qwen3:1.7b".to_owned());
    let mut model = OpenAiChatModel::new(&base, name)?;
    if let Some(secs) = num::<u64>("C_TIMEOUT_SECS") {
        model = model.with_timeout(Duration::from_secs(secs));
    }
    if let Some(max) = num::<usize>("C_MAX_RESPONSE_BYTES") {
        model = model.with_max_response_bytes(max);
    }
    let mut extra = serde_json::Map::new();
    if env("C_THINKING").as_deref() != Some("1") {
        extra.insert(
            "chat_template_kwargs".to_owned(),
            serde_json::json!({"enable_thinking": false}),
        );
    }
    if let Some(more) = env("C_EXTRA_BODY") {
        let serde_json::Value::Object(more) = serde_json::from_str(&more)? else {
            return Err("C_EXTRA_BODY must be a JSON object".into());
        };
        extra.extend(more);
    }
    if !extra.is_empty() {
        model = model.with_extra_body(extra);
    }
    Ok(model)
}

/// `-1` is off; otherwise the port to bind (0 is ephemeral).
async fn listener(key: &str) -> Result<Option<tokio::net::TcpListener>, std::io::Error> {
    let port = num::<i32>(key).unwrap_or(0);
    if port < 0 {
        return Ok(None);
    }
    tokio::net::TcpListener::bind(("127.0.0.1", u16::try_from(port).unwrap_or(0)))
        .await
        .map(Some)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tools::open_log();
    // Kept until `main` returns, which flushes it.
    let telemetry = if env("C_TELEMETRY").as_deref() == Some("1") {
        let t = Telemetry::builder()
            .with_default_service_name("campaign-agent")
            .with_export_where_configured()
            .build()?;
        t.install_subscriber();
        Some(t)
    } else {
        None
    };

    let mut tool_loop = ToolLoop::new(
        model()?,
        tools::catalogue(&env("C_TOOLS").unwrap_or_else(|| "inventory".to_owned()))?,
    )
    .with_preamble(env("C_PREAMBLE").unwrap_or_else(|| {
        "You are an operations agent. Call the tools yourself to answer; never ask the user \
         to call a tool. Report values exactly as the tools returned them."
            .to_owned()
    }))
    .with_agent_name("campaign-agent");
    if let Some(turns) = num::<usize>("C_MAX_TURNS") {
        tool_loop = tool_loop.with_max_turns(turns);
    }
    // `C_RETRY=<retries>:<max delay seconds>` turns on rate-limit retry.
    if let Some((retries, delay)) = env("C_RETRY").as_deref().and_then(|v| v.split_once(':')) {
        tool_loop = tool_loop.with_model_retry(
            ModelRetry::new()
                .with_max_retries(retries.parse().unwrap_or(3))
                .with_max_delay(Duration::from_secs(delay.parse().unwrap_or(30))),
        );
    }
    let mut executor = ToolLoopExecutor::new(tool_loop);
    // Skills are discovered after the preamble is set, as the docs say.
    let mut config = SkillsConfig::new();
    let mut any = false;
    if let Some(dir) = env("C_SKILLS_OPERATOR") {
        config = config.with_source(SkillSource::operator(dir));
        any = true;
    }
    if let Some(dir) = env("C_SKILLS_PROJECT") {
        config = config.with_source(SkillSource::project(dir).with_trusted(true));
        any = true;
    }
    if any {
        let catalog = SkillCatalog::discover(&config);
        for diagnostic in catalog.diagnostics() {
            println!("SKILL-DIAGNOSTIC {diagnostic}");
        }
        println!("SKILLS loaded={}", catalog.skills().len());
        executor = executor.with_skills(&catalog)?;
    }
    if env("C_DRAFTS").as_deref() == Some("1") {
        executor = executor.with_drafts();
    }
    let steer = num::<usize>("C_STEER").unwrap_or(0);
    if steer > 0 {
        executor = executor.with_steering();
    }

    let mut builder = if env("C_MODE").as_deref() == Some("coordinator") {
        let worker = a2a_protocol_sdk::client::ClientBuilder::new(
            env("C_WORKER_URL").ok_or("C_WORKER_URL names the worker")?,
        )
        .with_timeout(Duration::from_secs(900))
        .build()?;
        RequestHandlerBuilder::new(coordinator::Coordinator {
            worker: Arc::new(worker),
            ask: num("C_GRANT_METER").unwrap_or(u64::MAX),
            ask_time: num::<u64>("C_GRANT_TIME_MS").map(Duration::from_millis),
        })
        .without_agent_card()
    } else {
        RequestHandlerBuilder::new(executor).without_agent_card()
    };
    if let Some(secs) = num::<u64>("C_EXEC_TIMEOUT_SECS") {
        builder = builder.with_executor_timeout(Duration::from_secs(secs));
    }
    if steer > 0 {
        let mut cfg = SteeringConfig::new().with_inbox_capacity(steer);
        if let Some(bytes) = num::<usize>("C_STEER_MAX_BYTES") {
            cfg = cfg.with_max_message_size(bytes);
        }
        builder = builder.with_steering(cfg);
    }
    if let Some(auth) = env("C_AUTH") {
        let pairs: Vec<(String, String)> = auth
            .split(',')
            .filter_map(|p| p.split_once(':'))
            .map(|(t, i)| (t.to_owned(), i.to_owned()))
            .collect();
        builder = builder.with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens(pairs));
    }
    if let Some(t) = &telemetry {
        builder = builder.with_metrics(t.otel_metrics());
    }
    let handler = Arc::new(builder.build()?);

    let mut ready = String::from("READY");
    if let Some(l) = listener("C_PORT_RPC").await? {
        ready += &format!(" rpc={}", l.local_addr()?.port());
        let server = Server::from_listener(l);
        tokio::spawn(server.serve_with_shutdown(
            JsonRpcDispatcher::new(Arc::clone(&handler)),
            std::future::pending(),
        ));
    }
    if let Some(l) = listener("C_PORT_REST").await? {
        ready += &format!(" rest={}", l.local_addr()?.port());
        let server = Server::from_listener(l);
        tokio::spawn(server.serve_with_shutdown(
            RestDispatcher::new(Arc::clone(&handler)),
            std::future::pending(),
        ));
    }
    if let Some(l) = listener("C_PORT_GRPC").await? {
        ready += &format!(" grpc={}", l.local_addr()?.port());
        tokio::spawn(
            GrpcDispatcher::new(Arc::clone(&handler), GrpcConfig::default())
                .serve_with_shutdown(l, std::future::pending()),
        );
    }
    println!("{ready}");
    tokio::signal::ctrl_c().await?;
    Ok(())
}
