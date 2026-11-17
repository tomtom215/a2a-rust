// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What one model reply is: the answer, or the calls it asks for, checked
//! before any of them runs.

use super::ToolLoop;
use super::model::{ChatMessage, ToolCall};
use super::policy::{CallStatus, TraceContent};
use super::recover;
use super::run::Progress;
use super::run_support::{give_ids, limit_calls, repeated_id};
use super::steering::Side;
use super::trace;

/// What one model reply is.
pub(super) enum Turn {
    /// The answer, and why the model stopped.
    Answer(String, Option<String>),
    /// Calls to run, and whether the model wrote them as text.
    Calls(Vec<ToolCall>, bool),
    /// Calls two of which share this id: none is run (N08).
    Ambiguous(String),
}

impl ToolLoop {
    /// What a reply is: the calls it asks for, with the assistant turn pushed
    /// to `messages`, or the answer.
    pub(super) fn turn_of(
        &self,
        reply: super::model::ModelTurn,
        turn: usize,
        names: &[String],
        side: &Side<'_>,
        progress: &mut Progress,
    ) -> Turn {
        let mut text = reply.text;
        let mut calls = reply.tool_calls;
        let mut from_text = false;
        limit_calls(
            &mut calls,
            self.max_calls_per_turn,
            &mut progress.trace,
            side.trace,
        );
        if calls.is_empty() {
            let recovered = self.recovered(&text, names);
            from_text = recovered.is_some();
            let Some((name, arguments)) = recovered else {
                return Turn::Answer(text, reply.finish_reason);
            };
            // The model wrote the call as text: run it as the call it is, and
            // keep it in the conversation as one.
            calls.push(ToolCall::new(
                format!("recovered-{turn}"),
                name,
                arguments.to_string(),
            ));
            text.clear();
        }
        give_ids(&mut calls, turn, &progress.messages);
        if let Some(id) = repeated_id(&calls) {
            return Turn::Ambiguous(id);
        }
        progress.messages.push(ChatMessage::Assistant {
            text,
            tool_calls: calls.clone(),
        });
        Turn::Calls(calls, from_text)
    }

    /// The call a reply written as text is, if it is one the loop's
    /// [`TextCallRecovery`] runs.
    fn recovered(&self, text: &str, names: &[String]) -> Option<(String, serde_json::Value)> {
        recover::text_call(text, names).filter(|(name, _)| {
            let read_only = self.tools.get(name).is_some_and(|t| t.read_only());
            self.text_recovery.permits(read_only)
        })
    }

    /// The line `call` leaves in the caller's trace, as [`TraceContent`] says.
    pub(super) fn trace_line(&self, call: &ToolCall, content: &str, status: CallStatus) -> String {
        match self.trace_content {
            TraceContent::Full => trace::line(&call.name, &call.arguments, content),
            TraceContent::Receipt => status.receipt(&call.name, &call.id),
        }
    }
}
