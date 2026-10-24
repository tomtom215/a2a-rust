// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `ServerError::metric_label` for the A2A errors a binding sends as they are.

use super::*;

/// An A2A error the binding sends as it is is labelled for what it says: a
/// refused credential by its kind (spec §3.3.2), anything else by its code.
/// Each was `protocol` until 0.15, in the audit trail and in
/// `a2a.server.errors` alike.
#[test]
fn a_protocol_error_is_labelled_for_what_it_says() {
    for (expected, err) in [
        ("unauthenticated", A2aError::unauthenticated("no", "Bearer")),
        ("permission_denied", A2aError::permission_denied("no")),
        (
            "permission_denied",
            A2aError::insufficient_scope("no", ["tasks:write"]),
        ),
        (
            "version_not_supported",
            A2aError::new(ErrorCode::VersionNotSupported, "0.3"),
        ),
    ] {
        assert_eq!(ServerError::Protocol(err).metric_label(), expected);
    }
}
