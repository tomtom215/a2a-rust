// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Entry points for the fuzz targets in `fuzz/`, compiled only for them.
//!
//! Three parsers read peer-controlled bytes and are private, so a fuzz
//! target — an external crate — cannot reach them: a bearer token's JWT,
//! the REST binding's `ListTasks` query string, and the `x-forwarded-for`
//! header the rate limiter trusts behind a proxy. This module forwards to
//! each unchanged. It exists under `cfg(fuzzing)`, which `cargo fuzz` sets,
//! and under `cfg(test)`, so the forwarding itself is tested; it is in no
//! build an adopter makes and in no published API. Audit escape class 8.

use std::collections::HashMap;

/// The JWT validator, fixed to accept HS256 under one secret and ES256 under
/// the RFC 7515 Appendix A.3 key, so both signature paths are reachable.
/// Returns whether the token was accepted.
#[cfg(feature = "auth-jwt")]
#[must_use]
pub fn jwt_validate(token: &str) -> bool {
    use crate::auth::jwt::{Jwks, JwtValidator};
    let Ok(jwks) = Jwks::from_json(br#"{"keys":[]}"#).and_then(|j| {
        j.with_ec_p256(
            "a3",
            "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU",
            "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0",
        )
    }) else {
        return false;
    };
    JwtValidator::new()
        .with_hs256_secret(b"fuzz".to_vec())
        .validate(token, &jwks)
        .is_ok()
}

/// The REST binding's routing of a request line: the tenant prefix split off
/// the path, then the `ListTasks` query parameters, as `dispatch_rest` reads
/// them.
#[must_use]
pub fn rest_route(
    path: &str,
    query: &str,
) -> (
    Option<String>,
    String,
    a2a_protocol_types::params::ListTasksParams,
) {
    let (tenant, rest) = crate::dispatch::rest::query::strip_tenant_prefix(path);
    let params = crate::dispatch::rest::query::parse_list_tasks_query(query, tenant);
    (tenant.map(str::to_owned), rest.to_owned(), params)
}

/// Whether a push-notification webhook URL a client registers passes the
/// SSRF check (`validate_webhook_url`, without DNS).
#[must_use]
pub fn webhook_url_allowed(url: &str) -> bool {
    crate::push::sender::validate_webhook_url(url).is_ok()
}

/// The in-memory store's decoding of a client-supplied `ListTasks` page token.
#[must_use]
pub fn page_token(token: &str) -> Option<(i64, u64)> {
    crate::store::task_store::in_memory::decode_order_key(token)
}

/// The `A2A-Extensions` request header, split into extension URIs.
#[must_use]
pub fn extensions_header(value: &str) -> Vec<String> {
    let headers = HashMap::from([("a2a-extensions".to_owned(), value.to_owned())]);
    crate::handler::helpers::parse_extensions_header(&headers)
}

/// The rate limiter's caller key, from an `x-forwarded-for` value behind
/// `trusted_proxy_hops` proxies.
#[must_use]
pub fn caller_key_from_forwarded_for(xff: &str, trusted_proxy_hops: usize) -> String {
    let headers = HashMap::from([("x-forwarded-for".to_owned(), xff.to_owned())]);
    let ctx = crate::call_context::CallContext::new("fuzz").with_http_headers(headers);
    crate::rate_limit::identity::caller_key(&ctx, trusted_proxy_hops)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwards_the_request_line() {
        let (tenant, rest, p) = rest_route(
            "/acme/tasks",
            "contextId=c%201&pageSize=7&includeArtifacts=true",
        );
        assert_eq!(tenant.as_deref(), Some("acme"));
        assert_eq!(rest, "/tasks");
        assert_eq!(p.tenant.as_deref(), Some("acme"));
        assert_eq!(p.context_id.as_deref(), Some("c 1"));
        assert_eq!(p.page_size, Some(7));
        assert_eq!(p.include_artifacts, Some(true));
    }

    #[test]
    fn forwards_the_webhook_url() {
        assert!(webhook_url_allowed("https://example.com/hook"));
        assert!(!webhook_url_allowed("http://2852039166/"));
    }

    #[test]
    fn forwards_the_page_token() {
        assert_eq!(page_token("12:3"), Some((12, 3)));
        assert_eq!(page_token("12"), None);
    }

    #[test]
    fn forwards_the_extensions_header() {
        assert_eq!(extensions_header("urn:a, urn:b"), ["urn:a", "urn:b"]);
    }

    #[test]
    fn forwards_the_forwarded_for_header() {
        assert_eq!(
            caller_key_from_forwarded_for("198.51.100.1, ::ffff:203.0.113.7", 1),
            "203.0.113.7"
        );
        assert_eq!(
            caller_key_from_forwarded_for("198.51.100.1", 2),
            "anonymous"
        );
    }

    #[cfg(feature = "auth-jwt")]
    #[test]
    fn forwards_the_token() {
        // HS256 over a header and claims (expiring in 2100) with the secret `fuzz`,
        // computed with ring below rather than pasted, so it cannot rot.
        use base64::Engine;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256"}"#),
            URL_SAFE_NO_PAD.encode(br#"{"sub":"s","exp":4102444800}"#)
        );
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, b"fuzz");
        let sig = URL_SAFE_NO_PAD.encode(ring::hmac::sign(&key, input.as_bytes()).as_ref());
        assert!(jwt_validate(&format!("{input}.{sig}")));
        assert!(!jwt_validate(&format!("{input}.{sig}x")));
        assert!(!jwt_validate("a.b"));
    }
}
