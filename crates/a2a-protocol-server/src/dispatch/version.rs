// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Protocol version negotiation (spec §3.6): which `A2A-Version` a request
//! asks for, and whether this server serves it. Every built-in binding checks
//! through [`validate_version_header`]; a binding outside this crate checks
//! through [`validate_version_metadata`].

/// The service parameter naming the A2A protocol version, spelled the way a
/// non-HTTP binding carries it.
///
/// A2A §3.6.2 defines the parameter and §10.2 says each binding transmits it in
/// whatever its own metadata mechanism is: an `A2A-Version` HTTP header for
/// JSON-RPC and REST, a gRPC metadata entry, SLIMRPC session metadata. HTTP
/// header names are case-insensitive and gRPC requires lowercase, so this is
/// the lowercase spelling and [`validate_version_metadata`] matches keys
/// without regard to case.
pub const A2A_VERSION_METADATA_KEY: &str = "a2a-version";

/// Validates the A2A version carried in a binding's request metadata.
///
/// The counterpart of this crate's private header validator, for bindings that
/// carry service parameters in a string map rather than in HTTP headers, and
/// the supported way for a binding **outside this crate** to enforce §3.6.2 —
/// which the built-in bindings do through private helpers this makes public.
/// Without it an out-of-tree binding can send a version but cannot check one,
/// and would have to reimplement the comparison and hope it stays in step.
///
/// Key lookup is case-insensitive. Only this server's `Major.Minor`
/// ([`A2A_VERSION`](a2a_protocol_types::A2A_VERSION), `1.0`) is accepted, as
/// §3.6.2 requires; a patch segment is ignored, per §3.6. The map is generic over its hasher so a binding that
/// keeps metadata in something other than the default `RandomState` — most
/// transport crates do — can pass it without rebuilding the map.
///
/// `require` decides what an absent or empty value means, and the two answers
/// are both defensible, which is why it is the caller's to make. §3.6.2 says a
/// missing value MUST be read as protocol 0.3 — a version this server does not
/// implement — so `true` rejects it, matching the reference Python SDK and this
/// crate's own HTTP default. `false` accepts it, which is what the gRPC binding
/// does for clients predating the parameter.
///
/// # Errors
///
/// [`A2aError::version_not_supported`] when the version is one this server does
/// not implement, or is absent while `require` is set.
///
/// # Example
///
/// ```rust
/// use a2a_protocol_server::dispatch::validate_version_metadata;
/// use std::collections::HashMap;
///
/// let mut metadata = HashMap::new();
/// metadata.insert("A2A-Version".to_string(), "1.0".to_string());
/// assert!(validate_version_metadata(&metadata, true).is_ok());
///
/// // Absent, and the caller requires it: rejected as 0.3 per §3.6.2.
/// assert!(validate_version_metadata(&HashMap::new(), true).is_err());
/// ```
///
/// [`A2aError::version_not_supported`]: a2a_protocol_types::error::A2aError::version_not_supported
pub fn validate_version_metadata<S: std::hash::BuildHasher>(
    metadata: &std::collections::HashMap<String, String, S>,
    require: bool,
) -> Result<(), a2a_protocol_types::error::A2aError> {
    let value = metadata
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(A2A_VERSION_METADATA_KEY))
        .map(|(_, v)| v.as_str());
    validate_version_header(value, require)
}

/// The `A2A-Version` a request asks for: its header, or else the
/// `A2A-Version` request parameter.
///
/// Spec §3.6.1: "Clients MAY provide the `A2A-Version` as a request parameter
/// instead of a header", with `GET /tasks/task-123?A2A-Version=1.0` as the
/// example. Until 2026-10-10 only the header was read, so a client doing
/// exactly what the example shows was taken to be speaking 0.3 and refused.
/// A non-empty header wins over the parameter; an empty or absent one falls
/// back to it.
pub fn requested_version(headers: &hyper::HeaderMap, uri: &hyper::Uri) -> Option<String> {
    let header = headers
        .get(a2a_protocol_types::A2A_VERSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.trim().is_empty());
    if let Some(v) = header {
        return Some(v.to_owned());
    }
    uri.query().and_then(|q| {
        super::rest::query::parse_query_param(q, a2a_protocol_types::A2A_VERSION_HEADER)
    })
}

/// Validates an `A2A-Version` header value per spec §3.6.2.
///
/// `value` is the raw header value (`None` when the header is absent).
/// Absent or empty MUST be interpreted as protocol 0.3; when `require` is
/// set (the strict default) that yields the same `VersionNotSupported`
/// rejection the reference Python SDK produces. Otherwise the value must
/// name this server's `Major.Minor` ([`is_served_version`]): §3.6.2 says a
/// request is processed with the semantics of the version it asks for, and
/// this server has only 1.0's. Until 0.15 any `1.x` was accepted and served
/// as 1.0, as the Python SDK still does.
pub fn validate_version_header(
    value: Option<&str>,
    require: bool,
) -> Result<(), a2a_protocol_types::error::A2aError> {
    let v = value.unwrap_or("").trim();
    if v.is_empty() {
        if require {
            return Err(a2a_protocol_types::error::A2aError::version_not_supported(
                "A2A version '0.3' is not supported by this server; expected '1.0' (send the A2A-Version header)",
            ));
        }
        return Ok(());
    }
    if is_served_version(v) {
        return Ok(());
    }
    Err(a2a_protocol_types::error::A2aError::version_not_supported(
        format!(
            "unsupported A2A version: {v}; this server supports {}",
            a2a_protocol_types::A2A_VERSION
        ),
    ))
}

/// Whether `version` names the protocol version this server implements: the
/// `Major.Minor` of [`A2A_VERSION`](a2a_protocol_types::A2A_VERSION), with
/// anything after the minor ignored (§3.6: patch numbers "MUST not be
/// considered"). Both components must be decimal digits only, so `"1"`,
/// `"v1.0"`, `"+1.0"` and `"1.0-rc"` name no `Major.Minor` and are refused.
fn is_served_version(version: &str) -> bool {
    fn major_minor(version: &str) -> Option<(u32, u32)> {
        let mut parts = version.split('.');
        let mut number = || {
            parts
                .next()
                .filter(|p| p.bytes().all(|b| b.is_ascii_digit()))?
                .parse()
                .ok()
        };
        Some((number()?, number()?))
    }
    major_minor(version).is_some_and(|v| Some(v) == major_minor(a2a_protocol_types::A2A_VERSION))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_metadata_matches_key_case_insensitively() {
        // Bindings spell this differently — HTTP headers arrive in whatever
        // case the peer sent, gRPC lowercases, SLIMRPC passes through what the
        // caller set. All four spellings are the same parameter.
        for key in ["a2a-version", "A2A-Version", "A2A-VERSION", "a2a-Version"] {
            let mut md = std::collections::HashMap::new();
            md.insert(key.to_string(), "1.0".to_string());
            assert!(
                validate_version_metadata(&md, true).is_ok(),
                "key spelling {key} should be recognised"
            );
        }
    }

    #[test]
    fn version_metadata_rejects_unsupported_version() {
        let mut md = std::collections::HashMap::new();
        md.insert("a2a-version".to_string(), "0.3".to_string());
        let err = validate_version_metadata(&md, true).expect_err("0.3 is not supported");
        assert!(
            err.message.contains("0.3"),
            "the error should name the version it rejected, got: {}",
            err.message
        );
    }

    #[test]
    fn version_metadata_accepts_this_major_minor_with_any_patch() {
        for v in ["1.0", "1.0.2", "1.00", " 1.0 "] {
            let mut md = std::collections::HashMap::new();
            md.insert("a2a-version".to_string(), v.to_string());
            assert!(
                validate_version_metadata(&md, true).is_ok(),
                "{v} names 1.0 and should be accepted"
            );
        }
    }

    /// §3.6.2: a request is processed with the semantics of the requested
    /// `Major.Minor`, and this server has only 1.0's, so another minor is
    /// refused with an error naming what it does support. Not-a-version
    /// spellings are refused too.
    #[test]
    fn version_metadata_refuses_another_minor_or_no_minor() {
        for v in [
            "1.1", "1.4", "1", "1.", "v1.0", "+1.0", "1.0-rc", "1.+0", "0.0", "2.0",
        ] {
            let mut md = std::collections::HashMap::new();
            md.insert("a2a-version".to_string(), v.to_string());
            let err = validate_version_metadata(&md, true)
                .expect_err(&format!("{v:?} is not 1.0 and must be refused"));
            assert_eq!(
                err.code,
                a2a_protocol_types::error::ErrorCode::VersionNotSupported
            );
            assert!(err.message.contains("supports 1.0"), "{}", err.message);
        }
    }

    #[test]
    fn version_metadata_absent_follows_the_require_flag() {
        let empty = std::collections::HashMap::new();
        assert!(
            validate_version_metadata(&empty, false).is_ok(),
            "require=false is the gRPC posture: absent means a legacy client, accept it"
        );
        assert!(
            validate_version_metadata(&empty, true).is_err(),
            "require=true is the HTTP posture: absent means 0.3 per 3.6.2, reject it"
        );
    }

    #[test]
    fn version_metadata_treats_empty_value_as_absent() {
        // A binding that sets the key but leaves it blank has told us nothing,
        // and 3.6.2 reads "no value" as 0.3 regardless of how it got that way.
        let mut md = std::collections::HashMap::new();
        md.insert("a2a-version".to_string(), "   ".to_string());
        assert!(validate_version_metadata(&md, true).is_err());
        assert!(validate_version_metadata(&md, false).is_ok());
    }

    #[test]
    fn version_metadata_ignores_unrelated_keys() {
        let mut md = std::collections::HashMap::new();
        md.insert("authorization".to_string(), "Bearer x".to_string());
        md.insert("x-tenant-id".to_string(), "acme".to_string());
        assert!(
            validate_version_metadata(&md, false).is_ok(),
            "no version key present, and require=false accepts that"
        );
    }
}
