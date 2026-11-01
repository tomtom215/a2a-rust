// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The TCP settings every outbound HTTP client in this crate dials with.
//!
//! Push delivery, JWKS and OIDC discovery and the model client had these only
//! in builds with `tls-rustls`, and the build without it dialled with the
//! operating system's defaults: no bound on a connect to an address that
//! drops packets (the SYN retry schedule, minutes) and no keepalive on a
//! pooled connection (audit B-30). One function sets them for all.

use std::time::Duration;

use hyper_util::client::legacy::connect::HttpConnector;

/// How long a TCP connect may take. Without a bound a black-holed address
/// holds the caller for the operating system's SYN retry schedule, minutes.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// TCP keepalive on a pooled connection, so a path that drops idle flows (a
/// NAT, a load balancer) is noticed instead of reused.
pub const TCP_KEEPALIVE: Duration = Duration::from_secs(30);

/// How long an idle pooled connection is kept. Below the 60 s idle timeout of
/// common load balancers (AWS ALB, nginx's keepalive) and this crate's
/// server's 75 s: a connection the peer has already closed is not reused
/// (audit B-07, B-30, N-B-3). hyper-util's default is 90 s.
pub const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(50);

/// Sets `TCP_NODELAY`, the connect bound and the keepalive on `tcp`.
pub fn tune<R>(tcp: &mut HttpConnector<R>) {
    tcp.set_nodelay(true);
    tcp.set_connect_timeout(Some(CONNECT_TIMEOUT));
    tcp.set_keepalive(Some(TCP_KEEPALIVE));
}

#[cfg(test)]
mod tests;
