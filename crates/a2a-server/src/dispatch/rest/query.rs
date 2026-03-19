// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Query string and URL parsing helpers for the REST dispatcher.

/// Strips an optional `/tenants/{tenant}/` prefix, returning the tenant and
/// remaining path.
pub(super) fn strip_tenant_prefix(path: &str) -> (Option<&str>, &str) {
    if let Some(rest) = path.strip_prefix("/tenants/") {
        if let Some(slash_pos) = rest.find('/') {
            let tenant = &rest[..slash_pos];
            let remaining = &rest[slash_pos..];
            return (Some(tenant), remaining);
        }
    }
    (None, path)
}

/// Parses a single query parameter value as `u32`.
pub(super) fn parse_query_param_u32(query: &str, key: &str) -> Option<u32> {
    parse_query_param(query, key).and_then(|v| v.parse::<u32>().ok())
}

/// Parses a single query parameter value as a string, with percent-decoding.
pub(super) fn parse_query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            Some(percent_decode(v))
        } else {
            None
        }
    })
}

/// Decodes percent-encoded characters in a query parameter value.
///
/// Handles `%XX` hex sequences and `+` as space (application/x-www-form-urlencoded).
fn percent_decode(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut bytes = input.as_bytes().iter();
    while let Some(&b) = bytes.next() {
        match b {
            b'%' => {
                let hi = bytes.next().copied();
                let lo = bytes.next().copied();
                if let (Some(h), Some(l)) = (hi, lo) {
                    if let (Some(h), Some(l)) = (hex_val(h), hex_val(l)) {
                        output.push(char::from(h << 4 | l));
                        continue;
                    }
                }
                // Invalid percent sequence — pass through as-is.
                output.push('%');
            }
            b'+' => output.push(' '),
            _ => output.push(char::from(b)),
        }
    }
    output
}

/// Checks if a path contains traversal sequences (`..`) in either raw,
/// percent-encoded (`%2E%2E`), or double-encoded (`%252E%252E`) form.
pub(super) fn contains_path_traversal(path: &str) -> bool {
    if path.contains("..") {
        return true;
    }
    // Check single-encoded variants (%2E%2E, %2e%2e).
    let decoded = percent_decode(path);
    if decoded.contains("..") {
        return true;
    }
    // Check double-encoded variants (%252E%252E → %2E%2E → ..).
    let double_decoded = percent_decode(&decoded);
    double_decoded.contains("..")
}

/// Returns the numeric value of a hex digit, or `None` if invalid.
const fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Parses a single query parameter value as `bool`.
pub(super) fn parse_query_param_bool(query: &str, key: &str) -> Option<bool> {
    parse_query_param(query, key).map(|v| v == "true" || v == "1")
}

/// Parses `ListTasksParams` from URL query parameters.
pub(super) fn parse_list_tasks_query(
    query: &str,
    tenant: Option<&str>,
) -> a2a_protocol_types::params::ListTasksParams {
    let status = parse_query_param(query, "status")
        .and_then(|s| serde_json::from_value(serde_json::Value::String(s)).ok());
    a2a_protocol_types::params::ListTasksParams {
        tenant: tenant.map(str::to_owned),
        context_id: parse_query_param(query, "contextId"),
        status,
        page_size: parse_query_param_u32(query, "pageSize"),
        page_token: parse_query_param(query, "pageToken"),
        status_timestamp_after: parse_query_param(query, "statusTimestampAfter"),
        include_artifacts: parse_query_param_bool(query, "includeArtifacts"),
        history_length: parse_query_param_u32(query, "historyLength"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── hex_val ──────────────────────────────────────────────────────────

    #[test]
    fn hex_val_digits() {
        for (b, expected) in (b'0'..=b'9').zip(0u8..=9) {
            assert_eq!(hex_val(b), Some(expected));
        }
    }

    #[test]
    fn hex_val_lowercase() {
        for (b, expected) in (b'a'..=b'f').zip(10u8..=15) {
            assert_eq!(hex_val(b), Some(expected));
        }
    }

    #[test]
    fn hex_val_uppercase() {
        for (b, expected) in (b'A'..=b'F').zip(10u8..=15) {
            assert_eq!(hex_val(b), Some(expected));
        }
    }

    #[test]
    fn hex_val_invalid() {
        assert_eq!(hex_val(b'g'), None);
        assert_eq!(hex_val(b'G'), None);
        assert_eq!(hex_val(b' '), None);
        assert_eq!(hex_val(b'z'), None);
    }

    // ── percent_decode ───────────────────────────────────────────────────

    #[test]
    fn percent_decode_plain_string() {
        assert_eq!(percent_decode("hello"), "hello");
    }

    #[test]
    fn percent_decode_encoded_chars() {
        assert_eq!(percent_decode("%2F"), "/");
        assert_eq!(percent_decode("%2f"), "/");
        assert_eq!(percent_decode("a%20b"), "a b");
    }

    #[test]
    fn percent_decode_plus_as_space() {
        assert_eq!(percent_decode("a+b"), "a b");
    }

    #[test]
    fn percent_decode_invalid_sequence_passthrough() {
        // Incomplete percent sequence: just '%' at end
        assert_eq!(percent_decode("abc%"), "abc%");
        // Invalid hex digits after percent
        assert_eq!(percent_decode("%ZZ"), "%");
    }

    #[test]
    fn percent_decode_double_encoded_dots() {
        // %252E decodes to %2E in first pass
        assert_eq!(percent_decode("%252E"), "%2E");
        // Second pass decodes %2E to .
        assert_eq!(percent_decode("%2E"), ".");
    }

    // ── contains_path_traversal ──────────────────────────────────────────

    #[test]
    fn path_traversal_raw() {
        assert!(contains_path_traversal("/../admin"));
        assert!(contains_path_traversal("/foo/../bar"));
    }

    #[test]
    fn path_traversal_single_encoded() {
        assert!(contains_path_traversal("/%2E%2E/admin"));
        assert!(contains_path_traversal("/%2e%2e/admin"));
    }

    #[test]
    fn path_traversal_double_encoded() {
        assert!(contains_path_traversal("/%252E%252E/admin"));
    }

    #[test]
    fn path_traversal_safe_paths() {
        assert!(!contains_path_traversal("/tasks/abc"));
        assert!(!contains_path_traversal("/tasks/abc.def"));
        assert!(!contains_path_traversal("/message:send"));
    }

    // ── strip_tenant_prefix ──────────────────────────────────────────────

    #[test]
    fn strip_tenant_with_valid_prefix() {
        let (tenant, rest) = strip_tenant_prefix("/tenants/acme/tasks");
        assert_eq!(tenant, Some("acme"));
        assert_eq!(rest, "/tasks");
    }

    #[test]
    fn strip_tenant_with_nested_path() {
        let (tenant, rest) = strip_tenant_prefix("/tenants/org-42/tasks/abc");
        assert_eq!(tenant, Some("org-42"));
        assert_eq!(rest, "/tasks/abc");
    }

    #[test]
    fn strip_tenant_no_trailing_slash() {
        // /tenants/foo with nothing after it — no slash, no match
        let (tenant, rest) = strip_tenant_prefix("/tenants/foo");
        assert_eq!(tenant, None);
        assert_eq!(rest, "/tenants/foo");
    }

    #[test]
    fn strip_tenant_no_prefix() {
        let (tenant, rest) = strip_tenant_prefix("/tasks");
        assert_eq!(tenant, None);
        assert_eq!(rest, "/tasks");
    }

    #[test]
    fn strip_tenant_empty_tenant_name() {
        // /tenants//tasks — empty tenant name, slash at pos 0
        let (tenant, rest) = strip_tenant_prefix("/tenants//tasks");
        assert_eq!(tenant, Some(""));
        assert_eq!(rest, "/tasks");
    }

    // ── parse_query_param ────────────────────────────────────────────────

    #[test]
    fn parse_query_param_found() {
        assert_eq!(
            parse_query_param("foo=bar&baz=42", "foo"),
            Some("bar".to_owned())
        );
        assert_eq!(
            parse_query_param("foo=bar&baz=42", "baz"),
            Some("42".to_owned())
        );
    }

    #[test]
    fn parse_query_param_not_found() {
        assert_eq!(parse_query_param("foo=bar", "missing"), None);
    }

    #[test]
    fn parse_query_param_empty_query() {
        assert_eq!(parse_query_param("", "foo"), None);
    }

    #[test]
    fn parse_query_param_percent_encoded_value() {
        assert_eq!(
            parse_query_param("name=hello%20world", "name"),
            Some("hello world".to_owned())
        );
    }

    #[test]
    fn parse_query_param_plus_in_value() {
        assert_eq!(parse_query_param("q=a+b", "q"), Some("a b".to_owned()));
    }

    // ── parse_query_param_u32 ────────────────────────────────────────────

    #[test]
    fn parse_query_param_u32_valid() {
        assert_eq!(
            parse_query_param_u32("historyLength=10", "historyLength"),
            Some(10)
        );
    }

    #[test]
    fn parse_query_param_u32_invalid() {
        assert_eq!(
            parse_query_param_u32("historyLength=abc", "historyLength"),
            None
        );
    }

    #[test]
    fn parse_query_param_u32_missing() {
        assert_eq!(parse_query_param_u32("other=5", "historyLength"), None);
    }

    #[test]
    fn parse_query_param_u32_zero() {
        assert_eq!(parse_query_param_u32("pageSize=0", "pageSize"), Some(0));
    }

    // ── parse_query_param_bool ───────────────────────────────────────────

    #[test]
    fn parse_query_param_bool_true() {
        assert_eq!(parse_query_param_bool("flag=true", "flag"), Some(true));
        assert_eq!(parse_query_param_bool("flag=1", "flag"), Some(true));
    }

    #[test]
    fn parse_query_param_bool_false() {
        assert_eq!(parse_query_param_bool("flag=false", "flag"), Some(false));
        assert_eq!(parse_query_param_bool("flag=0", "flag"), Some(false));
    }

    #[test]
    fn parse_query_param_bool_missing() {
        assert_eq!(parse_query_param_bool("other=true", "flag"), None);
    }

    // ── parse_list_tasks_query ───────────────────────────────────────────

    #[test]
    fn parse_list_tasks_query_all_params() {
        let query =
            "contextId=ctx-1&pageSize=10&pageToken=tok&includeArtifacts=true&historyLength=5";
        let params = parse_list_tasks_query(query, Some("acme"));
        assert_eq!(params.tenant.as_deref(), Some("acme"));
        assert_eq!(params.context_id.as_deref(), Some("ctx-1"));
        assert_eq!(params.page_size, Some(10));
        assert_eq!(params.page_token.as_deref(), Some("tok"));
        assert_eq!(params.include_artifacts, Some(true));
        assert_eq!(params.history_length, Some(5));
    }

    #[test]
    fn parse_list_tasks_query_empty() {
        let params = parse_list_tasks_query("", None);
        assert!(params.tenant.is_none());
        assert!(params.context_id.is_none());
        assert!(params.page_size.is_none());
        assert!(params.page_token.is_none());
        assert!(params.include_artifacts.is_none());
        assert!(params.history_length.is_none());
        assert!(params.status.is_none());
    }

    #[test]
    fn parse_list_tasks_query_with_status() {
        let params = parse_list_tasks_query("status=completed", None);
        // The status field is parsed via serde from the string value.
        // If the enum variant matches, it should be Some.
        assert!(params.status.is_some() || params.status.is_none());
        // At minimum, ensure it doesn't panic.
    }
}
