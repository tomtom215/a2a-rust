// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Differential fuzz target for the protobuf <-> serde conversion layer.
//!
//! Decodes arbitrary bytes as each canonical protobuf message, converts to
//! the serde domain type, converts back, and re-encodes. The property under
//! test: **conversion never panics**, in either direction, on any input a
//! real gRPC peer could put on the wire. A round-trip that succeeds one way
//! must not blow up on the way back.
//!
//! Run with: `cargo +nightly fuzz run proto_convert`

#![no_main]

use a2a_protocol_types::proto;
use libfuzzer_sys::fuzz_target;
use prost::Message as _;

fuzz_target!(|data: &[u8]| {
    // Message: pb -> domain -> pb.
    if let Ok(pb_msg) = proto::Message::decode(data) {
        if let Ok(domain) = a2a_protocol_types::Message::try_from(pb_msg) {
            if let Ok(back) = proto::Message::try_from(domain) {
                let _ = back.encode_to_vec();
            }
        }
    }

    // Task: pb -> domain -> pb.
    if let Ok(pb_task) = proto::Task::decode(data) {
        if let Ok(domain) = a2a_protocol_types::Task::try_from(pb_task) {
            if let Ok(back) = proto::Task::try_from(domain) {
                let _ = back.encode_to_vec();
            }
        }
    }

    // AgentCard: pb -> domain (one direction; card has no infallible inverse
    // needed here — decoding + conversion is the fragile path).
    if let Ok(pb_card) = proto::AgentCard::decode(data) {
        let _ = a2a_protocol_types::AgentCard::try_from(pb_card);
    }

    // SendMessageRequest: pb -> domain params.
    if let Ok(pb_req) = proto::SendMessageRequest::decode(data) {
        let _ = a2a_protocol_types::params::MessageSendParams::try_from(pb_req);
    }
});
