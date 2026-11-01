// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The host-name lookup behind the SSRF pre-flight, with a bound on how many
//! may run at once.
//!
//! `getaddrinfo` cannot be cancelled. A lookup that outlives the caller's
//! timeout keeps its blocking-pool thread until the resolver answers, and the
//! timeout only stopped the caller waiting. A caller who can register
//! webhooks under a domain whose name server answers slowly could therefore
//! fill the pool with threads, one per registration, and every other user of
//! `spawn_blocking` (file I/O, other lookups) waited behind them (audit
//! D-22). Here the permit is held by the thread itself and returned when the
//! lookup returns, not when the caller stops waiting, so at most
//! [`MAX_CONCURRENT_LOOKUPS`] threads can be held, and a lookup past that
//! waits for a permit inside the caller's own timeout.

use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::{Arc, LazyLock};

use tokio::sync::Semaphore;

/// The most pre-flight lookups that may hold a blocking-pool thread at once,
/// across the process. The pool's default is 512 threads.
const MAX_CONCURRENT_LOOKUPS: usize = 32;

static LOOKUPS: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(MAX_CONCURRENT_LOOKUPS)));

/// Resolves `host:port` through the system resolver, within the bound.
pub(super) async fn lookup_host(addr: String) -> io::Result<Vec<SocketAddr>> {
    run_bounded(&LOOKUPS, move || {
        addr.to_socket_addrs().map(Iterator::collect)
    })
    .await
}

/// Runs `lookup` on the blocking pool once `limit` has a permit, and keeps the
/// permit until `lookup` returns, whether or not anyone is still waiting.
pub(super) async fn run_bounded<F>(limit: &Arc<Semaphore>, lookup: F) -> io::Result<Vec<SocketAddr>>
where
    F: FnOnce() -> io::Result<Vec<SocketAddr>> + Send + 'static,
{
    let permit = Arc::clone(limit)
        .acquire_owned()
        .await
        .map_err(io::Error::other)?;
    tokio::task::spawn_blocking(move || {
        let _held = permit;
        lookup()
    })
    .await
    .map_err(io::Error::other)?
}

#[cfg(test)]
mod tests;
