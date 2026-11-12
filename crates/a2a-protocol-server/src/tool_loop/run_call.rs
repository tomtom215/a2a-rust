// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! One tool call inside the loop: the checks before it runs, running it, and
//! what is made of the result.

use std::collections::HashMap;

use tokio_util::sync::CancellationToken;

use super::{LoopError, Origin, ToolLoop};
use crate::rpc_span::{AgentRun, GenAiSpan};
use crate::tool_loop::model::ToolCall;
use crate::tool_loop::policy::{CallStatus, PolicyDecision, PolicyRequest};
use crate::tool_loop::run_support::parse_arguments;
use crate::tool_loop::tool::{Tool, ToolError, ToolInvocation};
use crate::tool_loop::{idempotency, schema};

impl ToolLoop {
    /// Why the policy refuses `call`, if there is one and it does.
    async fn denied(
        &self,
        call: &ToolCall,
        read_only: bool,
        arguments: &serde_json::Value,
        origin: Origin<'_>,
    ) -> Option<String> {
        let policy = self.policy.as_ref()?;
        let request = PolicyRequest {
            tool: &call.name,
            arguments,
            call_id: &call.id,
            read_only,
            recovered_from_text: origin.from_text,
            tenant: origin.identity.and_then(|i| i.tenant.as_deref()),
            principal: origin.identity.and_then(|i| i.principal.as_deref()),
            task: origin.identity.and_then(|i| i.task.as_deref()),
        };
        match policy.authorize(&request).await {
            PolicyDecision::Deny(why) => Some(why),
            PolicyDecision::Allow => None,
        }
    }

    /// The span of `call`, opened as it starts to run.
    pub(super) fn tool_span(&self, call: &ToolCall) -> GenAiSpan {
        let tool = self.tools.get(&call.name);
        let description = tool.map(|t| t.description());
        GenAiSpan::tool(
            &call.name,
            tool.is_some(),
            &call.id,
            description,
            &self.meter,
        )
    }

    /// Runs one call in its span and returns what goes back to the model.
    /// Only [`ToolError::Fatal`] and cancellation are errors here. A call
    /// whose key is in `done` is not run: the earlier result is returned,
    /// marked, and no span is opened and nothing counted in `run`, since
    /// nothing ran.
    pub(super) async fn call(
        &self,
        run: &AgentRun,
        call: &ToolCall,
        cancel: &CancellationToken,
        scope: &str,
        done: &mut HashMap<String, String>,
        origin: Origin<'_>,
    ) -> Result<(String, CallStatus), LoopError> {
        let tool = self.tools.get(&call.name);
        let arguments = parse_arguments(&call.arguments);
        let key = match (&tool, &arguments) {
            (Some(_), Ok(arguments)) => {
                idempotency::call_key(&self.secret, scope, &call.name, arguments)
            }
            _ => String::new(),
        };
        if let Some(earlier) = done.get(&key) {
            let content = format!(
                "not run again: an identical call already ran in this task and returned: {earlier}"
            );
            return Ok((content, CallStatus::NotRun));
        }
        run.tool_call();
        let span = self.tool_span(call);
        let Some(tool) = tool else {
            span.failed("unknown_tool");
            let content = format!(
                "error: no tool named '{}'; the tools are: {}",
                call.name,
                self.tools.names().join(", ")
            );
            return Ok((content, CallStatus::Failed));
        };
        let arguments = match arguments {
            Ok(arguments) => arguments,
            Err(e) => {
                span.failed("invalid_arguments");
                return Ok((format!("error: {e}"), CallStatus::Failed));
            }
        };
        // Before the policy: it is asked about calls that are well formed.
        if let Some(why) = schema::refusal(self.argument_check, &tool.parameters(), &arguments) {
            span.failed("invalid_arguments");
            return Ok((format!("error: {why}"), CallStatus::Failed));
        }
        if let Some(why) = self
            .denied(call, tool.read_only(), &arguments, origin)
            .await
        {
            span.failed("policy_denied");
            let content = format!("error: this call was not permitted: {why}");
            return Ok((content, CallStatus::Failed));
        }
        let invocation = ToolInvocation::new(arguments, key.clone());
        let result = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(LoopError::Cancelled),
            result = span.instrument(tool.call(invocation)) => result,
        };
        Self::settled(result, &span, (&**tool, call, key), done)
    }

    /// What a call that ran returns to the model and records.
    fn settled(
        result: Result<String, ToolError>,
        span: &GenAiSpan,
        (tool, call, key): (&dyn Tool, &ToolCall, String),
        done: &mut HashMap<String, String>,
    ) -> Result<(String, CallStatus), LoopError> {
        match result {
            Ok(output) => {
                span.succeeded();
                if !tool.read_only() {
                    done.insert(key, output.clone());
                }
                Ok((output, CallStatus::Ran))
            }
            Err(ToolError::Failed(e)) => {
                span.failed("tool_error");
                Ok((format!("error: {e}"), CallStatus::Failed))
            }
            Err(ToolError::Fatal(message)) => {
                span.failed("tool_fatal");
                Err(LoopError::Fatal {
                    tool: call.name.clone(),
                    message,
                })
            }
        }
    }
}
