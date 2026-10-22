// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Signing what the server serves, and verifying what it is sent (ADR 0027).
//!
//! With a signer configured
//! ([`RequestHandlerBuilder::with_content_signer`](crate::RequestHandlerBuilder::with_content_signer),
//! feature `signing`), every artifact, status message and direct reply the
//! server sends is signed (`a2a_protocol_types::provenance`) over exactly
//! what is sent: the events of a stream, a push, and the tasks and messages
//! `SendMessage`, `GetTask`, `ListTasks` and `CancelTask` return.
//!
//! On the way out, not when the executor writes: an artifact streamed in
//! chunks is merged by the store, and the merge keeps no chunk's signature
//! valid (the SQLite and PostgreSQL append deltas write the new parts
//! alone). Signed as it is served, each chunk on the stream verifies as that
//! chunk, and the stored artifact `GetTask` returns verifies as the whole.
//!
//! What is signed is what only the executor writes: artifacts, status
//! messages and direct replies. Not a task's history: a caller's message is
//! stored there as it arrived, and a caller may send one with role `agent`,
//! so signing agent-role history would sign the caller's words as the
//! agent's (found by review, 2026-10-22). Left as it is: content already
//! carrying a signature, such as another agent's artifact forwarded
//! unchanged, whose signature is the point.

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::message::Message;
use a2a_protocol_types::responses::{SendMessageResponse, TaskListResponse};
use a2a_protocol_types::task::Task;

/// What the server signs content with: nothing, unless a signer is set.
#[derive(Clone, Default)]
pub struct Outbound {
    #[cfg(feature = "signing")]
    signer: Option<std::sync::Arc<a2a_protocol_types::provenance::ContentSigner>>,
}

impl std::fmt::Debug for Outbound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Outbound")
            .field("signs", &self.signs())
            .finish()
    }
}

impl Outbound {
    /// Signing with `signer`.
    #[cfg(feature = "signing")]
    pub fn signing(signer: a2a_protocol_types::provenance::ContentSigner) -> Self {
        Self {
            signer: Some(std::sync::Arc::new(signer)),
        }
    }

    /// Whether anything is signed.
    #[cfg_attr(not(feature = "signing"), allow(clippy::unused_self))]
    pub const fn signs(&self) -> bool {
        #[cfg(feature = "signing")]
        {
            self.signer.is_some()
        }
        #[cfg(not(feature = "signing"))]
        {
            false
        }
    }

    /// Signs the content `event` carries.
    ///
    /// # Errors
    ///
    /// When content carries provenance that is not provenance, or cannot be
    /// signed.
    pub fn sign_event(&self, event: &mut StreamResponse) -> A2aResult<()> {
        match event {
            StreamResponse::Message(message) => self.sign(message),
            StreamResponse::StatusUpdate(update) => update
                .status
                .message
                .as_mut()
                .map_or(Ok(()), |message| self.sign(message)),
            StreamResponse::ArtifactUpdate(update) => self.sign(&mut update.artifact),
            StreamResponse::Task(task) => self.sign_task(task),
            // `StreamResponse` is `#[non_exhaustive]`; an event this does
            // not know carries no content it knows how to sign.
            _ => Ok(()),
        }
    }

    /// Signs a task's status message and artifacts; its history is left as
    /// stored (see the module docs).
    ///
    /// # Errors
    ///
    /// As [`sign_event`](Self::sign_event).
    pub fn sign_task(&self, task: &mut Task) -> A2aResult<()> {
        if !self.signs() {
            return Ok(());
        }
        if let Some(message) = task.status.message.as_mut() {
            self.sign(message)?;
        }
        for artifact in task.artifacts.iter_mut().flatten() {
            self.sign(artifact)?;
        }
        Ok(())
    }

    /// Signs every task in a page.
    ///
    /// # Errors
    ///
    /// As [`sign_event`](Self::sign_event).
    pub fn sign_page(&self, page: &mut TaskListResponse) -> A2aResult<()> {
        page.tasks
            .iter_mut()
            .try_for_each(|task| self.sign_task(task))
    }

    /// Signs what a blocking `SendMessage` returns.
    ///
    /// # Errors
    ///
    /// As [`sign_event`](Self::sign_event).
    pub fn sign_response(&self, response: &mut SendMessageResponse) -> A2aResult<()> {
        match response {
            SendMessageResponse::Task(task) => self.sign_task(task),
            SendMessageResponse::Message(message) => self.sign(message),
            // `#[non_exhaustive]`, as above.
            _ => Ok(()),
        }
    }

    /// Signs what `SendMessage` returns: a stream frame by frame as it is
    /// read, a blocking answer now.
    ///
    /// # Errors
    ///
    /// As [`sign_event`](Self::sign_event).
    pub fn sign_sent(
        &self,
        sent: crate::handler::SendMessageResult,
    ) -> crate::error::ServerResult<crate::handler::SendMessageResult> {
        use crate::handler::SendMessageResult;
        Ok(match sent {
            SendMessageResult::Stream(reader) => {
                SendMessageResult::Stream(reader.with_outbound(self.clone()))
            }
            SendMessageResult::Response(mut response) => {
                self.sign_response(&mut response)?;
                SendMessageResult::Response(response)
            }
        })
    }

    #[cfg(feature = "signing")]
    fn sign<C: a2a_protocol_types::provenance::Content>(&self, content: &mut C) -> A2aResult<()> {
        use a2a_protocol_types::provenance::{provenance_of, sign_content};
        let Some(signer) = &self.signer else {
            return Ok(());
        };
        if provenance_of(content)?.is_some_and(|p| p.signature.is_some()) {
            return Ok(());
        }
        sign_content(content, signer)
    }

    #[cfg(not(feature = "signing"))]
    #[allow(clippy::unnecessary_wraps, clippy::unused_self)] // the signing build's shape
    const fn sign<C>(&self, _content: &mut C) -> A2aResult<()> {
        Ok(())
    }
}

/// What the server checks an arriving message's signature against: nothing,
/// unless keys are set.
#[derive(Clone, Default)]
pub struct Inbound {
    #[cfg(feature = "signing")]
    keys: Option<std::sync::Arc<a2a_protocol_types::signing::Jwks>>,
    #[cfg(feature = "signing")]
    require_signed: bool,
}

impl std::fmt::Debug for Inbound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("Inbound");
        #[cfg(feature = "signing")]
        d.field("keys", &self.keys.as_ref().map(|k| k.keys.len()))
            .field("require_signed", &self.require_signed);
        d.finish()
    }
}

impl Inbound {
    /// Checking against `keys`.
    #[cfg(feature = "signing")]
    pub fn verifying(keys: a2a_protocol_types::signing::Jwks) -> Self {
        Self {
            keys: Some(std::sync::Arc::new(keys)),
            require_signed: false,
        }
    }

    /// Refusing unsigned messages as well.
    #[cfg(feature = "signing")]
    pub const fn requiring_signatures(mut self) -> Self {
        self.require_signed = true;
        self
    }

    /// What `message`'s signature shows.
    ///
    /// # Errors
    ///
    /// `InvalidParams` when its provenance is malformed, its signature does
    /// not verify against the keys (the content was altered, or signed by a
    /// key not in the set), or it is unsigned and signatures are required.
    #[cfg(feature = "signing")]
    pub fn check(&self, message: &Message) -> A2aResult<crate::request_context::ContentCheck> {
        use a2a_protocol_types::error::A2aError;
        use a2a_protocol_types::provenance::{provenance_of, verify_content};

        use crate::request_context::ContentCheck;
        let Some(keys) = &self.keys else {
            return Ok(ContentCheck::NotChecked);
        };
        let signed = provenance_of(message)?.is_some_and(|p| p.signature.is_some());
        if !signed {
            if self.require_signed {
                return Err(A2aError::invalid_params(
                    "this agent accepts only signed messages: the message carries no \
                     provenance signature (a2a-rust.com/provenance)",
                ));
            }
            return Ok(ContentCheck::Unsigned);
        }
        verify_content(message, keys)
            .map(ContentCheck::Verified)
            .map_err(|e| {
                A2aError::invalid_params(format!(
                    "the message's provenance signature does not verify: {}",
                    e.message
                ))
            })
    }

    /// What `message`'s signature shows: without `signing`, never checked.
    #[cfg(not(feature = "signing"))]
    #[allow(clippy::unnecessary_wraps, clippy::unused_self)] // the signing build's shape
    pub const fn check(
        &self,
        _message: &Message,
    ) -> A2aResult<crate::request_context::ContentCheck> {
        Ok(crate::request_context::ContentCheck::NotChecked)
    }
}

/// A [`PushSender`](crate::push::PushSender) that signs each event before
/// handing it on.
#[cfg(feature = "signing")]
pub struct SigningPushSender {
    pub inner: std::sync::Arc<dyn crate::push::PushSender>,
    pub outbound: Outbound,
}

#[cfg(feature = "signing")]
impl crate::push::PushSender for SigningPushSender {
    fn send<'a>(
        &'a self,
        url: &'a str,
        event: &'a StreamResponse,
        config: &'a a2a_protocol_types::push::TaskPushNotificationConfig,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let mut signed = event.clone();
            self.outbound.sign_event(&mut signed)?;
            self.inner.send(url, &signed, config).await
        })
    }

    fn allows_private_urls(&self) -> bool {
        self.inner.allows_private_urls()
    }

    fn allows_webhook_url(&self, url: &str) -> bool {
        self.inner.allows_webhook_url(url)
    }

    fn requires_https(&self) -> bool {
        self.inner.requires_https()
    }

    fn max_delivery_duration(&self) -> Option<std::time::Duration> {
        self.inner.max_delivery_duration()
    }
}
