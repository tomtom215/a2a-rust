// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The typed refusal of a keyed send by a server that cannot honour keys
//! (COR-06, DEC-08 as amended 2026-11-26).
//!
//! A server whose store cannot deduplicate refuses a keyed send rather than
//! run it without deduplication, and it does so before anything is claimed or
//! run. The refusal names itself with a `google.rpc.ErrorInfo` detail, reason
//! [`KEYS_UNSUPPORTED_REASON`] in the `a2a-rust.com` domain, so a client that
//! added a key on its own can send the message again without one, knowing the
//! first send did nothing, without reading the error's prose.

use crate::error::{A2aError, ErrorCode};
use crate::message::Message;

/// The `ErrorInfo` `reason` of the refusal.
pub const KEYS_UNSUPPORTED_REASON: &str = "IDEMPOTENCY_KEYS_UNSUPPORTED";

/// The `ErrorInfo` `domain` of the refusal: this SDK's, as A2A defines no
/// such reason.
const DOMAIN: &str = "a2a-rust.com";

const ERROR_INFO: &str = "type.googleapis.com/google.rpc.ErrorInfo";

/// The refusal: `UnsupportedOperation`, `message` as its text, and the typed
/// detail.
#[must_use]
pub fn keys_unsupported(message: impl Into<String>) -> A2aError {
    A2aError::with_data(
        ErrorCode::UnsupportedOperation,
        message,
        serde_json::json!([{
            "@type": ERROR_INFO,
            "reason": KEYS_UNSUPPORTED_REASON,
            "domain": DOMAIN,
        }]),
    )
}

/// Whether `error` is that refusal: an `UnsupportedOperation` whose `data`
/// holds the typed detail, as a list of details or as the one detail.
#[must_use]
pub fn refuses_keys(error: &A2aError) -> bool {
    if error.code != ErrorCode::UnsupportedOperation {
        return false;
    }
    let names_it = |d: &serde_json::Value| {
        d.get("@type").and_then(serde_json::Value::as_str) == Some(ERROR_INFO)
            && d.get("domain").and_then(serde_json::Value::as_str) == Some(DOMAIN)
            && d.get("reason").and_then(serde_json::Value::as_str) == Some(KEYS_UNSUPPORTED_REASON)
    };
    match &error.data {
        Some(serde_json::Value::Array(details)) => details.iter().any(names_it),
        Some(detail) => names_it(detail),
        None => false,
    }
}

/// Takes the key off `message`: its metadata entry, and the extension URI
/// from `extensions` (an emptied `extensions` or `metadata` is left as
/// `None`, as a message that never carried a key has it).
pub fn remove_key(message: &mut Message) {
    if let Some(serde_json::Value::Object(map)) = message.metadata.as_mut() {
        map.remove(super::IDEMPOTENCY_METADATA_KEY);
        if map.is_empty() {
            message.metadata = None;
        }
    }
    if let Some(uris) = message.extensions.as_mut() {
        uris.retain(|u| u != super::IDEMPOTENCY_EXTENSION_URI);
        if uris.is_empty() {
            message.extensions = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_refusal_is_recognised_and_nothing_else_is() {
        assert!(refuses_keys(&keys_unsupported("no keys here")));
        let one = A2aError::with_data(
            ErrorCode::UnsupportedOperation,
            "x",
            serde_json::json!({ "@type": ERROR_INFO, "reason": KEYS_UNSUPPORTED_REASON, "domain": DOMAIN }),
        );
        assert!(refuses_keys(&one), "a lone detail");
        assert!(!refuses_keys(&A2aError::new(
            ErrorCode::UnsupportedOperation,
            "cannot honour keys"
        )));
        let other_code = A2aError::with_data(
            ErrorCode::InvalidParams,
            "x",
            keys_unsupported("x").data.unwrap(),
        );
        assert!(
            !refuses_keys(&other_code),
            "the code must be UnsupportedOperation"
        );
        let other_domain = A2aError::with_data(
            ErrorCode::UnsupportedOperation,
            "x",
            serde_json::json!([{ "@type": ERROR_INFO, "reason": KEYS_UNSUPPORTED_REASON, "domain": "a2a-protocol.org" }]),
        );
        assert!(!refuses_keys(&other_domain));
    }

    #[test]
    fn removing_the_key_leaves_the_message_as_it_was_before_it() {
        let bare = Message::user("m", vec![crate::message::Part::text("hi")]);
        let mut keyed = bare.clone();
        super::super::set_key(&mut keyed, &"k".repeat(64)).unwrap();
        remove_key(&mut keyed);
        assert_eq!(keyed, bare);
        let mut kept = bare;
        kept.metadata = Some(serde_json::json!({ "mine": 1 }));
        kept.extensions = Some(vec!["urn:x".to_owned()]);
        let before = kept.clone();
        super::super::set_key(&mut kept, &"k".repeat(64)).unwrap();
        remove_key(&mut kept);
        assert_eq!(kept, before, "other metadata and extensions are kept");
    }
}
