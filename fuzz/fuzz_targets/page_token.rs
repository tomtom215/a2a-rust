// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Fuzz target for `ListTasks` page tokens and the `A2A-Extensions` header.
//!
//! Both come from the client verbatim: the in-memory store decodes the page
//! token it handed out (`millis:seq`) from whatever comes back, and the
//! extensions header is split into URIs. Neither may panic, and a decoded
//! token must re-encode to itself when the input was canonical.
//!
//! Run with: `cargo +nightly fuzz run page_token`

#![no_main]

use a2a_protocol_server::fuzzing::{extensions_header, page_token};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    if let Some((millis, seq)) = page_token(text) {
        let canonical = format!("{millis}:{seq}");
        if canonical.len() == text.len() {
            assert_eq!(canonical, text);
        }
    }
    for uri in extensions_header(text) {
        assert!(!uri.is_empty() && uri.trim() == uri, "{uri:?}");
    }
});
