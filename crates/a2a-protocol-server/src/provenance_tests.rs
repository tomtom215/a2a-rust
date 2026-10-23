// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What the signing wrappers pass through and what they sign.

#![cfg(feature = "signing")]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::provenance::{ContentSigner, provenance_of};
use a2a_protocol_types::push::TaskPushNotificationConfig;

use super::*;
use crate::push::PushSender;

fn signer() -> ContentSigner {
    let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
        .expect("a key");
    ContentSigner::ed25519("k", pkcs8.as_ref()).expect("a signer")
}

/// A sender whose answers are not the trait's defaults.
struct Answers {
    private: bool,
}

impl PushSender for Answers {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn allows_private_urls(&self) -> bool {
        self.private
    }

    fn requires_https(&self) -> bool {
        !self.private
    }

    fn max_delivery_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs(7))
    }
}

/// The wrapper answers as the sender it wraps, so signing changes no
/// delivery policy.
#[test]
fn the_signing_push_sender_answers_as_the_one_it_wraps() {
    for private in [true, false] {
        let wrapped = SigningPushSender {
            inner: Arc::new(Answers { private }),
            outbound: Outbound::signing(signer()),
        };
        assert_eq!(wrapped.allows_private_urls(), private);
        assert_eq!(wrapped.requires_https(), !private);
        assert_eq!(
            wrapped.max_delivery_duration(),
            Some(Duration::from_secs(7))
        );
    }
}

#[test]
fn a_blocking_message_answer_is_signed() {
    let outbound = Outbound::signing(signer());
    let mut response = SendMessageResponse::Message(Message::user_text("m-1", "hello"));
    outbound.sign_response(&mut response).unwrap();
    let SendMessageResponse::Message(message) = response else {
        unreachable!("a message stays a message")
    };
    let provenance = provenance_of(&message).unwrap().expect("provenance");
    assert!(provenance.signature.is_some());
}

#[test]
fn a_streamed_message_is_signed() {
    let outbound = Outbound::signing(signer());
    let mut event = StreamResponse::Message(Message::user_text("m-1", "hello"));
    outbound.sign_event(&mut event).unwrap();
    let StreamResponse::Message(message) = event else {
        unreachable!("a message stays a message")
    };
    let provenance = provenance_of(&message).unwrap().expect("provenance");
    assert!(provenance.signature.is_some());
}

/// `Debug` says how many keys there are and whether unsigned content is
/// refused, never the keys themselves.
#[test]
fn inbound_debugs_as_its_key_count_and_policy() {
    let keys = a2a_protocol_types::signing::Jwks::new(vec![]);
    assert_eq!(
        format!("{:?}", Inbound::verifying(keys).requiring_signatures()),
        "Inbound { keys: Some(0), require_signed: true }"
    );
}

/// `Debug` says whether content is signed, and nothing of the key.
#[test]
fn outbound_debugs_as_whether_it_signs() {
    assert_eq!(
        format!("{:?}", Outbound::signing(signer())),
        "Outbound { signs: true }"
    );
    assert_eq!(
        format!("{:?}", Outbound::default()),
        "Outbound { signs: false }"
    );
}
