// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Fuzz target for an Agent Skills `SKILL.md` (ADR 0035).
//!
//! The file is whatever someone who can write to a skills directory chose: the
//! frontmatter's size check, the reader of the YAML subset it is written in
//! (plain, quoted and block scalars, one level of mapping, and a name for every
//! construct it does not read: anchors, aliases, tags, flow collections), and
//! the checks of each field and of the body. It must never panic, however the
//! input is cut, nested or escaped.
//!
//! Run with: `cargo +nightly fuzz run skill_frontmatter`

#![no_main]

use a2a_protocol_server::skills::fuzz_frontmatter;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The file is read as UTF-8 before it is parsed, and one that is not is
    // skipped, so only text reaches the parser.
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = fuzz_frontmatter(text);
    }
});
