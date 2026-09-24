// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Fuzz target for the rate limiter's `x-forwarded-for` handling.
//!
//! Behind trusted proxies the rate limiter keys each caller by an entry of
//! `x-forwarded-for`, which the client writes in part. The derivation must
//! never panic, whatever the header holds and however many hops are trusted.
//!
//! Run with: `cargo +nightly fuzz run forwarded_for`

#![no_main]

use a2a_protocol_server::fuzzing::caller_key_from_forwarded_for;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&hops, rest)) = data.split_first() else { return };
    if let Ok(xff) = std::str::from_utf8(rest) {
        let _ = caller_key_from_forwarded_for(xff, usize::from(hops % 8));
    }
});
