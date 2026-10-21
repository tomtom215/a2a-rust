// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Fuzz target for the audit log's reading of what `with_privacy` stores: a
//! sealed value (`a2a-sealed:v1:<sid>:<base64>`), the wrapped subject key
//! it is opened with, and a keyed digest (`hmac-sha256:<kid>:<hex>`).
//!
//! A record protected once per process is changed in one place, by a byte
//! replaced or by the whole value replaced with the input, and read back
//! with `AuditLog::reveal` and `digest_matches`. Nothing may panic.
//! Unchanged, every name reveals and the digest matches. Changed, a sealed
//! value is refused, or reads as erased (`""`, named in `erased`) when its
//! `sid` no longer names a key, or, when it no longer carries the sealed
//! prefix, reads as itself, a value stored in the clear (documented on
//! `AuditLog::reveal`); it never reveals another name. A changed wrapped
//! key is refused, and a changed digest never matches.
//!
//! Run with: `cargo +nightly fuzz run sealed_audit_value`

#![no_main]

use a2a_protocol_server::fuzzing::{audit_digest_matches, audit_fixture, audit_reveal};
use libfuzzer_sys::fuzz_target;

const SET: &[u8] = b"0123456789abcdefABCDEF+/=:-xa2sl";
const NAMES: [&str; 5] = ["alice", "bob", "why", "agent", "alice"];
const FIELDS: [&str; 5] = ["actor", "requestedBy", "reason", "actors/0", "actors/1"];

fn alter(value: &str, head: &[u8], rest: &[u8]) -> String {
    if head[0] & 0x80 != 0 {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let mut bytes = value.as_bytes().to_vec();
    let at = usize::from(u16::from_le_bytes([head[1], head[2]])) % bytes.len().max(1);
    if let Some(b) = bytes.get_mut(at) {
        *b = SET[usize::from(head[3]) % SET.len()];
    }
    String::from_utf8(bytes).expect("ASCII")
}

fuzz_target!(|data: &[u8]| {
    let Some((head, rest)) = data.split_at_checked(4) else { return };
    let fx = audit_fixture().expect("the fixture");
    let mut record = fx.record.clone();
    let mut keys = fx.keys.clone();
    let which = usize::from(head[0] & 0x7f) % 7;
    let key = usize::from(head[1]) % keys.len();
    let original;
    let changed;
    {
        let slot: &mut String = match which {
            0 => &mut record.actor.as_mut().expect("actor").subject,
            5 => &mut keys[key].wrapped,
            6 => record.digests.get_mut("message").expect("digest"),
            i => {
                let v = match i {
                    1 => record.detail.get_mut("requestedBy"),
                    2 => record.detail.get_mut("reason"),
                    n => record.detail.get_mut("actors").and_then(|a| a.get_mut(n - 3)),
                };
                let serde_json::Value::String(s) = v.expect("a field") else { panic!() };
                s
            }
        };
        original = slot.clone();
        *slot = alter(&original, head, rest);
        changed = slot.clone();
    }
    let same = changed == original;
    let matches = audit_digest_matches(&record, "message", &fx.content);
    if which == 6 && !same {
        assert_ne!(matches, Some(true), "a changed digest matched: {changed}");
        return;
    }
    assert_eq!(matches, Some(true));
    let revealed = audit_reveal(&keys, &record);
    if same {
        let (r, erased) = revealed.expect("an unchanged record reveals");
        assert!(erased.is_empty(), "{erased:?}");
        assert_eq!(r.actor.expect("actor").subject, "alice");
        return;
    }
    if which == 5 {
        assert!(revealed.is_none(), "a changed wrapped key opened");
        return;
    }
    let Some((r, erased)) = revealed else { return };
    let got = match which {
        0 => r.actor.expect("actor").subject,
        1 => r.detail["requestedBy"].as_str().expect("str").to_owned(),
        2 => r.detail["reason"].as_str().expect("str").to_owned(),
        n => r.detail["actors"][n - 3].as_str().expect("str").to_owned(),
    };
    if !changed.starts_with("a2a-sealed:v1:") {
        assert_eq!(got, changed, "a value in the clear reads as itself");
    } else {
        assert!(got.is_empty(), "{} revealed {got:?}, not {:?}", FIELDS[which], NAMES[which]);
        assert_eq!(erased, [FIELDS[which]]);
    }
});
