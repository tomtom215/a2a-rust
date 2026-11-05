// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The calls of a client that is not balanced, passed through its origin's
//! state: the pool's circuit breaker and in-flight cap (ADR 0050 decision 6).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::time::Instant;

use super::{CallGuard, OriginState, Ticket, outcome_of};
use crate::error::ClientResult;
use crate::streaming::EventStream;
use crate::transport::Transport;

/// Runs a unary call under `guard` and records its outcome.
pub async fn unary<F>(mut guard: CallGuard, call: F) -> ClientResult<serde_json::Value>
where
    F: Future<Output = ClientResult<serde_json::Value>>,
{
    let result = call.await;
    guard.record(outcome_of(&result));
    result
}

/// Opens a stream under `guard`, records the outcome of opening it, and
/// keeps the guard with the stream, so that it counts in flight until it is
/// dropped. The place under the in-flight cap is given back once it opens.
pub async fn stream<F>(mut guard: CallGuard, call: F) -> ClientResult<EventStream>
where
    F: Future<Output = ClientResult<EventStream>>,
{
    let result = call.await;
    guard.record(outcome_of(&result));
    guard.release_permit();
    // `AssertUnwindSafe`: dropping the guard only counts the stream out and
    // records nothing learned; a panic cannot leave it half done.
    result.map(|stream| stream.holding(std::panic::AssertUnwindSafe(guard)))
}

/// A [`Transport`] whose calls pass through one origin's state.
pub struct OriginTransport {
    inner: Box<dyn Transport>,
    state: Arc<OriginState>,
}

impl OriginTransport {
    pub const fn new(inner: Box<dyn Transport>, state: Arc<OriginState>) -> Self {
        Self { inner, state }
    }

    fn admit(&self) -> ClientResult<CallGuard> {
        let ticket: Ticket = self.state.admit(Instant::now())?;
        self.state.enter(ticket)
    }
}

impl Transport for OriginTransport {
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
            let guard = self.admit()?;
            unary(
                guard,
                self.inner
                    .send_request_with_query(method, params, extra_headers, query),
            )
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
            let guard = self.admit()?;
            stream(
                guard,
                self.inner
                    .send_streaming_request_with_query(method, params, extra_headers, query),
            )
            .await
        })
    }

    fn rpc_system(&self) -> Option<&'static str> {
        self.inner.rpc_system()
    }

    fn server_url(&self) -> Option<&str> {
        self.inner.server_url()
    }
}
