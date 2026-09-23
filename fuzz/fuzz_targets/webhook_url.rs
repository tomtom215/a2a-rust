// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Fuzz target for the push-notification webhook URL check.
//!
//! A client registers the URL the server will later POST to, so the SSRF
//! check parses a hostile string: URI syntax, bracketed IPv6, C-style numeric
//! IPv4 hosts (decimal, octal, hex, packed short forms) and hostnames. It
//! must never panic.
//!
//! Run with: `cargo +nightly fuzz run webhook_url`

#![no_main]

use a2a_protocol_server::fuzzing::webhook_url_allowed;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(url) = std::str::from_utf8(data) {
        let _ = webhook_url_allowed(url);
    }
});
