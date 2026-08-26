// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for the counter-tests' one decision.
//!
//! `run` needs two live agents — one advertising optional capabilities and one
//! advertising none — because a single agent cannot both support and not
//! support a feature. What does not need them is `code_of`, which decides
//! whether a call was refused *for the right reason*, and that is the whole
//! verdict a counter-test produces.

use a2a_protocol_client::ClientError;
use a2a_protocol_types::error::{A2aError, ErrorCode};

use super::code_of;
use crate::tests::transport_failures;

/// A counter-test passes only when the server refuses with the code the
/// specification names. `code_of` is what supplies that code.
#[test]
fn a_protocol_error_yields_its_code() {
    for want in [
        ErrorCode::InvalidParams,
        ErrorCode::MethodNotFound,
        ErrorCode::InternalError,
    ] {
        let e = ClientError::Protocol(A2aError::new(want, "no"));
        assert_eq!(code_of(&e), Some(want));
    }
}

/// Everything else yields `None`, so a counter-test cannot pass on a
/// connection failure. That is the failure worth guarding: an agent that is
/// simply unreachable refuses *every* call, and a checker that accepted a
/// transport error as a refusal would report a clean sweep against a server
/// that was never running.
#[test]
fn a_transport_failure_yields_no_code() {
    for e in transport_failures() {
        assert_eq!(
            code_of(&e),
            None,
            "a transport failure produced a protocol code: {e}"
        );
    }
}
