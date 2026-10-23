// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Fuzz target for the SQL push-config stores' reading of a stored config
//! with a cipher: the `a2a-sealed:v1:` prefix, the base64, then the cipher,
//! or plain JSON when unsealed rows are accepted.
//!
//! Raw mode: the input is the stored text. Reading it must not panic; a
//! sealed text never opens (only a forged tag could), and an unsealed one is
//! read only when unsealed rows are accepted. Round-trip mode: a config made
//! from the input is sealed, one byte of the stored text replaced, and read
//! back: unchanged it reads as the config, changed it is refused.
//!
//! Run with: `cargo +nightly fuzz run sealed_push_config`

#![no_main]

use a2a_protocol_server::fuzzing::{push_decode, push_encode};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use libfuzzer_sys::fuzz_target;

const PREFIX: &str = "a2a-sealed:v1:";
const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=:a";

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else { return };
    let split = usize::from(mode >> 3).min(rest.len());
    let (context, body) = rest.split_at(split);
    let accept = mode & 2 != 0;
    // Different contexts per mode, as in `sealed_ring_value`.
    if mode & 1 == 0 {
        let Ok(text) = std::str::from_utf8(body) else { return };
        let read = push_decode(text, &[b"raw:", context].concat(), accept);
        if text.starts_with(PREFIX) || !accept {
            assert!(read.is_none(), "read {text:?}");
        }
        return;
    }
    let context = &[b"trip:", context].concat()[..];
    let Some((head, url)) = body.split_at_checked(3) else { return };
    let config = TaskPushNotificationConfig::new("task", String::from_utf8_lossy(url));
    let stored = push_encode(&config, context).expect("the fuzz ring seals");
    let mut bytes = stored.clone().into_bytes();
    let at = usize::from(u16::from_le_bytes([head[0], head[1]])) % bytes.len();
    bytes[at] = B64[usize::from(head[2]) % B64.len()];
    let changed = String::from_utf8(bytes).expect("ASCII");
    let read = push_decode(&changed, context, accept);
    if changed == stored {
        let read = read.expect("opens");
        assert_eq!(
            serde_json::to_value(read).expect("json"),
            serde_json::to_value(config).expect("json")
        );
    } else if changed.starts_with(PREFIX) || !accept {
        assert!(read.is_none(), "a changed config opened: {changed}");
    }
});
