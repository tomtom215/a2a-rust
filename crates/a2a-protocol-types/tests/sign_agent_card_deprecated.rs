// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `sign_agent_card`, deprecated in 0.15.0 and kept for `STABILITY.md` §3's
//! one-minor window, signs as `sign_card_es256` does with a key id, and
//! refuses `None`, which in 0.14 wrote a protected header without the `kid`
//! spec §8.4.2 requires.

#![cfg(feature = "signing")]
#![allow(deprecated)]

use a2a_protocol_types::agent_card::AgentCard;
use a2a_protocol_types::error::ErrorCode;
use a2a_protocol_types::signing::{sign_agent_card, verify_agent_card};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::SystemRandom;
use ring::signature::{self, EcdsaKeyPair, KeyPair as _};

fn card() -> AgentCard {
    serde_json::from_value(serde_json::json!({
        "name": "Test Agent",
        "description": "A test agent",
        "version": "1.0.0",
        "supportedInterfaces": [{
            "url": "https://agent.example.com/rpc",
            "protocolBinding": "JSONRPC",
            "protocolVersion": "1.0.0"
        }],
        "capabilities": {},
        "defaultInputModes": ["text/plain"],
        "defaultOutputModes": ["text/plain"],
        "skills": [{"id": "echo", "name": "Echo", "description": "Echoes input", "tags": ["echo"]}]
    }))
    .expect("a card")
}

#[test]
fn the_deprecated_sign_agent_card_needs_a_key_id() {
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
        .expect("a key");
    let pair = EcdsaKeyPair::from_pkcs8(
        &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        pkcs8.as_ref(),
        &rng,
    )
    .expect("the key parses");
    let card = card();

    let sig = sign_agent_card(&card, pkcs8.as_ref(), Some("key-1")).expect("signed with a key id");
    let header: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&sig.protected).expect("base64"))
            .expect("JSON");
    assert_eq!(
        header,
        serde_json::json!({"alg": "ES256", "typ": "JOSE", "kid": "key-1"})
    );
    verify_agent_card(&card, &sig, pair.public_key().as_ref()).expect("verifies");

    let err = sign_agent_card(&card, pkcs8.as_ref(), None).expect_err("no kid, no signature");
    assert_eq!(err.code, ErrorCode::InvalidParams, "{err:?}");
}
