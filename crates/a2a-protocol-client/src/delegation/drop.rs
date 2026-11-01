// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What dropping a [`Delegation`] does. Moved out of `mod.rs` when resuming
//! took that file past the 500-line ratchet; nothing else changed.

use super::Delegation;

impl Drop for Delegation {
    /// Cancels an unsettled child in the background, on the current Tokio
    /// runtime. Outside a runtime nothing can be sent, and the drop is only
    /// logged.
    fn drop(&mut self) {
        if !self.armed || self.settled.is_some() {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            trace_warn!(
                task_id = self.task_id.as_deref().unwrap_or_default(),
                "delegation dropped outside a Tokio runtime; its child was not cancelled"
            );
            return;
        };
        let rest = Self {
            client: std::sync::Arc::clone(&self.client),
            tenant: self.tenant.take(),
            stream: self.stream.take(),
            task_id: self.task_id.take(),
            settled: None,
            armed: false,
            id_wait: self.id_wait,
            renewal: None,
            budget: None,
            idempotency_key: self.idempotency_key.take(),
            origin: self.origin.clone(),
            resume_policy: super::ResumePolicy::none(),
            last_event_id: self.last_event_id.take(),
            streak: 0,
            resumed_at: None,
            // The cancel the drop sends settles the record too.
            journal: self.journal.take(),
        };
        let origin = self.origin.clone();
        rt.spawn(origin.carry(async move {
            let done = rest.cancel().await;
            trace_debug!(
                task_id = done.task_id.as_deref().unwrap_or_default(),
                outcome = ?done.outcome,
                "delegation dropped before its child settled"
            );
            let _ = done;
        }));
    }
}
