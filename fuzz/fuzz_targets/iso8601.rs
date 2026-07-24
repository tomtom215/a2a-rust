// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Fuzz target for the ISO-8601 timestamp parser.
//!
//! `parse_iso8601_to_unix_millis` runs on stored task timestamps (§5.6.1)
//! and on the client-supplied `statusTimestampAfter` filter — both
//! attacker-influenced. It must never panic on any input, and the
//! parse -> format -> parse round-trip must be stable for values it accepts.
//!
//! Run with: `cargo +nightly fuzz run iso8601`

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };

    // Never panics regardless of input.
    if let Some(millis) = a2a_protocol_types::parse_iso8601_to_unix_millis(s) {
        // Accepted values must round-trip through the canonical formatter
        // back to the same instant (the ordering guarantee depends on this).
        let formatted = a2a_protocol_types::unix_millis_to_iso8601(millis);
        let reparsed = a2a_protocol_types::parse_iso8601_to_unix_millis(&formatted);
        assert_eq!(
            reparsed,
            Some(millis),
            "canonical form {formatted:?} did not round-trip (from input {s:?})"
        );
    }
});
