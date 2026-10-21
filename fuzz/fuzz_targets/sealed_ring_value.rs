// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Fuzz target for `Aes256GcmCipher`'s open: the stored value's key-id
//! length byte, key id, nonce, ciphertext and tag, whatever the store holds.
//!
//! Two modes, chosen by the first byte. Raw: the input is a stored value;
//! opening must not panic, and nothing opens, since only a forged tag could.
//! Round trip: the input's plaintext is sealed, the sealed value changed (a
//! byte flipped, cut short, extended, its length byte changed, or left
//! alone), then opened: unchanged it opens to the plaintext, laid out as
//! the cipher documents; changed in any way it is refused.
//!
//! Run with: `cargo +nightly fuzz run sealed_ring_value`

#![no_main]

use a2a_protocol_server::fuzzing::{ring_open, ring_seal};
use libfuzzer_sys::fuzz_target;

/// Changes `sealed` as `how` and `arg` say; the identity is one choice.
fn alter(sealed: &mut Vec<u8>, how: u8, arg: u16) {
    let at = usize::from(arg) % sealed.len().max(1);
    match how % 5 {
        0 => {}
        1 => sealed[at] ^= (arg >> 8) as u8 | 1,
        2 => sealed.truncate(at),
        3 => sealed.extend_from_slice(&arg.to_le_bytes()[..1 + usize::from(arg & 1)]),
        _ => sealed[0] = sealed[0].wrapping_add((arg as u8) | 1),
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else { return };
    let split = usize::from(mode >> 2).min(rest.len());
    let (context, body) = rest.split_at(split);
    // The modes use different contexts: libFuzzer lifts the round trip's
    // sealed values out of its comparisons and splices them into raw
    // inputs, where under the same context they would rightly open (the
    // first run did exactly that).
    if mode & 1 == 0 {
        let context = [b"raw:", context].concat();
        assert!(ring_open(body, &context).is_none(), "opened an unsealed value");
        return;
    }
    let context = &[b"trip:", context].concat()[..];
    let Some((head, plaintext)) = body.split_at_checked(3) else { return };
    let sealed = ring_seal(plaintext, context).expect("the fuzz ring seals");
    assert_eq!(sealed.len(), 2 + 12 + plaintext.len() + 16);
    assert_eq!(&sealed[..2], b"\x01k");
    let mut changed = sealed.clone();
    alter(&mut changed, head[0], u16::from_le_bytes([head[1], head[2]]));
    let opened = ring_open(&changed, context);
    if changed == sealed {
        assert_eq!(opened.as_deref(), Some(plaintext));
    } else {
        assert!(opened.is_none(), "a changed value opened");
    }
});
