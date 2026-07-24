// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Fuzz target for JWKS parsing.
//!
//! `Jwks::from_json` parses a key set fetched from a remote OIDC/JWKS
//! endpoint — an external, potentially hostile document (base64url moduli,
//! EC coordinates, arbitrary key types). It must reject malformed input
//! with an error, never panic.
//!
//! Run with: `cargo +nightly fuzz run jwks_parse`

#![no_main]

use a2a_protocol_server::auth::Jwks;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Errors are expected and fine; panics (index-out-of-bounds on short
    // base64, integer overflow on key sizes, etc.) are bugs.
    let _ = Jwks::from_json(data);
});
