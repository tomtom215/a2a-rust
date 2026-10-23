// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The idempotency key each tool call carries (ADR 0026).
//!
//! A key is HMAC-SHA256, under the loop's secret, of the run's scope, the
//! tool's name and the call's arguments in canonical form, each field
//! length-prefixed, rendered as 64 lowercase hex digits. So:
//!
//! - the same call in the same scope has the same key, whatever order the
//!   model wrote the arguments' fields in;
//! - a different call, or the same call in another scope (another task, or
//!   another message to the same task), has a different key;
//! - nobody without the secret can compute a key, so one cannot be guessed
//!   from a task id and a tool's arguments.

use std::fmt::Write as _;

use ring::hmac;
use serde_json::Value;

/// The secret keys are derived under, 32 bytes.
pub(super) type Secret = [u8; 32];

/// A fresh secret: two version-4 UUIDs, 244 random bits from the operating
/// system's generator, the source the handler already draws task ids from.
pub(super) fn fresh_secret() -> Secret {
    let mut secret = [0u8; 32];
    secret[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    secret[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    secret
}

/// A fresh scope for a run nothing else names: 122 random bits.
pub(super) fn fresh_scope() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// The scope of a run answering message `message_id` of task `task_id`.
/// Length-prefixed, so no two pairs of ids make the same scope.
pub(super) fn task_scope(task_id: &str, message_id: &str) -> String {
    format!("{}:{task_id}{message_id}", task_id.len())
}

/// The key of a call to `name` with `arguments` in `scope`.
pub(super) fn call_key(secret: &Secret, scope: &str, name: &str, arguments: &Value) -> String {
    let mut canonical_arguments = String::new();
    canonical(arguments, &mut canonical_arguments);
    let mut ctx = hmac::Context::with_key(&hmac::Key::new(hmac::HMAC_SHA256, secret));
    for field in [scope, name, canonical_arguments.as_str()] {
        ctx.update(&(field.len() as u64).to_be_bytes());
        ctx.update(field.as_bytes());
    }
    let mut key = String::with_capacity(64);
    for byte in ctx.sign().as_ref() {
        let _ = write!(key, "{byte:02x}");
    }
    key
}

/// `value` as JSON with every object's fields sorted by name and no
/// whitespace. Strings and numbers are written as `serde_json` writes them,
/// so `1` and `1.0` stay different calls.
fn canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            // serde_json's map is sorted already, unless a crate in the
            // build turns on its `preserve_order` (genai 0.7 does), when it
            // keeps the model's order. Without this sort, two tests fail
            // under `--features serde_json/preserve_order`.
            let mut fields: Vec<_> = map.iter().collect();
            fields.sort_unstable_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (i, (name, field)) in fields.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let _ = write!(out, "{}", Value::String(name.clone()));
                out.push(':');
                canonical(field, out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(item, out);
            }
            out.push(']');
        }
        leaf => {
            let _ = write!(out, "{leaf}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SECRET: Secret = [7; 32];

    #[test]
    fn field_order_does_not_change_the_key() {
        let a: Value = serde_json::from_str(r#"{"to":"ops","body":{"x":1,"y":[2,{"b":1,"a":2}]}}"#)
            .expect("json");
        let b: Value = serde_json::from_str(r#"{"body":{"y":[2,{"a":2,"b":1}],"x":1},"to":"ops"}"#)
            .expect("json");
        assert_eq!(
            call_key(&SECRET, "s", "notify", &a),
            call_key(&SECRET, "s", "notify", &b)
        );
        let mut text = String::new();
        canonical(&a, &mut text);
        assert_eq!(text, r#"{"body":{"x":1,"y":[2,{"a":2,"b":1}]},"to":"ops"}"#);
    }

    #[test]
    fn every_input_changes_the_key() {
        let args = json!({"service": "billing"});
        let key = call_key(&SECRET, "s", "restart", &args);
        assert_eq!(key.len(), 64);
        assert!(
            key.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        for other in [
            call_key(&[8; 32], "s", "restart", &args),
            call_key(&SECRET, "t", "restart", &args),
            call_key(&SECRET, "s", "restart2", &args),
            call_key(&SECRET, "s", "restart", &json!({"service": "checkout"})),
            call_key(&SECRET, "s", "restart", &json!({"service": ["billing"]})),
            call_key(
                &SECRET,
                "s",
                "restart",
                &json!({"service": "billing", "force": true}),
            ),
        ] {
            assert_ne!(key, other);
        }
    }

    #[test]
    fn fields_cannot_run_into_each_other() {
        let args = json!({});
        assert_ne!(
            call_key(&SECRET, "ab", "c", &args),
            call_key(&SECRET, "a", "bc", &args)
        );
        assert_ne!(task_scope("ab", "c"), task_scope("a", "bc"));
    }

    #[test]
    fn the_key_is_hmac_sha256_of_the_length_prefixed_fields() {
        // Computed with Python's hmac module, not this code:
        //   m = b''.join(struct.pack('>Q', len(f)) + f
        //                for f in [b's', b'n', b'{"a":1}'])
        //   hmac.new(bytes([7] * 32), m, hashlib.sha256).hexdigest()
        // so a change to the framing (prefix width, byte order, field
        // order) or the canonical form fails here.
        assert_eq!(
            call_key(&SECRET, "s", "n", &json!({"a": 1})),
            "97ad54f73f4284934124eaa46532d54f4c13c58cd6e0bf5d51c38fc937ede4df"
        );
    }

    #[test]
    fn fresh_secrets_and_scopes_differ() {
        assert_ne!(fresh_secret(), fresh_secret());
        assert_ne!(fresh_scope(), fresh_scope());
    }
}
