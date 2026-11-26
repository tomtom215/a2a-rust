// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The key a delegation's message carries, and the start that sends it
//! (COR-06, DEC-08 as amended 2026-11-26).
//!
//! Every start gives a message with no idempotency key a fresh one, whatever
//! the client believes of its agent: a send whose response was lost can then
//! be sent again under the same key and reach the child the first created,
//! on any agent that honours keys. An agent that ignores the extension ignores
//! the key. A server of this SDK whose store cannot honour keys refuses the
//! keyed send, typed ([`idempotency::refuses_keys`]), before anything is
//! claimed or run; a key the start added itself is then taken off and the
//! message sent once more, so that agent is no worse off than before. A key
//! the caller set is never taken off: the caller asked for deduplication.
//!
//! The retry rule is unchanged: a client retries a keyed send only against an
//! agent known to honour keys (`retry::carries_idempotency_key`).

use std::sync::Arc;

use a2a_protocol_types::MessageSendParams;
use a2a_protocol_types::idempotency;

use super::{DEFAULT_ID_WAIT, Delegation, HandOffPolicy, Origin, ResumePolicy, fresh_key};
use crate::client::A2aClient;
use crate::error::{ClientError, ClientResult};

/// Gives `params` a fresh key when it has none; whether it did.
pub(super) fn add_key(params: &mut MessageSendParams) -> bool {
    let added = matches!(idempotency::key_of(&params.message), Ok(None));
    if added {
        // A fresh key is 64 hex digits, always valid, so this cannot fail.
        let _ = idempotency::set_key(&mut params.message, &fresh_key());
    }
    added
}

impl Delegation {
    /// Sends `params` and returns the handle following it; `added` is
    /// whether the start gave the message its key, which it may take off
    /// again on the typed refusal (module docs).
    pub(super) async fn start_keyed(
        client: Arc<A2aClient>,
        mut params: MessageSendParams,
        added: bool,
    ) -> ClientResult<Self> {
        let stream = match client.stream_message(params.clone()).await {
            Ok(stream) => stream,
            Err(ClientError::Protocol(e)) if added && idempotency::refuses_keys(&e) => {
                trace_debug!("delegation: the agent cannot honour keys; sending without one");
                idempotency::remove_key(&mut params.message);
                client.stream_message(params.clone()).await?
            }
            Err(e) => return Err(e),
        };
        let idempotency_key = idempotency::key_of(&params.message)
            .ok()
            .flatten()
            .map(str::to_owned);
        Ok(Self {
            client,
            tenant: params.tenant.clone(),
            stream: Some(stream),
            task_id: None,
            settled: None,
            armed: true,
            id_wait: DEFAULT_ID_WAIT,
            renewal: None,
            budget: None,
            idempotency_key,
            origin: Origin::current(),
            resume_policy: ResumePolicy::default(),
            last_event_id: None,
            streak: 0,
            resumed_at: None,
            opened_at: Some(tokio::time::Instant::now()),
            events_since_open: 0,
            message: Some(params.message),
            context_id: None,
            handed_back: false,
            continued: 0,
            hand_off_policy: HandOffPolicy::default(),
            journal: None,
        })
    }
}

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;
