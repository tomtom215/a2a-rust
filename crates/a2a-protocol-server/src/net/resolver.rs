// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Name resolution for the shared HTTP client, with a way to say "this host
//! is these addresses" for as long as a delivery runs.
//!
//! # Why
//!
//! Push delivery checks a webhook's host against the private ranges by
//! resolving it, then connects. With the client resolving a second time, a
//! name that answers a public address to the check and an internal one to the
//! connect (DNS rebinding) got a TCP connection and a TLS `ClientHello` to the
//! internal address (audit D-13, SEC-13). `http://` was closed first, by
//! rewriting the URL to the validated address. An `https://` URL cannot be
//! rewritten: the certificate is checked against the host name in the URL, so
//! the connection has to ask for that name. Since 0.16.0 an `http://` one is
//! not rewritten either: the proxy rules, `NO_PROXY` included, were then
//! matched against the address rather than the name they were written for
//! (audit NEW-b-4). For both, the name stays and the answer is fixed: while a [`PinGuard`] is held, the host resolves to the
//! addresses that were checked and to nothing else, and the server's
//! certificate is verified against the name as before.
//!
//! A host with no pin goes to the system resolver, so JWKS fetches, model
//! calls and OTLP export are unchanged.
//!
//! The pin does not reach a delivery that goes through a proxy: the proxy
//! resolves the name, and which names it may reach is its own policy (see
//! `docs/audits/2026-11-01-triage/DECISIONS.md`, F16).

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::connect::dns::{GaiResolver, Name};
use tower_service::Service;

/// The TCP connector every outbound client in this crate dials with.
pub type Tcp = HttpConnector<PinResolver>;

/// A TCP connector that resolves through the system, with no pins.
#[cfg(any(feature = "tls-rustls", feature = "otlp", test))]
pub fn system_tcp() -> Tcp {
    HttpConnector::new_with_resolver(PinResolver::default())
}

/// One pin: an id (so the guard that made it removes only it) and the
/// addresses the host resolves to while it is held.
type Pins = Mutex<HashMap<String, Vec<(u64, Vec<SocketAddr>)>>>;

/// The resolver behind [`Tcp`]. Clones share their pins.
#[derive(Clone)]
pub struct PinResolver {
    pins: Arc<Pins>,
    // Ids for pins; only the push client makes any, OTLP export none.
    next: Arc<std::sync::atomic::AtomicU64>,
    system: GaiResolver,
}

impl Default for PinResolver {
    fn default() -> Self {
        Self {
            pins: Arc::default(),
            next: Arc::default(),
            system: GaiResolver::new(),
        }
    }
}

impl std::fmt::Debug for PinResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PinResolver").finish_non_exhaustive()
    }
}

/// The form of a host name that pins are kept under and looked up by: lower
/// case, without a trailing dot (`Example.COM.` and `example.com` are one
/// name to DNS).
fn key(host: &str) -> String {
    host.strip_suffix('.').unwrap_or(host).to_ascii_lowercase()
}

impl PinResolver {
    /// Resolves `host` to `addrs` until the returned guard is dropped.
    ///
    /// Pins stack: while two deliveries to one host each hold one, the host
    /// resolves to both sets, so neither delivery's addresses vanish when the
    /// other ends.
    pub fn pin(&self, host: &str, addrs: Vec<SocketAddr>) -> PinGuard {
        let id = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let key = key(host);
        self.pins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(key.clone())
            .or_default()
            .push((id, addrs));
        PinGuard {
            pins: Arc::clone(&self.pins),
            key,
            id,
        }
    }

    /// Whether `host` has a pin.
    #[cfg(test)]
    pub(crate) fn is_pinned(&self, host: &str) -> bool {
        self.pinned(host).is_some()
    }

    /// The addresses `host` is pinned to, in the order they were pinned and
    /// without repeats, or `None` when it is not pinned.
    fn pinned(&self, host: &str) -> Option<Vec<SocketAddr>> {
        let held: Vec<SocketAddr> = {
            let pins = self.pins.lock().unwrap_or_else(PoisonError::into_inner);
            pins.get(&key(host))?
                .iter()
                .flat_map(|(_, addrs)| addrs.iter().copied())
                .collect()
        };
        let mut all: Vec<SocketAddr> = Vec::new();
        for addr in held {
            if !all.contains(&addr) {
                all.push(addr);
            }
        }
        Some(all)
    }
}

/// A host's pin; the host resolves normally again when it is dropped.
#[must_use = "the pin ends when the guard is dropped"]
pub struct PinGuard {
    pins: Arc<Pins>,
    key: String,
    id: u64,
}

impl Drop for PinGuard {
    fn drop(&mut self) {
        let mut pins = self.pins.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(held) = pins.get_mut(&self.key) {
            held.retain(|(id, _)| *id != self.id);
            if held.is_empty() {
                pins.remove(&self.key);
            }
        }
    }
}

impl Service<Name> for PinResolver {
    type Response = std::vec::IntoIter<SocketAddr>;
    type Error = std::io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    // Equivalent mutant: cargo-mutants replaces the body with
    // `Poll::from(Ok(()))`, which is `Poll::Ready(Ok(()))`, the value written
    // here. No test can tell the two apart (ADR 0006).
    #[mutants::skip]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, name: Name) -> Self::Future {
        if let Some(addrs) = self.pinned(name.as_str()) {
            return Box::pin(async move { Ok(addrs.into_iter()) });
        }
        let mut system = self.system.clone();
        Box::pin(async move {
            let addrs = system.call(name).await?;
            Ok(addrs.collect::<Vec<_>>().into_iter())
        })
    }
}

#[cfg(test)]
mod tests;
