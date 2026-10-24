// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The loop against a scripted model: what each kind of reply does to the
//! conversation, the trace and the outcome. The scripted model and the
//! test tools are here; the tests are in the three modules below.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::*;
use crate::tool_loop::model::{ModelError, ModelTurn, ToolSpec};
use crate::tool_loop::tool::{DuplicateTool, Tool, ToolInvocation};

/// What the scripted model saw on each turn.
#[derive(Debug, Clone)]
struct Seen {
    preamble: String,
    messages: Vec<ChatMessage>,
    tools: Vec<ToolSpec>,
}

/// Answers each turn with the next scripted reply; past the end, repeats
/// the last one when `repeat` is set and fails otherwise.
struct Scripted {
    replies: Mutex<VecDeque<Result<ModelTurn, ModelError>>>,
    last: Mutex<Option<Result<ModelTurn, ModelError>>>,
    repeat: bool,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Scripted {
    fn new(replies: Vec<Result<ModelTurn, ModelError>>) -> (Self, Arc<Mutex<Vec<Seen>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let model = Self {
            replies: Mutex::new(replies.into()),
            last: Mutex::new(None),
            repeat: false,
            seen: Arc::clone(&seen),
        };
        (model, seen)
    }

    fn forever(reply: ModelTurn) -> (Self, Arc<Mutex<Vec<Seen>>>) {
        let (mut model, seen) = Self::new(vec![Ok(reply)]);
        model.repeat = true;
        (model, seen)
    }
}

impl ToolModel for Scripted {
    fn model_name(&self) -> &'static str {
        "scripted"
    }
    fn provider_name(&self) -> &'static str {
        "test"
    }
    fn complete<'a>(
        &'a self,
        request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        self.seen.lock().unwrap().push(Seen {
            preamble: request.preamble.to_owned(),
            messages: request.messages.to_vec(),
            tools: request.tools.to_vec(),
        });
        let next = self.replies.lock().unwrap().pop_front();
        let reply = match next {
            Some(reply) => {
                *self.last.lock().unwrap() = Some(reply.clone());
                reply
            }
            None if self.repeat => self.last.lock().unwrap().clone().unwrap(),
            None => Err(ModelError::Other("script exhausted".to_owned())),
        };
        Box::pin(async move { reply })
    }
}

/// A model whose turn never finishes.
struct Hanging;
impl ToolModel for Hanging {
    fn model_name(&self) -> &'static str {
        "hanging"
    }
    fn provider_name(&self) -> &'static str {
        "test"
    }
    fn complete<'a>(
        &'a self,
        _request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        Box::pin(std::future::pending())
    }
}

/// What a test tool does when called.
type Behaviour = Box<dyn Fn(Value) -> Result<String, ToolError> + Send + Sync>;

/// A tool that counts its calls, records each call's key, and answers with
/// `behaviour`.
struct TestTool {
    name: &'static str,
    calls: Arc<AtomicUsize>,
    keys: Arc<Mutex<Vec<String>>>,
    behaviour: Behaviour,
    read_only: bool,
}

impl TestTool {
    fn new(
        name: &'static str,
        behaviour: impl Fn(Value) -> Result<String, ToolError> + Send + Sync + 'static,
    ) -> (Self, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let tool = Self {
            name,
            calls: Arc::clone(&calls),
            keys: Arc::new(Mutex::new(Vec::new())),
            behaviour: Box::new(behaviour),
            read_only: false,
        };
        (tool, calls)
    }

    /// Declares the tool read-only.
    fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }
}

impl Tool for TestTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &'static str {
        "a test tool"
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"service": {"type": "string"}}})
    }
    fn read_only(&self) -> bool {
        self.read_only
    }
    fn call<'a>(
        &'a self,
        invocation: ToolInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.keys.lock().unwrap().push(invocation.idempotency_key);
        let result = (self.behaviour)(invocation.arguments);
        Box::pin(async move { result })
    }
}

/// `service_status`, a read-only lookup answering for `checkout` and
/// failing for anything else.
fn status_tool() -> (TestTool, Arc<AtomicUsize>) {
    let (tool, calls) = TestTool::new("service_status", |args| {
        match args.get("service").and_then(Value::as_str) {
            Some("checkout") => Ok(r#"{"state":"healthy"}"#.to_owned()),
            Some(other) => Err(ToolError::Failed(format!(
                "no service named '{other}'; call list_services"
            ))),
            None => Err(ToolError::Failed("'service' is required".to_owned())),
        }
    });
    (tool.read_only(), calls)
}

fn call(id: &str, name: &str, arguments: &str) -> ToolCall {
    ToolCall::new(id, name, arguments)
}

#[allow(clippy::unnecessary_wraps)] // a scripted reply, like the `Err` ones beside it
fn calls(calls: Vec<ToolCall>) -> Result<ModelTurn, ModelError> {
    Ok(ModelTurn::new("", calls))
}

#[allow(clippy::unnecessary_wraps)] // a scripted reply, like the `Err` ones beside it
fn text(text: &str) -> Result<ModelTurn, ModelError> {
    Ok(ModelTurn::new(text, Vec::new()))
}

fn tools(list: Vec<TestTool>) -> ToolSet {
    let mut set = ToolSet::new();
    for tool in list {
        set.add(tool).unwrap();
    }
    set
}

async fn run(tool_loop: &ToolLoop) -> Result<Answer, LoopError> {
    tool_loop
        .run("how is checkout?", &CancellationToken::new())
        .await
}

/// The tool results the model was sent on turn `turn`, as (id, content).
fn results(seen: &[Seen], turn: usize) -> Vec<(String, String)> {
    seen[turn]
        .messages
        .iter()
        .filter_map(|m| match m {
            ChatMessage::ToolResult {
                call_id, content, ..
            } => Some((call_id.clone(), content.clone())),
            _ => None,
        })
        .collect()
}

mod budget;
mod cancel;
mod idempotency;
mod recovery;
mod turns;

/// A model that does not say where it is served records no server: the
/// default is `None`, not an empty host.
#[test]
fn a_model_names_no_server_unless_it_says_where_it_is_served() {
    let (model, _) = Scripted::new(vec![]);
    assert_eq!(model.server(), None);
}
