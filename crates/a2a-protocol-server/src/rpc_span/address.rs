// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The addresses on a call's `SERVER` span (`docs/rpc/rpc-spans.md`,
//! "RPC server span", `open-telemetry/semantic-conventions@55c98cf`):
//!
//! * `network.peer.address` and `network.peer.port` (Recommended): the
//!   connection the call arrived on, from the headers map's `:peer-addr`,
//!   which each binding sets when it knows the address
//!   ([`PEER_ADDR_KEY`](crate::call_context::PEER_ADDR_KEY)).
//! * `server.address` and `server.port` (Recommended, and Conditionally
//!   Required with it): "the address the client used to reach this server,
//!   as reported by the transport". Instrumentations "SHOULD NOT use actual
//!   network-level connection information" for them, so they come from the
//!   request's `Host`, and are left unset when there is none, as on gRPC,
//!   whose metadata does not carry `:authority`. The port is recorded only
//!   when `Host` names one.
//!
//! `Host` is the caller's to choose, so it is recorded only when it parses
//! as an authority with a host of at most 253 bytes, the longest DNS name.
//! Neither address goes on `rpc.server.call.duration`: a caller-chosen
//! value on a metric is unbounded cardinality.

use std::collections::HashMap;

/// The longest DNS name, in bytes.
const MAX_HOST: usize = 253;

/// Records the span's server and peer addresses from `headers`.
pub(super) fn record(span: &tracing::Span, headers: &HashMap<String, String>) {
    if let Some(peer) = headers
        .get(crate::call_context::PEER_ADDR_KEY)
        .and_then(|p| p.parse::<std::net::SocketAddr>().ok())
    {
        span.record("network.peer.address", peer.ip().to_string().as_str());
        span.record("network.peer.port", i64::from(peer.port()));
    }
    if let Some((host, port)) = headers.get("host").and_then(|h| server(h)) {
        span.record("server.address", host.as_str());
        if let Some(port) = port {
            span.record("server.port", i64::from(port));
        }
    }
}

/// `Host` as `server.address` (an IPv6 literal without its brackets) and
/// the port it names, if any; `None` when it is not a usable authority.
pub(super) fn server(host: &str) -> Option<(String, Option<u16>)> {
    let authority: hyper::http::uri::Authority = host.parse().ok()?;
    if authority.as_str().contains('@') {
        return None;
    }
    let name = authority
        .host()
        .trim_start_matches('[')
        .trim_end_matches(']');
    (!name.is_empty() && name.len() <= MAX_HOST).then(|| (name.to_owned(), authority.port_u16()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc_span::tests::{Fields, Seen, handler};
    use crate::rpc_span::{RpcSystem, ServerSpan};

    fn fields_for(headers: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
        use tracing_subscriber::layer::SubscriberExt as _;
        let fields = Fields::default();
        let headers: HashMap<String, String> = headers
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        let handler = handler(&Seen::default());
        tracing::subscriber::with_default(
            tracing_subscriber::registry().with(fields.clone()),
            || {
                drop(ServerSpan::open(
                    &handler,
                    RpcSystem::HttpJson,
                    "GetTask",
                    Some(&headers),
                ));
            },
        );
        let recorded = fields.0.lock().unwrap();
        recorded.clone()
    }

    fn get<'a>(f: &'a std::collections::BTreeMap<String, String>, k: &str) -> Option<&'a str> {
        f.get(k).map(String::as_str)
    }

    #[test]
    fn the_span_carries_the_host_the_client_used_and_the_connections_peer() {
        let f = fields_for(&[
            ("host", "agent.example.com:8443"),
            (":peer-addr", "[2001:db8::7]:51000"),
        ]);
        assert_eq!(get(&f, "server.address"), Some("agent.example.com"));
        assert_eq!(get(&f, "server.port"), Some("8443"));
        assert_eq!(get(&f, "network.peer.address"), Some("2001:db8::7"));
        assert_eq!(get(&f, "network.peer.port"), Some("51000"));
    }

    #[test]
    fn a_host_without_a_port_records_no_port_and_an_ipv6_host_loses_its_brackets() {
        let f = fields_for(&[("host", "[::1]")]);
        assert_eq!(get(&f, "server.address"), Some("::1"));
        assert_eq!(get(&f, "server.port"), None);
        let f = fields_for(&[("host", "agent.example.com")]);
        assert_eq!(get(&f, "server.address"), Some("agent.example.com"));
        assert_eq!(get(&f, "server.port"), None);
    }

    /// Nothing is recorded that the transport did not report, and a `Host`
    /// that is not an authority, or names a host longer than any DNS name,
    /// is left out rather than copied onto the span.
    #[test]
    fn what_is_absent_or_unusable_is_not_recorded() {
        let f = fields_for(&[]);
        for key in [
            "server.address",
            "server.port",
            "network.peer.address",
            "network.peer.port",
        ] {
            assert_eq!(get(&f, key), None, "{key}");
        }
        let long = format!("{}.example", "a".repeat(250));
        for host in ["", "user@agent.example.com", "a b", long.as_str()] {
            let f = fields_for(&[("host", host), (":peer-addr", "not an address")]);
            assert_eq!(get(&f, "server.address"), None, "{host:?}");
            assert_eq!(get(&f, "network.peer.address"), None);
        }
    }
}
