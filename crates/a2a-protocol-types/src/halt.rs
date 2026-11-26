// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The refusal of a halted server, as a caller reads it (COR-05, ADR 0049
//! decision 4).
//!
//! An operator's halt of a server, a tenant or a job refuses new work with a
//! `google.rpc.ErrorInfo` detail, reason [`HALTED_REASON`] in the
//! `a2a-rust.com` domain, on every binding. A caller stops on it: a halt is
//! an operator's decision, not a fault, so retrying it only adds load to a
//! server told to stop, and counting it against the server's health would
//! eject a server that is answering as asked.

use crate::error::A2aError;

/// The `ErrorInfo` `reason` of a halt refusal.
pub const HALTED_REASON: &str = "HALTED";

/// The `ErrorInfo` `domain` of the reasons this SDK defines.
pub const A2A_RUST_ERROR_DOMAIN: &str = "a2a-rust.com";

const ERROR_INFO: &str = "type.googleapis.com/google.rpc.ErrorInfo";

/// The typed detail of a halt refusal.
#[must_use]
pub fn halted_detail() -> serde_json::Value {
    serde_json::json!({
        "@type": ERROR_INFO,
        "reason": HALTED_REASON,
        "domain": A2A_RUST_ERROR_DOMAIN,
    })
}

/// Whether `error` is a halt refusal: its `data` holds the typed detail, as a
/// list of details or as the one detail, whatever its code.
#[must_use]
pub fn is_halted(error: &A2aError) -> bool {
    let names_it = |d: &serde_json::Value| {
        d.get("@type").and_then(serde_json::Value::as_str) == Some(ERROR_INFO)
            && d.get("domain").and_then(serde_json::Value::as_str) == Some(A2A_RUST_ERROR_DOMAIN)
            && d.get("reason").and_then(serde_json::Value::as_str) == Some(HALTED_REASON)
    };
    match &error.data {
        Some(serde_json::Value::Array(details)) => details.iter().any(names_it),
        Some(detail) => names_it(detail),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn the_detail_is_read_in_a_list_or_alone_and_nothing_else_is_a_halt() {
        let alone = A2aError::with_data(ErrorCode::InternalError, "halted: x", halted_detail());
        assert!(is_halted(&alone));
        let listed = A2aError::with_data(
            ErrorCode::InternalError,
            "halted: x",
            serde_json::json!([{ "@type": ERROR_INFO, "reason": "INTERNAL_ERROR", "domain": "a2a-protocol.org" }, halted_detail()]),
        );
        assert!(is_halted(&listed));
        assert!(
            !is_halted(&A2aError::internal("halted: x")),
            "the prose is not the signal"
        );
        let foreign = A2aError::with_data(
            ErrorCode::InternalError,
            "x",
            serde_json::json!({ "@type": ERROR_INFO, "reason": HALTED_REASON, "domain": "example.com" }),
        );
        assert!(
            !is_halted(&foreign),
            "another domain's HALTED is not this one"
        );
    }
}
