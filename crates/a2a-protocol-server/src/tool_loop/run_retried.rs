// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A rate-limited turn sent again (audit H17, ADR 0048): the loop's side of
//! [`ModelRetry`]. Moved out of `run.rs`, which it took past 500 lines.

use a2a_protocol_types::budget::BudgetHandle;
use tokio_util::sync::CancellationToken;

use super::super::model::{ChatRequest, ModelError, ModelTurn, TextSink};
use super::super::retry::ModelRetry;
use super::{LoopError, ToolLoop};

impl ToolLoop {
    /// Sends a turn the provider refused as rate limited
    /// ([`ModelError::RateLimited`]) again, as `retry` says, instead of
    /// ending the run on it (audit H17, ADR 0048). Off by default: without
    /// it a 429 ends the run, and the task ends `transient`.
    ///
    /// The wait honours the provider's `Retry-After`, is never longer than
    /// what remains of the run's budget, and is abandoned when the run is
    /// cancelled. A [`LimitedModel`](super::super::LimitedModel)'s slot is not held
    /// while the loop waits.
    #[must_use]
    pub const fn with_model_retry(mut self, retry: ModelRetry) -> Self {
        self.model_retry = Some(retry);
        self
    }

    /// [`turn`](Self::turn), sent again after a rate-limited answer for as
    /// long as [`with_model_retry`](Self::with_model_retry)'s policy and the
    /// budget allow. Each attempt has its own span.
    pub(super) async fn retried_turn(
        &self,
        request: ChatRequest<'_>,
        cancel: &CancellationToken,
        sink: Option<&TextSink>,
        budget: Option<&BudgetHandle>,
    ) -> Result<ModelTurn, LoopError> {
        let mut retries = 0;
        loop {
            let result = self.turn(request, cancel, sink).await;
            let Err(LoopError::Model(ModelError::RateLimited { retry_after, .. })) = &result else {
                return result;
            };
            let Some(wait) = self
                .model_retry
                .and_then(|policy| policy.next_wait(retries, *retry_after, budget))
            else {
                return result;
            };
            trace_info!(
                retry = retries + 1,
                wait_ms = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX),
                "tool loop: the model provider is rate limiting; sending the turn again"
            );
            tokio::select! {
                biased;
                () = cancel.cancelled() => return Err(LoopError::Cancelled),
                () = tokio::time::sleep(wait) => {}
            }
            retries += 1;
        }
    }
}
