// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! One tool call inside the loop: the checks before it runs, running it, and
//! what is made of the result.

use std::collections::HashMap;

use tokio_util::sync::CancellationToken;

use super::{LoopError, Origin, ToolLoop};
use crate::rpc_span::{AgentRun, GenAiSpan};
use crate::tool_loop::loop_error::ApprovalNeeded;
use crate::tool_loop::model::ToolCall;
use crate::tool_loop::policy::{
    CallStatus, PolicyDecision, PolicyRequest, TrustedIdentity, call_digest,
};
use crate::tool_loop::run_support::parse_arguments;
use crate::tool_loop::tool::{Tool, ToolError, ToolInvocation};
use crate::tool_loop::{idempotency, schema};

/// What the policy decided about one call.
enum Gate {
    Allow,
    Deny(String),
    Approval(String),
}

impl ToolLoop {
    /// What the loop asks a policy about `call`: the call, and the identity
    /// of the request, never of the model.
    fn policy_request<'a>(
        call: &'a ToolCall,
        read_only: bool,
        arguments: &'a serde_json::Value,
        origin: Origin<'a>,
    ) -> PolicyRequest<'a> {
        PolicyRequest {
            tool: &call.name,
            arguments,
            call_id: &call.id,
            read_only,
            recovered_from_text: origin.from_text,
            tenant: origin.identity.and_then(|i| i.tenant.as_deref()),
            principal: origin.identity.and_then(|i| i.principal.as_deref()),
            task: origin.identity.and_then(|i| i.task.as_deref()),
        }
    }

    /// What the policy says of `request`.
    async fn gate(&self, request: &PolicyRequest<'_>) -> Gate {
        let Some(policy) = self.policy.as_ref() else {
            return Gate::Allow;
        };
        match policy.authorize(request).await {
            PolicyDecision::Deny(why) => Gate::Deny(why),
            PolicyDecision::RequireApproval(summary) => Gate::Approval(summary),
            _ => Gate::Allow,
        }
    }

    /// What a call that needs approval does now: `None` runs it (a person
    /// approved exactly this call), `Some` is what the call returns instead.
    fn approval_gate(
        call: &ToolCall,
        arguments: &serde_json::Value,
        summary: String,
        scope: &str,
        origin: Origin<'_>,
        span: &GenAiSpan,
    ) -> Option<Result<(String, CallStatus), LoopError>> {
        let who = |f: fn(&TrustedIdentity) -> Option<&str>| origin.identity.and_then(f);
        let digest = call_digest(
            &call.name,
            arguments,
            who(|i| i.tenant.as_deref()),
            who(|i| i.principal.as_deref()),
            who(|i| i.task.as_deref()),
        );
        match origin.approval {
            Some(v) if v.digest == digest && v.approved => None,
            Some(v) if v.digest == digest => {
                span.failed("approval_refused");
                Some(Ok((
                    format!("error: this call was not approved by {}", v.by),
                    CallStatus::Failed,
                )))
            }
            _ if !origin.can_park => {
                span.failed("approval_unavailable");
                Some(Ok((
                    "error: this action needs approval, and this loop has no journal to wait \
                     in; it was not run"
                        .to_owned(),
                    CallStatus::Failed,
                )))
            }
            _ => {
                span.failed("awaiting_approval");
                Some(Err(LoopError::AwaitingApproval(Box::new(ApprovalNeeded {
                    request_id: scope.to_owned(),
                    summary,
                    digest,
                    tool: call.name.clone(),
                    call_id: call.id.clone(),
                }))))
            }
        }
    }

    /// Everything that decides whether `call` runs, in order: the policy, a
    /// person's approval of exactly this call, and the policy told of that
    /// approval last. `None` runs it; `Some` is what it returns instead.
    async fn decide(
        &self,
        call: &ToolCall,
        read_only: bool,
        arguments: &serde_json::Value,
        scope: &str,
        origin: Origin<'_>,
        span: &GenAiSpan,
    ) -> Option<Result<(String, CallStatus), LoopError>> {
        let request = Self::policy_request(call, read_only, arguments, origin);
        match self.gate(&request).await {
            Gate::Allow => None,
            Gate::Deny(why) => {
                span.failed("policy_denied");
                let content = format!("error: this call was not permitted: {why}");
                Some(Ok((content, CallStatus::Failed)))
            }
            Gate::Approval(summary) => {
                if let Some(refusal) =
                    Self::approval_gate(call, arguments, summary, scope, origin, span)
                {
                    return Some(refusal);
                }
                // Approved, for exactly this call: the policy is told so,
                // last, and may still stop it (N02).
                if let (Some(policy), Some(verdict)) = (self.policy.as_ref(), origin.approval)
                    && let Err(why) = policy.approved(&request, &verdict.by).await
                {
                    span.failed("approval_unrecorded");
                    return Some(Ok((format!("error: {why}"), CallStatus::Failed)));
                }
                None
            }
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
        if let Some(refused) = self
            .decide(call, tool.read_only(), &arguments, scope, origin, &span)
            .await
        {
            return refused;
        }
        let invocation = ToolInvocation::new(arguments, key.clone());
        let result = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(LoopError::Cancelled),
            result = span.instrument(tool.call(invocation)) => result,
        };
        Self::settled(
            result,
            &span,
            (&**tool, call, key),
            done,
            self.byte_limits.result,
        )
    }

    /// What a call that ran returns to the model and records.
    fn settled(
        result: Result<String, ToolError>,
        span: &GenAiSpan,
        (tool, call, key): (&dyn Tool, &ToolCall, String),
        done: &mut HashMap<String, String>,
        limit: usize,
    ) -> Result<(String, CallStatus), LoopError> {
        // What the model is shown, and what an identical call is answered
        // with later, is bounded (N09).
        let result = result.map(|output| crate::tool_loop::limits::bound_result(output, limit));
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
                Ok((
                    crate::tool_loop::limits::bound_result(format!("error: {e}"), limit),
                    CallStatus::Failed,
                ))
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
