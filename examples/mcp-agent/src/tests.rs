// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for the MCP bridge and the loop above it, over a real MCP session
//! that never leaves the process.
//!
//! `tokio::io::duplex` gives client and server a pipe to each other, so every
//! request here is genuinely serialized, framed and parsed as MCP — the same
//! code the child-process transport drives, without a process to spawn. What
//! that cannot cover is the spawn itself and the real server's schemas;
//! `tests/child_process.rs` covers those against the shipped binary.
//!
//! The model is scripted rather than fixed, for the same reason as
//! `examples/rig-agent`: the branches that matter — a tool refusing, a
//! session dying, a model that never stops calling — are ones a live provider
//! reaches only by chance.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use rig_core::completion::message::{ToolCall, ToolFunction};
use rig_core::completion::{
    AssistantContent, CompletionError, CompletionModel, CompletionRequest, CompletionResponse,
    Usage,
};
use rig_core::streaming::StreamingCompletionResponse;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, ServiceExt, tool, tool_router};
use serde::Deserialize;
use serde_json::json;

use crate::agent::{AgentError, MAX_TURNS, McpAgent};
use crate::mcp::{McpError, McpTools, ToolOutcome};

// ── A tiny MCP server, in process ────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EchoArgs {
    /// The text to echo back.
    text: String,
}

#[derive(Clone)]
struct TestServer;

#[tool_router(server_handler)]
impl TestServer {
    /// Echo the given text back.
    #[tool]
    fn echo(&self, Parameters(args): Parameters<EchoArgs>) -> String {
        args.text
    }

    /// Always refuses, the way a tool reports a bad request.
    #[tool]
    fn always_refuses(&self) -> Result<String, ErrorData> {
        Err(ErrorData::invalid_params("nope, try echo instead", None))
    }
}

/// Connects an [`McpTools`] to a [`TestServer`] over an in-process pipe.
async fn connected() -> McpTools {
    let (server_side, client_side) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        if let Ok(service) = TestServer.serve(server_side).await {
            let _ = service.waiting().await;
        }
    });
    McpTools::connect(client_side)
        .await
        .expect("the in-process server offers two tools")
}

// ── A scripted model ─────────────────────────────────────────────────────────

#[derive(Clone)]
struct ScriptedModel {
    turns: Arc<Mutex<VecDeque<Vec<AssistantContent>>>>,
    seen: Arc<Mutex<Vec<CompletionRequest>>>,
}

impl ScriptedModel {
    fn new(turns: impl IntoIterator<Item = Vec<AssistantContent>>) -> Self {
        Self {
            turns: Arc::new(Mutex::new(turns.into_iter().collect())),
            seen: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn requests(&self) -> Vec<CompletionRequest> {
        self.seen
            .lock()
            .expect("no test panics while holding this")
            .clone()
    }
}

fn says(text: &str) -> Vec<AssistantContent> {
    vec![AssistantContent::text(text)]
}

fn calls(id: &str, name: &str, arguments: serde_json::Value) -> Vec<AssistantContent> {
    vec![AssistantContent::ToolCall(ToolCall::from_wire(
        id,
        ToolFunction::new(name.to_owned(), arguments),
    ))]
}

impl CompletionModel for ScriptedModel {
    async fn completion(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, CompletionError> {
        self.seen
            .lock()
            .expect("no test panics while holding this")
            .push(request);
        let next = self
            .turns
            .lock()
            .expect("no test panics while holding this")
            .pop_front();
        match next {
            Some(choice) => Ok(CompletionResponse::new(choice, Usage::new(), "scripted")),
            None => Err(CompletionError::ProviderError(
                "the script ran out of turns".to_owned(),
            )),
        }
    }

    async fn stream(
        &self,
        _request: CompletionRequest,
    ) -> Result<StreamingCompletionResponse, CompletionError> {
        Err(CompletionError::ProviderError(
            "streaming not scripted".to_owned(),
        ))
    }
}

async fn agent_with(
    turns: impl IntoIterator<Item = Vec<AssistantContent>>,
) -> McpAgent<ScriptedModel> {
    McpAgent::new(
        ScriptedModel::new(turns),
        "You are a test agent.",
        connected().await,
    )
}

// ── The bridge ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn discovery_reports_the_servers_tools_and_not_a_compiled_in_list() {
    let tools = connected().await;
    let mut names = tools.names();
    names.sort_unstable();
    assert_eq!(names, ["always_refuses", "echo"]);
}

#[tokio::test]
async fn a_discovered_schema_survives_the_crossing_to_rig() {
    // The schema the model is shown has to be the server's, field for field.
    // A bridge that rebuilt it would drift from whatever the server changed
    // to, and the model would call the tool wrongly for as long as that
    // lasted.
    let tools = connected().await;
    let echo = tools
        .definitions()
        .iter()
        .find(|t| t.name == "echo")
        .expect("echo was discovered");

    assert_eq!(echo.parameters["type"], "object");
    assert_eq!(echo.parameters["properties"]["text"]["type"], "string");
    assert_eq!(echo.parameters["required"][0], "text");
    assert!(
        echo.description.contains("Echo"),
        "the doc comment is the description the model reads: {}",
        echo.description
    );
}

#[tokio::test]
async fn a_tool_that_runs_returns_its_output() {
    let tools = connected().await;
    assert_eq!(
        tools
            .invoke("echo", &json!({ "text": "hello" }))
            .await
            .expect("the session is live"),
        ToolOutcome::Output("hello".to_owned())
    );
}

#[tokio::test]
async fn a_tool_that_refuses_is_not_a_transport_error() {
    // The whole distinction this bridge exists to keep. A refusal that
    // surfaced as `Err` would end the A2A task on a recoverable answer.
    let tools = connected().await;
    let outcome = tools
        .invoke("always_refuses", &json!({}))
        .await
        .expect("a refusal is not a session failure");
    match outcome {
        ToolOutcome::Refused(why) => assert!(why.contains("try echo instead"), "{why}"),
        ToolOutcome::Output(text) => panic!("a refusal was read as output: {text}"),
    }
}

#[tokio::test]
async fn a_dead_session_is_a_transport_error() {
    // The opposite case, and it has to stay distinguishable: nothing the
    // model does next can fix a server that is gone.
    let (server_side, client_side) = tokio::io::duplex(4096);
    let handle = tokio::spawn(async move {
        if let Ok(service) = TestServer.serve(server_side).await {
            let _ = service.waiting().await;
        }
    });
    let tools = McpTools::connect(client_side).await.expect("connected");
    handle.abort();

    let failed = tools
        .invoke("echo", &json!({ "text": "hello" }))
        .await
        .expect_err("the server is gone");
    assert!(matches!(failed, McpError::Transport(_)), "{failed:?}");
}

// ── The loop ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_tool_call_crosses_mcp_and_its_result_reaches_the_next_turn() {
    let agent = agent_with([
        calls("call-1", "echo", json!({ "text": "from the server" })),
        says("The server said: from the server."),
    ])
    .await;

    let answer = agent.prompt("say something").await.expect("two turns");
    assert_eq!(answer.text, "The server said: from the server.");
    assert_eq!(answer.trace.len(), 1);
    assert!(answer.trace[0].starts_with("echo("), "{}", answer.trace[0]);
    assert!(
        answer.trace[0].contains("from the server"),
        "{}",
        answer.trace[0]
    );
}

#[tokio::test]
async fn the_discovered_catalogue_is_sent_on_every_turn() {
    let model = ScriptedModel::new([
        calls("call-1", "echo", json!({ "text": "a" })),
        calls("call-2", "echo", json!({ "text": "b" })),
        says("done"),
    ]);
    let observer = model.clone();
    let agent = McpAgent::new(model, "You are a test agent.", connected().await);

    agent.prompt("go").await.expect("three turns");

    let requests = observer.requests();
    assert_eq!(requests.len(), 3);
    for (turn, request) in requests.iter().enumerate() {
        let mut names: Vec<&str> = request.tools.iter().map(|t| t.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["always_refuses", "echo"], "turn {turn}");
    }
}

#[tokio::test]
async fn a_refused_tool_reaches_the_model_and_the_task_survives() {
    let model = ScriptedModel::new([
        calls("call-1", "always_refuses", json!({})),
        says("That one refused, so here is an answer instead."),
    ]);
    let observer = model.clone();
    let agent = McpAgent::new(model, "You are a test agent.", connected().await);

    let answer = agent.prompt("go").await.expect("a refusal is recoverable");
    assert_eq!(
        answer.text,
        "That one refused, so here is an answer instead."
    );

    let replayed = serde_json::to_string(&observer.requests()[1].chat_history)
        .expect("rig messages serialize");
    assert!(
        replayed.contains("try echo instead"),
        "the model was not told why the tool refused: {replayed}"
    );
}

#[tokio::test]
async fn a_model_that_only_calls_tools_hits_the_turn_limit() {
    let agent = agent_with(
        std::iter::repeat_with(|| calls("call-n", "echo", json!({ "text": "again" })))
            .take(MAX_TURNS),
    )
    .await;

    let failed = agent.prompt("loop forever").await.expect_err("bounded");
    assert!(matches!(failed, AgentError::TurnLimit(n) if n == MAX_TURNS));
    assert!(failed.to_string().contains(&MAX_TURNS.to_string()));
}

#[tokio::test]
async fn an_answer_with_no_tool_calls_has_an_empty_trace() {
    let agent = agent_with([says("No tools needed.")]).await;
    let answer = agent.prompt("hello").await.expect("one turn");
    assert_eq!(answer.text, "No tools needed.");
    assert!(answer.trace.is_empty());
}
