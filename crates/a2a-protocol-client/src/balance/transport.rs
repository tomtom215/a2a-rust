// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`BalancedTransport`]: a [`Transport`] over a set of endpoints, with
//! hedging of idempotent unary calls (ADR 0050).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::time::Instant;

use super::members::{Member, Members, Set, next_pick};
use super::selection::choose;
use super::{BalancePolicy, EndpointSource};
use crate::error::ClientResult;
use crate::origin::{CallGuard, transport as origin_calls};
use crate::retry::budget::Ledger;
use crate::streaming::EventStream;
use crate::transport::Transport;

pub use super::members::Factory;

/// Calls spread over the endpoints an [`EndpointSource`] gives.
pub struct BalancedTransport {
    members: Arc<Members>,
    /// The client's own endpoint, which names the set in errors.
    name: String,
    max_ejected_percent: u8,
    hedge: Option<(Duration, Ledger)>,
    peer_honours_idempotency: bool,
    picks: AtomicU64,
}

impl BalancedTransport {
    pub fn new(
        name: String,
        source: Arc<dyn EndpointSource>,
        factory: Factory,
        policy: &BalancePolicy,
        peer_honours_idempotency: bool,
    ) -> Self {
        Self {
            members: Arc::new(Members::new(source, factory)),
            name,
            max_ejected_percent: policy.max_ejected_percent,
            hedge: policy.hedge.map(|h| (h.delay, Ledger::new(h.budget))),
            peer_honours_idempotency,
            picks: AtomicU64::new(0),
        }
    }

    /// Chooses an endpoint other than `exclude` and starts a call to it.
    fn pick(
        &self,
        set: &Set,
        exclude: Option<&Arc<Member>>,
    ) -> ClientResult<(Arc<Member>, CallGuard)> {
        let choice = choose(
            set,
            exclude,
            self.max_ejected_percent,
            &self.name,
            Instant::now(),
        )?;
        let guard = choice.member.state.enter(choice.ticket)?;
        choice
            .member
            .last_pick
            .store(next_pick(&self.picks), Ordering::Relaxed);
        Ok((choice.member, guard))
    }

    /// Whether a call of `method` with `params` may be sent twice.
    fn hedgeable(&self, method: &str, params: &serde_json::Value) -> bool {
        crate::retry::is_idempotent_method(method)
            || crate::retry::carries_idempotency_key(method, params, self.peer_honours_idempotency)
    }

    #[cfg(test)]
    pub fn members(&self) -> &Members {
        &self.members
    }
}

impl std::fmt::Debug for BalancedTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BalancedTransport")
            .field("name", &crate::redact::without_userinfo(&self.name))
            .field("members", &self.members)
            .field("max_ejected_percent", &self.max_ejected_percent)
            .field("hedge_delay", &self.hedge.as_ref().map(|(d, _)| *d))
            .finish_non_exhaustive()
    }
}

impl Transport for BalancedTransport {
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
            let set = self.members.get().await?;
            let hedge = self
                .hedge
                .as_ref()
                .filter(|_| self.hedgeable(method, &params));
            let copy = hedge.map(|_| params.clone());
            let (first_member, guard) = self.pick(&set, None)?;
            let first = origin_calls::unary(
                guard,
                first_member.transport.send_request_with_query(
                    method,
                    params,
                    extra_headers,
                    query,
                ),
            );
            let (Some((delay, ledger)), Some(copy)) = (hedge, copy) else {
                return first.await;
            };
            ledger.deposit();
            tokio::pin!(first);
            tokio::select! {
                result = &mut first => return result,
                () = tokio::time::sleep(*delay) => {}
            }
            if !ledger.withdraw() {
                return first.await;
            }
            let Ok((second_member, guard)) = self.pick(&set, Some(&first_member)) else {
                ledger.refund();
                return first.await;
            };
            trace_debug!(
                method,
                "no answer within the hedge delay; sent a second copy"
            );
            let second = origin_calls::unary(
                guard,
                second_member
                    .transport
                    .send_request_with_query(method, copy, extra_headers, query),
            );
            tokio::pin!(second);
            // The first answer that is not an error wins and the other call
            // is dropped; when both fail, the first call's error is returned.
            tokio::select! {
                result = &mut first => match result {
                    Ok(value) => Ok(value),
                    Err(e) => second.await.map_err(|_| e),
                },
                result = &mut second => match result {
                    Ok(value) => Ok(value),
                    Err(_) => first.await,
                },
            }
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
            let set = self.members.get().await?;
            let (member, guard) = self.pick(&set, None)?;
            origin_calls::stream(
                guard,
                member.transport.send_streaming_request_with_query(
                    method,
                    params,
                    extra_headers,
                    query,
                ),
            )
            .await
        })
    }
}
