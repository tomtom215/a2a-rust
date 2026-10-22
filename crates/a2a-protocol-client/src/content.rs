// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Verifying the provenance signatures on what an agent sends back
//! (ADR 0027).
//!
//! With keys set
//! ([`ClientBuilder::with_content_keys`](crate::ClientBuilder::with_content_keys),
//! feature `signing`), what the client receives is checked: in a stream's
//! events, and in the tasks and messages `send_message`, `get_task`,
//! `list_tasks` and `cancel_task` return.
//!
//! - Artifacts, status messages and direct replies, which only the agent
//!   writes, are verified whatever role a message claims; unsigned, they
//!   pass unless signatures are required.
//! - History messages are verified when they carry a signature and never
//!   required to: a task's history holds the caller's own messages, which
//!   no agent signs.
//!
//! Content whose signature does not verify, because it was altered or
//! signed by a key not in the set, fails the call with
//! [`ClientError::ContentNotVerified`]. Anyone able to alter content can
//! also remove its signature, so unsigned content is unverified, whoever
//! sent it. Only the content is covered: a task's state, an event's
//! `append` and `lastChunk`, and which task an artifact is attached to are
//! not signed.

use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::responses::{SendMessageResponse, TaskListResponse};
use a2a_protocol_types::task::Task;

use crate::error::ClientResult;

/// What received content is checked against: nothing, unless keys are set.
#[derive(Clone, Default)]
pub struct ContentKeys {
    #[cfg(feature = "signing")]
    keys: Option<std::sync::Arc<a2a_protocol_types::signing::Jwks>>,
    #[cfg(feature = "signing")]
    require_signed: bool,
}

impl std::fmt::Debug for ContentKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("ContentKeys");
        #[cfg(feature = "signing")]
        d.field("keys", &self.keys.as_ref().map(|k| k.keys.len()))
            .field("require_signed", &self.require_signed);
        d.finish()
    }
}

impl ContentKeys {
    /// Checking against `keys`.
    #[cfg(feature = "signing")]
    pub fn new(keys: a2a_protocol_types::signing::Jwks) -> Self {
        Self {
            keys: Some(std::sync::Arc::new(keys)),
            require_signed: false,
        }
    }

    /// Refusing unsigned content as well.
    #[cfg(feature = "signing")]
    pub const fn requiring_signatures(mut self) -> Self {
        self.require_signed = true;
        self
    }

    /// Checks the content `event` carries.
    pub fn event(&self, event: &StreamResponse) -> ClientResult<()> {
        match event {
            StreamResponse::Message(message) => self.check(message, "message", true),
            StreamResponse::StatusUpdate(update) => {
                update.status.message.as_ref().map_or(Ok(()), |message| {
                    self.check(message, "status message", true)
                })
            }
            StreamResponse::ArtifactUpdate(update) => {
                self.check(&update.artifact, "artifact", true)
            }
            StreamResponse::Task(task) => self.task(task),
            _ => Ok(()),
        }
    }

    /// Checks a task's status message, artifacts and history.
    pub fn task(&self, task: &Task) -> ClientResult<()> {
        if let Some(message) = &task.status.message {
            self.check(message, "status message", true)?;
        }
        for artifact in task.artifacts.iter().flatten() {
            self.check(artifact, "artifact", true)?;
        }
        for message in task.history.iter().flatten() {
            self.check(message, "history message", false)?;
        }
        Ok(())
    }

    /// Checks every task in a page.
    pub fn page(&self, page: &TaskListResponse) -> ClientResult<()> {
        page.tasks.iter().try_for_each(|task| self.task(task))
    }

    /// Checks what a blocking send returned.
    pub fn response(&self, response: &SendMessageResponse) -> ClientResult<()> {
        match response {
            SendMessageResponse::Task(task) => self.task(task),
            SendMessageResponse::Message(message) => self.check(message, "message", true),
            _ => Ok(()),
        }
    }

    #[cfg(feature = "signing")]
    fn check<C: a2a_protocol_types::provenance::Content + Clone>(
        &self,
        content: &C,
        what: &str,
        required: bool,
    ) -> ClientResult<()> {
        use a2a_protocol_types::provenance::{provenance_of, verify_content};

        use crate::error::ClientError;
        let Some(keys) = &self.keys else {
            return Ok(());
        };
        let signed = provenance_of(content)
            .map_err(|e| ClientError::ContentNotVerified(format!("{what}: {}", e.message)))?
            .is_some_and(|p| p.signature.is_some());
        if !signed {
            return if self.require_signed && required {
                Err(ClientError::ContentNotVerified(format!(
                    "{what} carries no provenance signature, and this client requires one"
                )))
            } else {
                Ok(())
            };
        }
        verify_content(content, keys)
            .map(drop)
            .map_err(|e| ClientError::ContentNotVerified(format!("{what}: {}", e.message)))
    }

    #[cfg(not(feature = "signing"))]
    #[allow(clippy::unnecessary_wraps, clippy::unused_self)] // the signing build's shape
    const fn check<C>(&self, _content: &C, _what: &str, _required: bool) -> ClientResult<()> {
        Ok(())
    }
}
