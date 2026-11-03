// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A cap on the calls one client has in flight (audit H6).
//!
//! A client with nothing between it and its caller sends every call it is
//! given, so a supervisor that fans out to a saturated peer adds to the
//! saturation as fast as it can loop. With a cap, a call that finds the cap
//! reached is refused up front with [`ClientError::TooManyPendingRequests`],
//! which is retryable, so a retry policy backs off and tries again as
//! responses arrive; nothing is queued where the caller cannot see it.
//!
//! The cap counts the calls that are waiting for a response. A streaming call
//! counts until its stream has opened, not for as long as it stays open: a few
//! long watchers would otherwise take every permit.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use tokio::sync::Semaphore;

use super::Transport;
use crate::error::{ClientError, ClientResult};
use crate::streaming::EventStream;

/// A [`Transport`] that lets at most `limit` calls wait for a response at once.
pub struct InFlightTransport {
    inner: Box<dyn Transport>,
    permits: Semaphore,
    limit: usize,
}

impl InFlightTransport {
    /// Wraps `inner`. `limit` is at least 1; the builder refuses zero.
    pub(crate) fn new(inner: Box<dyn Transport>, limit: usize) -> Self {
        Self {
            inner,
            permits: Semaphore::new(limit),
            limit,
        }
    }

    const fn refused(&self) -> ClientError {
        ClientError::TooManyPendingRequests { limit: self.limit }
    }
}

impl Transport for InFlightTransport {
    fn send_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        self.send_request_with_query(method, params, extra_headers, &[])
    }

    fn send_streaming_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        self.send_streaming_request_with_query(method, params, extra_headers, &[])
    }

    fn send_request_with_query<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
        query: &'a [(String, String)],
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        Box::pin(async move {
            let _permit = self.permits.try_acquire().map_err(|_| self.refused())?;
            self.inner
                .send_request_with_query(method, params, extra_headers, query)
                .await
        })
    }

    fn send_streaming_request_with_query<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
        query: &'a [(String, String)],
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        Box::pin(async move {
            let _permit = self.permits.try_acquire().map_err(|_| self.refused())?;
            self.inner
                .send_streaming_request_with_query(method, params, extra_headers, query)
                .await
        })
    }
}

#[cfg(test)]
mod tests;
