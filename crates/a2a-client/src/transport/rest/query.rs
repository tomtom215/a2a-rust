// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Query string building and percent-encoding for REST transport URIs.

/// Builds a URL query string from a JSON object's non-null fields.
///
/// Values are percent-encoded per RFC 3986 to avoid query-string
/// injection (e.g., values containing `&`, `=`, or spaces).
pub(super) fn build_query_string(params: &serde_json::Value) -> String {
    let Some(obj) = params.as_object() else {
        return String::new();
    };
    let mut parts = Vec::new();
    for (k, v) in obj {
        let raw = match v {
            serde_json::Value::Null => continue,
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            _ => match serde_json::to_string(v) {
                Ok(s) => s,
                Err(_) => continue,
            },
        };
        parts.push(format!(
            "{}={}",
            encode_query_value(k),
            encode_query_value(&raw)
        ));
    }
    parts.join("&")
}

/// Percent-encodes a string for safe use in a URL query parameter.
///
/// Encodes all characters except unreserved characters (RFC 3986 §2.3):
/// `A-Z a-z 0-9 - . _ ~`
pub(super) fn encode_query_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(char::from(HEX_CHARS[(b >> 4) as usize]));
                out.push(char::from(HEX_CHARS[(b & 0x0F) as usize]));
            }
        }
    }
    out
}

const HEX_CHARS: [u8; 16] = *b"0123456789ABCDEF";

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_query_value_encodes_special_chars() {
        // Spaces
        assert_eq!(encode_query_value("hello world"), "hello%20world");
        // Ampersand & equals — would break query string parsing if unencoded
        assert_eq!(encode_query_value("a=1&b=2"), "a%3D1%26b%3D2");
        // Percent sign itself
        assert_eq!(encode_query_value("100%"), "100%25");
        // Unreserved characters pass through
        assert_eq!(
            encode_query_value("safe-._~AZaz09"),
            "safe-._~AZaz09"
        );
    }

    #[test]
    fn build_query_string_encodes_values() {
        let params = serde_json::json!({
            "filter": "status=active&role=admin",
            "name": "John Doe"
        });
        let qs = build_query_string(&params);
        // Values should be percent-encoded
        assert!(qs.contains("filter=status%3Dactive%26role%3Dadmin"));
        assert!(qs.contains("name=John%20Doe"));
    }

    // ── Mutation-killing tests for build_query_string null/number/bool ───

    #[test]
    fn build_query_string_skips_null() {
        let params = serde_json::json!({"a": null, "b": "hello"});
        let qs = build_query_string(&params);
        assert!(!qs.contains("a="), "null values should be skipped");
        assert!(qs.contains("b=hello"), "non-null values should be present");
    }

    #[test]
    fn build_query_string_handles_number() {
        let params = serde_json::json!({"count": 42});
        let qs = build_query_string(&params);
        assert_eq!(qs, "count=42");
    }

    #[test]
    fn build_query_string_handles_bool() {
        let params = serde_json::json!({"active": true});
        let qs = build_query_string(&params);
        assert_eq!(qs, "active=true");
    }

    #[test]
    fn build_query_string_handles_false() {
        let params = serde_json::json!({"active": false});
        let qs = build_query_string(&params);
        assert_eq!(qs, "active=false");
    }
}
