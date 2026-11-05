// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The send path's side of the admission gate (ADR 0045): taking a slot for
//! the current tenant, and what a refusal is answered with.

use a2a_protocol_types::message::Message;
use tokio::sync::OwnedSemaphorePermit;

use super::RequestHandler;
use crate::admission::AdmissionLoad;
use crate::admission::gate::{AdmissionPermit, Refusal};
use crate::call_context::CallContext;
use crate::error::{ServerError, ServerResult};
use crate::store::tenant::TenantContext;

/// The slots a running turn holds, given back when its executor ends: the
/// tenant's (`max_concurrent_tasks`) and the admission gate's.
#[derive(Debug)]
pub struct TurnSlots {
    pub tenant: Option<OwnedSemaphorePermit>,
    pub admission: Option<AdmissionPermit>,
}

impl TurnSlots {
    /// Both slots, for the executor's future to hold until it ends.
    pub fn into_parts(self) -> (Option<OwnedSemaphorePermit>, Option<AdmissionPermit>) {
        (self.tenant, self.admission)
    }
}

impl RequestHandler {
    /// A slot of the admission gate for the current tenant, or `None` with no
    /// gate. Waits as the gate is configured, for no longer than what remains
    /// of the message's budget.
    ///
    /// # Errors
    ///
    /// [`ServerError::Overloaded`] when the gate is full and its queue is
    /// too, or the wait reached `max_wait`; [`ServerError::InvalidParams`]
    /// when the budget's deadline passed first, as for a budget that had
    /// already run out on arrival (`budget::requested`).
    pub(crate) async fn take_admission_slot(
        &self,
        message: &Message,
        call_ctx: &CallContext,
    ) -> ServerResult<Option<AdmissionPermit>> {
        let Some(gate) = &self.admission else {
            return Ok(None);
        };
        // A malformed budget is refused by the commit, with its own error;
        // here it only bounds the wait.
        let budget_left = super::budget::requested(message, call_ctx.received_at())
            .ok()
            .flatten()
            .and_then(|budget| budget.remaining_time());
        let tenant = TenantContext::current();
        match gate.acquire(&tenant, budget_left).await {
            Ok(permit) => Ok(Some(permit)),
            Err(Refusal::Full) => Err(ServerError::Overloaded(format!(
                "no admission slot free for tenant '{tenant}', and no room to wait for one"
            ))),
            Err(Refusal::Waited(waited)) => Err(ServerError::Overloaded(format!(
                "no admission slot came free for tenant '{tenant}' within {waited:?}"
            ))),
            Err(Refusal::Budget) => Err(ServerError::InvalidParams(
                "the budget's deadline passed while the send waited for admission".to_owned(),
            )),
        }
    }

    /// What the admission gate holds now (ADR 0045), or `None` without one.
    #[must_use]
    pub fn admission_load(&self) -> Option<AdmissionLoad> {
        self.admission
            .as_ref()
            .map(crate::admission::gate::AdmissionGate::load)
    }
}
