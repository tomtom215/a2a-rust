// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`InferenceLimit`] and [`LimitedModel`]: at most so many turns in flight
//! at a model provider, shared by every loop that talks to it (audit H17,
//! ADR 0048).

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::Semaphore;

use super::model::{ChatRequest, ModelError, ModelTurn, TextSink, ToolModel};

/// How many turns may be in flight at once at one model provider.
///
/// Cloning shares the limit: give every [`LimitedModel`] that talks to the
/// same provider a clone of one `InferenceLimit`, and together they send at
/// most [`max`](Self::max) turns at a time. A turn past the limit waits for
/// one to finish, in arrival order (a fair semaphore), and the wait is
/// abandoned when the run is cancelled or the executor's timeout ends it.
///
/// ```rust
/// use a2a_protocol_server::tool_loop::InferenceLimit;
///
/// // llama-server started with `-np 2` serves two turns at once.
/// let limit = InferenceLimit::new(2);
/// let shared = limit.clone();
/// assert_eq!(shared.max(), 2);
/// assert_eq!(limit.available(), 2);
/// ```
#[derive(Clone)]
pub struct InferenceLimit {
    permits: Arc<Semaphore>,
    max: usize,
}

impl InferenceLimit {
    /// A limit of `max` turns in flight. `0` is taken as `1`: a limit that
    /// admitted nothing would hang every run.
    #[must_use]
    pub fn new(max: usize) -> Self {
        let max = max.max(1);
        Self {
            permits: Arc::new(Semaphore::new(max)),
            max,
        }
    }

    /// The most turns in flight at once.
    #[must_use]
    pub const fn max(&self) -> usize {
        self.max
    }

    /// How many more turns could start now without waiting.
    #[must_use]
    pub fn available(&self) -> usize {
        self.permits.available_permits()
    }
}

impl fmt::Debug for InferenceLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InferenceLimit")
            .field("max", &self.max)
            .field("available", &self.available())
            .finish_non_exhaustive()
    }
}

/// A [`ToolModel`] that takes a slot of an [`InferenceLimit`] for each turn
/// and gives it back when the turn ends, however it ends.
///
/// The slot is held for the turn only: a loop waiting to retry a
/// rate-limited turn ([`ToolLoop::with_model_retry`](super::ToolLoop::with_model_retry))
/// holds none, so another task's turn can use it.
///
/// ```rust,no_run
/// use a2a_protocol_server::tool_loop::{InferenceLimit, LimitedModel, OpenAiChatModel};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let limit = InferenceLimit::new(2);
/// let model = LimitedModel::new(
///     OpenAiChatModel::new("http://127.0.0.1:8080/v1", "qwen3:1.7b")?,
///     &limit,
/// );
/// # drop(model); Ok(()) }
/// ```
pub struct LimitedModel<M> {
    inner: M,
    limit: InferenceLimit,
}

impl<M: ToolModel> LimitedModel<M> {
    /// `inner`, held to `limit`, which it shares with every other model
    /// given a clone of it.
    #[must_use]
    pub fn new(inner: M, limit: &InferenceLimit) -> Self {
        Self {
            inner,
            limit: limit.clone(),
        }
    }

    /// The model this wraps.
    #[must_use]
    pub const fn inner(&self) -> &M {
        &self.inner
    }

    /// The limit this is held to.
    #[must_use]
    pub const fn limit(&self) -> &InferenceLimit {
        &self.limit
    }

    /// Runs `turn` holding a slot. The semaphore is never closed (nothing
    /// outside this module can reach it, and nothing here closes it), so the
    /// acquire cannot fail.
    async fn limited<F>(&self, turn: F) -> Result<ModelTurn, ModelError>
    where
        F: Future<Output = Result<ModelTurn, ModelError>>,
    {
        let _slot = self.limit.permits.acquire().await.ok();
        turn.await
    }
}

impl<M: ToolModel + fmt::Debug> fmt::Debug for LimitedModel<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LimitedModel")
            .field("inner", &self.inner)
            .field("limit", &self.limit)
            .finish()
    }
}

impl<M: ToolModel> ToolModel for LimitedModel<M> {
    fn model_name(&self) -> &str {
        self.inner.model_name()
    }

    fn provider_name(&self) -> &str {
        self.inner.provider_name()
    }

    fn server(&self) -> Option<(&str, u16)> {
        self.inner.server()
    }

    fn complete<'a>(
        &'a self,
        request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        Box::pin(self.limited(self.inner.complete(request)))
    }

    fn complete_streaming<'a>(
        &'a self,
        request: ChatRequest<'a>,
        text: &'a TextSink,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        Box::pin(self.limited(self.inner.complete_streaming(request, text)))
    }
}

#[cfg(test)]
#[path = "limit_tests.rs"]
mod tests;
