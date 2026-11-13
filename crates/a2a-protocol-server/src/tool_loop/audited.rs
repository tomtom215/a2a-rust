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
/// read-only or was recovered from text, the verdict, the authenticated caller,
/// the task, and a SHA-256 digest of the arguments, never the arguments. The
/// record goes to the chain of the request's tenant, with the call's other
/// records.
///
/// * An **allowed** call is run only once its record is in the store, written
///   before the call, not spooled for later: if the store refuses it, the call
///   is answered as an error the model can read and is not run.
/// * A **denied** call is recorded as well, and a record the store refuses is
///   kept as any other record of a required log is; the call was not going to
///   run.
///
/// Install it with [`ToolLoop::with_policy`](super::ToolLoop::with_policy).
/// Records are written by the loop's own task, in the tenant of the request,
/// which is what [`ToolLoopExecutor`](super::ToolLoopExecutor) sets.
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
    fn record(request: &PolicyRequest<'_>, allowed: bool) -> Option<AuditRecord> {
        let mut r = AuditRecord::new(TenantContext::current(), kind::TOOL_ACTION);
        r.actor = request.principal.map(|p| Actor::new(p, None));
        r.task_id = request.task.map(str::to_owned);
        r.digests
            .insert("arguments".to_owned(), digest_of(request.arguments).ok()?);
        for (key, value) in [
            ("tool", request.tool.into()),
            ("toolCallId", request.call_id.into()),
            ("decision", if allowed { "allow" } else { "deny" }.into()),
            ("readOnly", request.read_only.into()),
            ("recoveredFromText", request.recovered_from_text.into()),
        ] {
            r.detail.insert(key.to_owned(), value);
        }
        Some(r)
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
            let allowed = matches!(decision, PolicyDecision::Allow);
            let Some(record) = Self::record(request, allowed) else {
                return PolicyDecision::Deny(NOT_RECORDED.to_owned());
            };
            if !allowed {
                // Not going to run: the record is kept, and nothing waits on it.
                if self.log.is_required() {
                    let _ = self.log.append_required(record).await;
                } else {
                    let _ = self.log.append(record).await;
                }
                return decision;
            }
            // Written here and now: an action must not run on the promise of a
            // record that a later retry might write.
            match self.log.append(record).await {
                Ok(_) => decision,
                Err(_error) => {
                    trace_error!(error = %_error, "audit: a tool action could not be recorded, so it was not run");
                    self.log.note_failure();
                    PolicyDecision::Deny(NOT_RECORDED.to_owned())
                }
            }
        })
    }
}
