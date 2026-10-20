// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The tool loop end to end (ADR 0022): `ToolLoopExecutor` through a real
//! `RequestHandler`, and `OpenAiChatModel` against a stand-in
//! chat-completions server on loopback.

#![cfg(feature = "tool-loop")]

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use a2a_protocol_server::tool_loop::{
    ChatRequest, ModelError, ModelTurn, Tool, ToolCall, ToolError, ToolModel,
};
use serde_json::{Value, json};

#[path = "../support/raw_http.rs"]
mod raw_http;

mod executor;
mod openai;

/// Answers each turn with the next scripted reply; past the end, repeats
/// the last.
pub struct Scripted(Mutex<VecDeque<Result<ModelTurn, ModelError>>>);

impl Scripted {
    pub fn new(replies: Vec<Result<ModelTurn, ModelError>>) -> Self {
        Self(Mutex::new(replies.into()))
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
        _request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        let mut replies = self.0.lock().unwrap();
        let reply = if replies.len() > 1 {
            replies.pop_front().unwrap()
        } else {
            replies.front().cloned().unwrap()
        };
        Box::pin(async move { reply })
    }
}

/// A turn asking for `name` with `arguments`.
pub fn asks(name: &str, arguments: &str) -> Result<ModelTurn, ModelError> {
    Ok(ModelTurn::new(
        "",
        vec![ToolCall::new("c1", name, arguments)],
    ))
}

/// A turn answering `text`.
pub fn says(text: &str) -> Result<ModelTurn, ModelError> {
    Ok(ModelTurn::new(text, Vec::new()))
}

/// `service_status`: healthy for `checkout`, a recoverable error for any
/// other name, and a fatal one for `outage`.
pub struct Status;

impl Tool for Status {
    fn name(&self) -> &'static str {
        "service_status"
    }
    fn description(&self) -> &'static str {
        "Look up one service's state."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"service": {"type": "string"}},
            "required": ["service"]
        })
    }
    fn call<'a>(
        &'a self,
        arguments: Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        let result = match arguments.get("service").and_then(Value::as_str) {
            Some("checkout") => Ok(r#"{"state":"healthy"}"#.to_owned()),
            Some("outage") => Err(ToolError::Fatal("the inventory is down".to_owned())),
            other => Err(ToolError::Failed(format!("no service named {other:?}"))),
        };
        Box::pin(async move { result })
    }
}
