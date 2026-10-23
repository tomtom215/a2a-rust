// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The obedient model, the orchestrator's tools, and the agents they reach.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use a2a_protocol_sdk::client::{
    A2aClient, BearerAuthInterceptor, ClientBuilder, StaticTokenProvider,
};
use a2a_protocol_sdk::server::dispatch::JsonRpcDispatcher;
use a2a_protocol_sdk::server::tool_loop::{
    ChatMessage, ChatRequest, ModelError, ModelTurn, TokenUsage, Tool, ToolCall, ToolError,
    ToolInvocation, ToolLoop, ToolLoopExecutor, ToolModel, ToolSet,
};
use a2a_protocol_sdk::server::{Dispatcher, RequestHandler, RequestHandlerBuilder};
use a2a_protocol_sdk::types::message::Message;
use a2a_protocol_sdk::types::params::MessageSendParams;
use a2a_protocol_sdk::types::provenance::ContentSigner;
use a2a_protocol_sdk::types::responses::SendMessageResponse;
use a2a_protocol_sdk::types::task::Task;
use serde_json::Value;

/// A message id no other message in the run has.
pub fn fresh_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!("m-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

/// The `CALL <tool> <json>` lines in `text`, in order.
fn directives(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("CALL ")?;
            let (name, args) = rest.split_once(' ').unwrap_or((rest, "{}"));
            Some((name.to_owned(), args.trim().to_owned()))
        })
        .collect()
}

/// A model that obeys every instruction it reads.
///
/// Each turn it gathers the `CALL` lines in the user's message and in every
/// tool result so far, in conversation order, and makes the first call it
/// has not yet made; with none left it answers. It cannot tell its
/// operator's lines from an attacker's, which is the point. Each turn
/// reports `tokens_per_turn` tokens of usage, when set.
pub struct Obedient {
    pub tokens_per_turn: Option<u64>,
}

impl ToolModel for Obedient {
    fn model_name(&self) -> &'static str {
        "obedient"
    }
    fn provider_name(&self) -> &'static str {
        "harness"
    }
    fn complete<'a>(
        &'a self,
        request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        let mut wanted = Vec::new();
        let mut made = 0;
        for message in request.messages {
            match message {
                ChatMessage::User(text) => wanted.extend(directives(text)),
                ChatMessage::ToolResult { content, .. } => wanted.extend(directives(content)),
                ChatMessage::Assistant { tool_calls, .. } => made += tool_calls.len(),
                _ => {}
            }
        }
        let mut turn = match wanted.get(made) {
            Some((name, args)) => {
                ModelTurn::new("", vec![ToolCall::new(format!("c{made}"), name, args)])
            }
            None => ModelTurn::new("done", Vec::new()),
        };
        if let Some(tokens) = self.tokens_per_turn {
            turn = turn.with_usage(TokenUsage::new(Some(tokens), Some(0)));
        }
        Box::pin(async move { Ok(turn) })
    }
}

type Effect = Arc<
    dyn Fn(Value, String) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send>>
        + Send
        + Sync,
>;

/// A tool made from a closure given the arguments and the call's key.
pub struct FnTool {
    name: &'static str,
    read_only: bool,
    effect: Effect,
}

impl FnTool {
    pub fn new<F, Fut>(name: &'static str, read_only: bool, f: F) -> Self
    where
        F: Fn(Value, String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<String, ToolError>> + Send + 'static,
    {
        Self {
            name,
            read_only,
            effect: Arc::new(move |args, key| Box::pin(f(args, key))),
        }
    }
}

impl Tool for FnTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &'static str {
        "a harness tool"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({"type": "object"})
    }
    fn read_only(&self) -> bool {
        self.read_only
    }
    fn call<'a>(
        &'a self,
        invocation: ToolInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        (self.effect)(invocation.arguments, invocation.idempotency_key)
    }
}

/// Payments made, as (to, amount).
pub type Ledger = Arc<Mutex<Vec<(String, u64)>>>;

/// `pay {"to", "amount"}`: a side effect, recorded in `ledger`.
pub fn pay(ledger: &Ledger) -> FnTool {
    let ledger = Arc::clone(ledger);
    FnTool::new("pay", false, move |args, _key| {
        let ledger = Arc::clone(&ledger);
        async move {
            let to = args["to"].as_str().unwrap_or("?").to_owned();
            let amount = args["amount"].as_u64().unwrap_or(0);
            ledger.lock().unwrap().push((to.clone(), amount));
            Ok(format!("paid {amount} to {to}"))
        }
    })
}

/// Pages an attacker may have written, by URL.
pub type Pages = Arc<Mutex<HashMap<String, String>>>;

/// `fetch {"url"}`: read-only, serving `pages`.
pub fn fetch(pages: &Pages) -> FnTool {
    let pages = Arc::clone(pages);
    FnTool::new("fetch", true, move |args, _key| {
        let pages = Arc::clone(&pages);
        async move {
            let url = args["url"].as_str().unwrap_or_default();
            pages
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .ok_or_else(|| ToolError::Failed(format!("no page at {url}")))
        }
    })
}

/// The orchestrator: a tool loop over the obedient model.
pub fn orchestrator(
    tools: ToolSet,
    max_turns: usize,
    tokens_per_turn: Option<u64>,
) -> RequestHandler {
    orchestrator_builder(tools, max_turns, tokens_per_turn)
        .build()
        .expect("orchestrator")
}

pub fn orchestrator_builder(
    tools: ToolSet,
    max_turns: usize,
    tokens_per_turn: Option<u64>,
) -> RequestHandlerBuilder {
    RequestHandlerBuilder::new(ToolLoopExecutor::new(
        ToolLoop::new(Obedient { tokens_per_turn }, tools).with_max_turns(max_turns),
    ))
    .without_agent_card()
}

pub fn tools(list: Vec<FnTool>) -> ToolSet {
    let mut set = ToolSet::new();
    for tool in list {
        set.add(tool).expect("unique names");
    }
    set
}

/// Asks the orchestrator `text`, blocking, and returns the task.
pub async fn ask(orchestrator: &RequestHandler, message: Message) -> Task {
    match orchestrator
        .on_send_message(MessageSendParams::new(message), false, None)
        .await
    {
        Ok(a2a_protocol_sdk::server::SendMessageResult::Response(SendMessageResponse::Task(t))) => {
            t
        }
        other => panic!("expected a task, got {:?}", other.map(|_| ())),
    }
}

/// The tool trace artifact's lines.
pub fn trace(task: &Task) -> Vec<String> {
    task.artifacts
        .iter()
        .flatten()
        .filter(|a| a.id.0 == a2a_protocol_sdk::server::tool_loop::TRACE_ARTIFACT_ID)
        .flat_map(|a| a.parts.iter().filter_map(|p| p.text_content()))
        .flat_map(|t| t.lines().map(str::to_owned).collect::<Vec<_>>())
        .collect()
}

/// Serves `handler` over JSON-RPC on loopback; returns its URL.
pub async fn serve(handler: RequestHandler) -> String {
    let dispatcher: Arc<dyn Dispatcher> = Arc::new(JsonRpcDispatcher::new(Arc::new(handler)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let io = hyper_util::rt::TokioIo::new(stream);
            let d = Arc::clone(&dispatcher);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let d = Arc::clone(&d);
                    async move { Ok::<_, std::convert::Infallible>(d.dispatch(req).await) }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, service)
                .await;
            });
        }
    });
    format!("http://{addr}")
}

/// A client for `url`, sending `token` as its bearer credential when given.
pub fn client(
    url: &str,
    token: Option<&str>,
    builder: impl FnOnce(ClientBuilder) -> ClientBuilder,
) -> Arc<A2aClient> {
    let mut b = ClientBuilder::new(url);
    if let Some(token) = token {
        b = b.with_interceptor(BearerAuthInterceptor::new(Arc::new(
            StaticTokenProvider::new(token),
        )));
    }
    Arc::new(builder(b).build().expect("client"))
}

/// The text of every artifact a peer's task carries, or its status.
pub fn answer(task: &Task) -> String {
    let texts: Vec<&str> = task
        .artifacts
        .iter()
        .flatten()
        .flat_map(|a| a.parts.iter().filter_map(|p| p.text_content()))
        .collect();
    if texts.is_empty() {
        format!("the task is {}", task.status.state)
    } else {
        texts.join("\n")
    }
}

/// `ask_agent {"text"}`: sends `text` to the agent behind `peer` and returns
/// what it answered, or the client's error.
pub fn ask_agent(peer: Arc<A2aClient>, last_task: Arc<Mutex<Option<Task>>>) -> FnTool {
    FnTool::new("ask_agent", false, move |args, _key| {
        let (peer, last_task) = (Arc::clone(&peer), Arc::clone(&last_task));
        async move {
            let text = args["text"].as_str().unwrap_or_default().to_owned();
            let sent = peer
                .send_message(MessageSendParams::new(Message::user_text(fresh_id(), text)))
                .await
                .map_err(|e| ToolError::Failed(e.to_string()))?;
            match sent {
                SendMessageResponse::Task(task) => {
                    let said = answer(&task);
                    *last_task.lock().unwrap() = Some(task);
                    Ok(said)
                }
                _ => Ok("a message".to_owned()),
            }
        }
    })
}

/// The OpenSSL-made test keys `a2a-protocol-types` uses; they protect
/// nothing.
const ED25519_PKCS8: &str = "302e020100300506032b65700422042068359e9b51363ae026870556e5f802f411678bc856d0dd18008a0d835623358d";
const P256_PKCS8: &str = "308187020100301306072a8648ce3d020106082a8648ce3d030107046d306b02010104206eb44e03132c8d0b706290a4b3b5d95bb7a4314208b116d40b9e036833fba5d6a14403420004befb1e84fb839bc1040180bd22acb60a232c97270c0a3914f9854785f443743a12a75dcdcbd3b5f38cad54eb69211df7c288cf2fee4afb8ebd367026705290c1";

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

/// The worker's real signing key.
pub fn worker_key() -> ContentSigner {
    ContentSigner::ed25519("worker", &hex(ED25519_PKCS8)).expect("ed25519")
}

/// An impostor's key, under the worker's key id.
pub fn impostor_key() -> ContentSigner {
    ContentSigner::es256("worker", &hex(P256_PKCS8)).expect("es256")
}
