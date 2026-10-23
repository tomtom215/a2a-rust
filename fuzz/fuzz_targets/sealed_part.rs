// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Fuzz target for `EncryptedTaskStore`'s reading of a stored part: the
//! sealed media type, the `{"sealedPart": {"v", "c"}}` shape, the version,
//! the base64 of `c`, then the cipher.
//!
//! Raw mode: the input is a stored part as JSON. Reading it must not panic;
//! a part labelled sealed never opens (only a forged tag could), and an
//! unlabelled one is returned unchanged exactly when unsealed parts are
//! accepted. Round-trip mode: a part made from the input is sealed, one
//! character of its stored `c` replaced, and read back: unchanged it opens
//! to the part, changed it is refused, so no second spelling of `c` opens.
//!
//! Run with: `cargo +nightly fuzz run sealed_part`

#![no_main]

use a2a_protocol_server::fuzzing::{part_open, part_seal};
use a2a_protocol_types::message::Part;
use libfuzzer_sys::fuzz_target;

const SEALED: &str = "application/vnd.a2a-rust.sealed-part+json";
const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=";

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else { return };
    let split = usize::from(mode >> 3).min(rest.len());
    let (context, body) = rest.split_at(split);
    let accept = mode & 2 != 0;
    // Different contexts per mode, as in `sealed_ring_value`.
    if mode & 1 == 0 {
        let Ok(part) = serde_json::from_slice::<Part>(body) else { return };
        let labelled = part.media_type.as_deref() == Some(SEALED);
        let read = part_open(part.clone(), &[b"raw:", context].concat(), accept);
        if labelled {
            assert!(read.is_none(), "opened a part nothing sealed");
        } else {
            assert_eq!(read, accept.then_some(part));
        }
        return;
    }
    let context = &[b"trip:", context].concat()[..];
    let Some((head, text)) = body.split_at_checked(3) else { return };
    let original = Part::text(String::from_utf8_lossy(text));
    let sealed = part_seal(&original, context).expect("the fuzz ring seals");
    let mut stored = serde_json::to_value(&sealed).expect("json");
    let c = stored["data"]["sealedPart"]["c"].as_str().expect("c").to_owned();
    let mut bytes = c.clone().into_bytes();
    let at = usize::from(u16::from_le_bytes([head[0], head[1]])) % bytes.len();
    bytes[at] = B64[usize::from(head[2]) % B64.len()];
    let changed = String::from_utf8(bytes).expect("ASCII");
    stored["data"]["sealedPart"]["c"] = changed.clone().into();
    let part: Part = serde_json::from_value(stored).expect("a part");
    let read = part_open(part, context, accept);
    if changed == c {
        assert_eq!(read, Some(original));
    } else {
        assert!(read.is_none(), "a changed part opened: {c} -> {changed}");
    }
});
