// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The `CLIENT` span of an agent-card fetch.
//!
//! A card fetch is a plain HTTP `GET`, not an A2A method, so its span is the
//! HTTP conventions' client span (`docs/http/http-spans.md`): kind `CLIENT`,
//! named `GET` (a card URL has no template the conventions would let it be
//! named by), with `http.request.method`, `url.full`, `server.address` and
//! `server.port`, `http.response.status_code` when a response came back
//! (`304` for a revalidated card), and `error.type` with `Error` status when
//! the fetch failed — the status for an unexpected one, otherwise the
//! failure's class as a call's span records it (`invalid_response` for a
//! card that breaks a2a.proto's field rules).
//!
//! `url.full` keeps the scheme, host, port and path, and drops userinfo and
//! the query, where a card URL behind a gateway may carry a key. No
//! `traceparent` is sent: the fetch has no client configuration to turn
//! propagation off with.

/// Opens the span for a `GET` of `url`.
pub fn open(url: &str) -> tracing::Span {
    let uri = url.parse::<hyper::Uri>().ok();
    let address = uri
        .as_ref()
        .and_then(hyper::Uri::host)
        .map(|h| h.trim_matches(['[', ']']).to_owned());
    let port = uri.as_ref().and_then(port);
    let full = uri.as_ref().map(scrubbed);
    tracing::info_span!(
        target: "a2a_protocol_client::discovery",
        "a2a.card.fetch",
        otel.name = "GET",
        otel.kind = "client",
        otel.status_code = tracing::field::Empty,
        http.request.method = "GET",
        url.full = full.as_deref(),
        server.address = address.as_deref(),
        server.port = port,
        http.response.status_code = tracing::field::Empty,
        error.type = tracing::field::Empty,
    )
}

/// Records how the fetch ended: nothing more on success, `error.type` and
/// `Error` status on failure.
pub fn ended(span: &tracing::Span, error: Option<&crate::error::ClientError>) {
    if let Some(error) = error {
        span.record("error.type", crate::call_span::error_type(error).as_str());
        span.record("otel.status_code", "ERROR");
    }
}

/// `server.port`: the URL's port, or its scheme's default.
fn port(uri: &hyper::Uri) -> Option<u16> {
    uri.port_u16().or_else(|| match uri.scheme_str() {
        Some("https") => Some(443),
        Some("http") => Some(80),
        _ => None,
    })
}

/// `uri` as scheme, authority without userinfo, and path.
fn scrubbed(uri: &hyper::Uri) -> String {
    let scheme = uri.scheme_str().unwrap_or("http");
    let host = uri.host().unwrap_or_default();
    let port = uri.port().map(|p| format!(":{p}")).unwrap_or_default();
    format!("{scheme}://{host}{port}{}", uri.path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_port_is_the_url_s_or_its_scheme_s_default() {
        let port_of = |url: &str| port(&url.parse().unwrap());
        assert_eq!(port_of("https://agent.example/card"), Some(443));
        assert_eq!(port_of("http://agent.example/card"), Some(80));
        assert_eq!(port_of("https://agent.example:8443/card"), Some(8443));
        assert_eq!(port_of("/relative/card"), None);
    }

    #[test]
    fn the_url_keeps_its_origin_and_path_and_drops_userinfo_and_query() {
        let uri: hyper::Uri = "https://user:pw@agent.example:8443/cards/a.json?key=SECRET"
            .parse()
            .unwrap();
        assert_eq!(scrubbed(&uri), "https://agent.example:8443/cards/a.json");
        let plain: hyper::Uri = "http://127.0.0.1/.well-known/agent-card.json"
            .parse()
            .unwrap();
        assert_eq!(
            scrubbed(&plain),
            "http://127.0.0.1/.well-known/agent-card.json"
        );
    }
}
