// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The pin half of the push sender's DNS-rebinding defence: the address a
//! delivery connects to, and the `Host` it presents there. Moved out of
//! `sender.rs` unchanged, which is over the 500-line limit.

use std::net::{IpAddr, SocketAddr};

use a2a_protocol_types::error::{A2aError, A2aResult};

/// Rewrites a webhook URL so that the host component is replaced with the
/// given literal [`SocketAddr`], preserving scheme, path, and query.
///
/// Used after [`validate_webhook_url_with_dns`](super::sender::validate_webhook_url_with_dns) returns a validated
/// `SocketAddr` so the outgoing request connects to the exact IP that was
/// validated — not whatever the HTTP client's own resolver returns seconds
/// later. This is the pin half of the DNS-rebinding defence; the caller is
/// responsible for setting the `Host` header to the original hostname so
/// HTTP vhost routing still works at the remote end.
pub(super) fn rewrite_uri_with_pinned_addr(url: &str, pinned: SocketAddr) -> A2aResult<hyper::Uri> {
    let uri: hyper::Uri = url
        .parse()
        .map_err(|e| A2aError::invalid_params(format!("invalid webhook URL: {e}")))?;

    let scheme = uri
        .scheme_str()
        .ok_or_else(|| A2aError::invalid_params("webhook URL missing scheme"))?;

    // IPv6 literals must be bracketed in the URI authority.
    let host_str = match pinned.ip() {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    };

    let path_and_query = uri
        .path_and_query()
        .map_or_else(|| "/".to_string(), std::string::ToString::to_string);

    let rewritten = format!(
        "{scheme}://{host_str}:{port}{path_and_query}",
        port = pinned.port()
    );

    rewritten
        .parse()
        .map_err(|e| A2aError::invalid_params(format!("could not rewrite webhook URL: {e}")))
}

/// Extracts the original `Host` header value (`host[:port]`) from a webhook URL.
///
/// Used with [`rewrite_uri_with_pinned_addr`] so the remote server still sees
/// the original hostname for vhost routing even though the connection is
/// dialled directly to the pinned IP.
pub(super) fn host_header_from_url(url: &str) -> A2aResult<String> {
    let uri: hyper::Uri = url
        .parse()
        .map_err(|e| A2aError::invalid_params(format!("invalid webhook URL: {e}")))?;
    let host = uri
        .host()
        .ok_or_else(|| A2aError::invalid_params("webhook URL missing host"))?;
    Ok(uri
        .port_u16()
        .map_or_else(|| host.to_string(), |port| format!("{host}:{port}")))
}

/// Decides whether to pin the pre-validated IP for this delivery.
///
/// `http://` requests pin (the caller rewrites the URI to the literal IP and
/// restores the original `Host` header) to close the DNS-rebinding TOCTOU
/// window. `https://` requests must **not** pin — the connection has to present
/// the original hostname for SNI and certificate verification, and TLS
/// validation itself defeats a rebind to a private address (no valid cert for
/// the hostname → handshake fails). A `None` address (IP literal, or SSRF
/// validation skipped) is never pinned.
pub(super) const fn pin_target(
    is_https: bool,
    pinned_addr: Option<SocketAddr>,
) -> Option<SocketAddr> {
    match pinned_addr {
        Some(addr) if !is_https => Some(addr),
        _ => None,
    }
}
