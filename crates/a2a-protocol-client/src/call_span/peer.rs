// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a client knows about the agent it calls: its system, address and
//! identity, for the call's span and `rpc.client.call.duration`.

use std::sync::Arc;

/// What a client knows about the agent it calls, for its spans.
#[derive(Clone, Debug, Default)]
#[cfg_attr(not(feature = "tracing"), allow(dead_code))]
pub struct Peer {
    /// `rpc.system.name`, when the client chose the transport.
    pub(crate) system: Option<&'static str>,
    pub(crate) address: Option<String>,
    pub(crate) port: Option<u16>,
    /// Name, description and version, from the card.
    pub(crate) agent: Option<Arc<(String, String, String)>>,
    /// Whether the transport carries per-request headers, so a
    /// `traceparent` set for one call reaches the agent with that call.
    #[cfg_attr(not(feature = "otel"), allow(dead_code))]
    pub(crate) carries_headers: bool,
}

impl Peer {
    /// The peer of a client given a transport of its own: the system and the
    /// URL the transport names, and the agent from the card, if any. No
    /// `traceparent` is set, since such a transport may carry no headers.
    pub(crate) fn custom(
        system: Option<&'static str>,
        url: Option<&str>,
        agent: Option<Arc<(String, String, String)>>,
    ) -> Self {
        let (address, port) = url.map(target).unwrap_or_default();
        Self {
            system,
            address,
            port,
            agent,
            carries_headers: false,
        }
    }

    /// The peer of a client this crate built a transport for.
    pub(crate) fn new(
        binding: &str,
        endpoint: &str,
        agent: Option<Arc<(String, String, String)>>,
    ) -> Self {
        let system = match binding.to_ascii_uppercase().as_str() {
            "JSONRPC" => Some("jsonrpc"),
            "HTTP+JSON" | "REST" => Some("a2a_http_json"),
            "GRPC" => Some("grpc"),
            _ => None,
        };
        let (address, port) = target(endpoint);
        Self {
            system,
            address,
            port,
            agent,
            carries_headers: system.is_some(),
        }
    }
}

/// `server.address` and `server.port` for a URL: the host without an IPv6
/// literal's brackets, and the port, or the scheme's default.
fn target(url: &str) -> (Option<String>, Option<u16>) {
    let uri = url.parse::<hyper::Uri>().ok();
    let address = uri
        .as_ref()
        .and_then(|u| u.host())
        .map(|h| h.trim_matches(['[', ']']).to_owned());
    let port = uri.as_ref().and_then(|u| {
        u.port_u16().or_else(|| match u.scheme_str() {
            Some("https" | "wss") => Some(443),
            Some("http" | "ws") => Some(80),
            _ => None,
        })
    });
    (address, port)
}
