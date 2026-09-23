// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Fuzz target for the REST binding's request line: the path's tenant prefix
//! and the `ListTasks` query string.
//!
//! Both are parsed by hand from whatever the peer sends — the `/{tenant}/`
//! and `/tenants/{tenant}/` prefixes, percent-decoding, `+`, numeric and
//! boolean parameters. The input is split at its first `?`, as a request
//! target is. Parsing must never panic, and the path left after the prefix
//! must be a suffix of the path given.
//!
//! Run with: `cargo +nightly fuzz run rest_route`

#![no_main]

use a2a_protocol_server::fuzzing::rest_route;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(target) = std::str::from_utf8(data) else { return };
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let (_, rest, _) = rest_route(path, query);
    assert!(path.ends_with(rest.as_str()), "{rest:?} is not a suffix of {path:?}");
});
