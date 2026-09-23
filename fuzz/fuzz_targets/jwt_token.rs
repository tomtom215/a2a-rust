// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Fuzz target for bearer-token (JWT) validation.
//!
//! A bearer token is the first peer-controlled input `JwtAuthInterceptor`
//! reads: three base64url segments, a JSON header naming an algorithm and a
//! key id, JSON claims and a signature. Validation must reject anything
//! malformed with an error and never panic, and must accept nothing it
//! cannot verify.
//!
//! Run with: `cargo +nightly fuzz run jwt_token`

#![no_main]

use a2a_protocol_server::fuzzing::jwt_validate;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(token) = std::str::from_utf8(data) {
        // No input the fuzzer can produce carries a valid signature by the
        // fixed keys except by forging one, so acceptance is itself a bug.
        assert!(!jwt_validate(token), "accepted an unsigned token: {token:?}");
    }
});
