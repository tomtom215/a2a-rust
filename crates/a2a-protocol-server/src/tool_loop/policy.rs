// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What the loop lets a model do and what it tells the caller about it
//! (adversarial review T1, T2, T3, 2026-11-12): whether a call the model wrote
//! as text is run, what the caller's trace carries, and a policy asked before
//! every call runs.
//!
//! The defaults keep the loop's documented behaviour. [`ToolLoop::strict`] is
//! the profile for a deployment that takes tool calls to be actions with
//! consequences: no call is lifted out of text, the caller's trace is a receipt
//! (tool, call id, outcome) that holds neither the arguments nor the result, and
//! a [`ToolPolicy`] can be added for per-action authorisation.
//!
//! [`ToolLoop::strict`]: super::ToolLoop::strict

use std::future::Future;
use std::pin::Pin;

use serde_json::Value;

/// Whether a reply that is exactly one JSON object naming a tool in the set is
/// run as that call (see [`ToolLoop::with_text_call_recovery`]).
///
/// [`ToolLoop::with_text_call_recovery`]: super::ToolLoop::with_text_call_recovery
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TextCallRecovery {
    /// A reply written as a call is never run: it is the answer. What a deployment
    /// wants when a request for a JSON *example* must stay an example.
    Off,
    /// Only a call to a tool that declares [`read_only`](super::Tool::read_only) is
    /// recovered; a call to any other tool is the answer. A declaration, not a
    /// sandbox: it is as true as the tool's author made it.
    ReadOnly,
    /// Any tool in the set, as every release has done. The default.
    #[default]
    All,
}

impl TextCallRecovery {
    /// Whether a call to a tool that is (`read_only`) or is not read-only is
    /// run when written as text.
    #[must_use]
    pub const fn permits(self, read_only: bool) -> bool {
        match self {
            Self::Off => false,
            Self::ReadOnly => read_only,
            Self::All => true,
        }
    }
}

/// What a run's trace, the artifact the caller reads, carries for each call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TraceContent {
    /// `name(arguments) -> result`, each line cut at
    /// [`TRACE_LINE_LIMIT`](super::TRACE_LINE_LIMIT): a record for the caller of
    /// everything the model sent and was sent back. Truncation is not redaction.
    /// The default.
    #[default]
    Full,
    /// `name [call id]: outcome`, where the outcome is `ran`, `failed` or
    /// `not run again`: that the call happened and how it ended, with neither
    /// the arguments nor the result. The model still sees both; the caller
    /// does not.
    Receipt,
}

/// How a call ended, for a [`TraceContent::Receipt`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CallStatus {
    Ran,
    Failed,
    NotRun,
}

impl CallStatus {
    /// `name [id]: outcome`.
    pub(super) fn receipt(self, name: &str, id: &str) -> String {
        let outcome = match self {
            Self::Ran => "ran",
            Self::Failed => "failed",
            Self::NotRun => "not run again",
        };
        format!("{name} [{id}]: {outcome}")
    }
}

/// The policy's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PolicyDecision {
    /// Run the call.
    Allow,
    /// Do not run it. The reason is the call's result, as the model sees it
    /// (an error it can recover from), and it is not recorded as a success.
    Deny(String),
    /// Do not run it until a person approves *this call*. The text is what the
    /// person is shown.
    ///
    /// The run is parked in its [`StepJournal`](super::StepJournal) with the call's
    /// [digest](call_digest), and the task asks for approval under the
    /// approval extension ([`ApprovalGate`](crate::approval::ApprovalGate)); the call
    /// runs when a decision for that digest arrives from an approver the gate
    /// admits, and for no other call. A loop with no journal has nowhere to park
    /// and refuses the call as it does for [`Deny`](Self::Deny). The policy is
    /// asked again on resume, and a [`Deny`](Self::Deny) then still denies.
    RequireApproval(String),
}

/// `sha256:<hex>` of a call as an action: the tool, its arguments in
/// canonical form, and who it is for (tenant, requester, task).
///
/// What an approval is bound to: a decision for this digest approves this call
/// from this requester in this tenant and task, and not the same call from
/// another, nor another call by the same.
#[must_use]
pub fn call_digest(
    tool: &str,
    arguments: &Value,
    tenant: Option<&str>,
    principal: Option<&str>,
    task: Option<&str>,
) -> String {
    let mut text = String::new();
    super::idempotency::canonical(
        &serde_json::json!({
            "tool": tool, "arguments": arguments,
            "tenant": tenant, "principal": principal, "task": task,
        }),
        &mut text,
    );
    super::journaling::sha256_text(text.as_bytes())
}

/// One call, as a [`ToolPolicy`] is asked about it: the tool and arguments the
/// model chose, and **the request's identity, taken from the authenticated
/// request and not from anything the model wrote**.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct PolicyRequest<'a> {
    /// The tool, which is in the set (an unknown name is answered before the policy).
    pub tool: &'a str,
    /// The arguments, a JSON object.
    pub arguments: &'a Value,
    /// The call's id.
    pub call_id: &'a str,
    /// Whether the tool declares itself [`read_only`](super::Tool::read_only).
    pub read_only: bool,
    /// Whether the model wrote this call as text and the loop recovered it
    /// ([`TextCallRecovery`]), rather than the provider returning it as a call.
    pub recovered_from_text: bool,
    /// The request's tenant, when the loop runs under
    /// [`ToolLoopExecutor`](super::ToolLoopExecutor).
    pub tenant: Option<&'a str>,
    /// The authenticated caller's identity, when there is one.
    pub principal: Option<&'a str>,
    /// The task the call is made for, when the loop runs under
    /// [`ToolLoopExecutor`](super::ToolLoopExecutor).
    pub task: Option<&'a str>,
}

/// Asked before every tool call runs, whatever kind it is (native, recovered
/// from text, or repeated after a retry), and able to refuse it. Install with
/// [`ToolLoop::with_policy`](super::ToolLoop::with_policy).
///
/// An allowed task is not an authorised action: a task the caller may create
/// does not authorise every tool a model may reach for. The policy decides per
/// call, from the tool, the arguments and the request's identity.
///
/// # Order
///
/// For a call [`authorize`](Self::authorize) allows, nothing else is asked:
/// it runs. For one it answers [`PolicyDecision::RequireApproval`], the run is
/// parked; once a person approves exactly that call, the loop asks
/// `authorize` again (a [`Deny`](PolicyDecision::Deny) still denies), checks
/// the decision against the call's digest, and then, **immediately before
/// the call runs**, tells the policy who approved it with
/// [`approved`](Self::approved). Only that hook sees the effective decision;
/// a policy that must record or check an approved action does it there, and
/// an `Err` stops the call (adversarial review N02, 2026-11-13).
pub trait ToolPolicy: Send + Sync + 'static {
    /// Whether the call may run.
    fn authorize<'a>(
        &'a self,
        request: &'a PolicyRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = PolicyDecision> + Send + 'a>>;

    /// A call that [`authorize`](Self::authorize) sent for approval has been
    /// approved by `approver` (the authenticated identity the approval gate
    /// verified) and is about to run. `Err(reason)` stops it: it is not run,
    /// and `reason` is its result as the model sees it.
    ///
    /// The default lets it run.
    fn approved<'a>(
        &'a self,
        request: &'a PolicyRequest<'a>,
        approver: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        let _ = (request, approver);
        Box::pin(async { Ok(()) })
    }
}

/// The authenticated request's identity, handed to the loop by its executor.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct TrustedIdentity {
    /// The request's tenant.
    pub tenant: Option<String>,
    /// The authenticated caller.
    pub principal: Option<String>,
    /// The task the run belongs to.
    pub task: Option<String>,
}
