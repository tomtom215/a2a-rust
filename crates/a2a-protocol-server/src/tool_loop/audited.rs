// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A tool call is on the audit record before it runs
//! (adversarial review A3, 2026-11-12).
//!
//! A [required](crate::audit::AuditLog::require_record) log makes sure a call
//! is recorded as it starts and keeps the record of how it ended. A tool call
//! inside a run is a finer action than the RPC that carries it, and may be the
//! irreversible one: sending the message, moving the money. [`AuditedPolicy`]
//! is the fail-closed boundary for it: it writes a `tool.action` record to the
//! tenant's chain, and **an allowed call whose record the log cannot write is
//! not run**.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_types::audit::{Actor, AuditRecord, digest_of, kind};

use super::policy::{PolicyDecision, PolicyRequest, ToolPolicy};
use crate::audit::AuditLog;
use crate::store::tenant::TenantContext;

/// What the model is told when an action cannot be put on the record.
const NOT_RECORDED: &str = "this action was not run: it could not be recorded in the audit log";

/// A [`ToolPolicy`] that records every call before it runs.
///
/// It asks the policy it wraps first (every call is allowed when it wraps none),
/// then writes a `tool.action` record: the tool, the call's id, whether it is
/// read-only or was recovered from text, the decision, the authenticated caller,
/// the task, and a SHA-256 digest of the arguments, never the arguments. The
/// record goes to the chain of the request's tenant, with the call's other
/// records.
///
/// `detail.decision` is one of:
///
/// * `allow`: the call is allowed and runs once this record is in the store,
///   written before the call, not spooled for later: if the store refuses it,
///   the call is answered as an error the model can read and is not run.
/// * `deny`: it is not run. A record the store refuses is kept as any other
///   record of a required log is; the call was not going to run.
/// * `approval_required`: the wrapped policy sent it to a person, and it does
///   not run now. Kept like a denial: nothing runs on it. Written again when the
///   run resumes, since the policy is asked again.
/// * `approved`: a person approved exactly this call (`detail.approvedBy`, the
///   identity the approval gate verified, which [`AuditPrivacy`] seals) and it
///   is about to run. Written durably like `allow`, immediately before the
///   call ([`ToolPolicy::approved`]): **an approved call whose record the store
///   refuses is not run** (adversarial review N02, 2026-11-13; until then an
///   approved call ran with no record of running, and a healthy store said
///   `deny` for it).
///
/// Install it with [`ToolLoop::with_policy`](super::ToolLoop::with_policy).
/// Records are written by the loop's own task, in the tenant of the request,
/// which is what [`ToolLoopExecutor`](super::ToolLoopExecutor) sets.
///
/// [`AuditPrivacy`]: crate::audit::AuditPrivacy
pub struct AuditedPolicy {
    log: Arc<AuditLog>,
    inner: Option<Box<dyn ToolPolicy>>,
}

impl std::fmt::Debug for AuditedPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditedPolicy")
            .field("log", &self.log)
            .field("wraps_a_policy", &self.inner.is_some())
            .finish()
    }
}

impl AuditedPolicy {
    /// Records every call to `log` and decides nothing else: each is allowed
    /// that the log can record.
    #[must_use]
    pub fn new(log: Arc<AuditLog>) -> Self {
        Self { log, inner: None }
    }

    /// Asks `inner` first. A call it denies is recorded as denied and not run;
    /// one it allows is recorded and then run.
    #[must_use]
    pub fn wrapping(mut self, inner: impl ToolPolicy) -> Self {
        self.inner = Some(Box::new(inner));
        self
    }

    /// The record of `request` and what was decided, or `None` if its
    /// arguments cannot be digested.
    fn record(request: &PolicyRequest<'_>, decision: &str) -> Option<AuditRecord> {
        let mut r = AuditRecord::new(TenantContext::current(), kind::TOOL_ACTION);
        r.actor = request.principal.map(|p| Actor::new(p, None));
        r.task_id = request.task.map(str::to_owned);
        r.digests
            .insert("arguments".to_owned(), digest_of(request.arguments).ok()?);
        for (key, value) in [
            ("tool", request.tool.into()),
            ("toolCallId", request.call_id.into()),
            ("decision", decision.into()),
            ("readOnly", request.read_only.into()),
            ("recoveredFromText", request.recovered_from_text.into()),
        ] {
            r.detail.insert(key.to_owned(), value);
        }
        Some(r)
    }

    /// Writes a record a call that runs depends on: now, not spooled. `Err`
    /// is what the model is told instead of the call running.
    async fn before_running(&self, record: Option<AuditRecord>) -> Result<(), String> {
        let Some(record) = record else {
            return Err(NOT_RECORDED.to_owned());
        };
        // Written here and now: an action must not run on the promise of a
        // record that a later retry might write.
        match self.log.append(record).await {
            Ok(_) => Ok(()),
            Err(_error) => {
                trace_error!(error = %_error, "audit: a tool action could not be recorded, so it was not run");
                self.log.note_failure();
                Err(NOT_RECORDED.to_owned())
            }
        }
    }

    /// Writes a record of a call that does not run: kept by a required log
    /// when the store refuses it, and waited on by nothing.
    async fn not_running(&self, record: AuditRecord) {
        if self.log.is_required() {
            let _ = self.log.append_required(record).await;
        } else {
            let _ = self.log.append(record).await;
        }
    }
}

impl ToolPolicy for AuditedPolicy {
    fn authorize<'a>(
        &'a self,
        request: &'a PolicyRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = PolicyDecision> + Send + 'a>> {
        Box::pin(async move {
            let decision = match &self.inner {
                Some(inner) => inner.authorize(request).await,
                None => PolicyDecision::Allow,
            };
            let word = match &decision {
                PolicyDecision::Allow => "allow",
                PolicyDecision::RequireApproval(_) => "approval_required",
                _ => "deny",
            };
            let record = Self::record(request, word);
            if matches!(decision, PolicyDecision::Allow) {
                return match self.before_running(record).await {
                    Ok(()) => decision,
                    Err(why) => PolicyDecision::Deny(why),
                };
            }
            // Not going to run now: the record is kept, and nothing waits on it.
            let Some(record) = record else {
                return PolicyDecision::Deny(NOT_RECORDED.to_owned());
            };
            self.not_running(record).await;
            decision
        })
    }

    fn approved<'a>(
        &'a self,
        request: &'a PolicyRequest<'a>,
        approver: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(inner) = &self.inner {
                inner.approved(request, approver).await?;
            }
            let record = Self::record(request, "approved").map(|mut r| {
                r.detail
                    .insert("approvedBy".to_owned(), serde_json::json!([approver]));
                r
            });
            self.before_running(record).await
        })
    }
}
