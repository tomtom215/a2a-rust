// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Spec §8.4.1, two MUSTs the official TCK tags `NOT_AUTOMATABLE` because a
//! black-box client cannot see what a signer signed:
//!
//! - `CARD-SIGN-001`: "Before signing, the Agent Card content MUST be
//!   canonicalized using the JSON Canonicalization Scheme (JCS) as defined
//!   in RFC 8785."
//! - `CARD-SIGN-002`: "The signatures field itself MUST be excluded from the
//!   content being signed."
//!
//! In-process the signed bytes are visible, so each is checked directly:
//! the signature is verified here with `ring` over a JWS signing input this
//! test builds itself, which pins exactly which payload was signed.
//! `tck/untested-musts.toml` maps both requirements to these tests.

#![cfg(feature = "signing")]

use a2a_protocol_types::agent_card::AgentCard;
use a2a_protocol_types::extensions::AgentCardSignature;
use a2a_protocol_types::signing::{
    Jwk, Jwks, VerifyingKey, canonicalize, canonicalize_card, canonicalize_card_json,
    sign_card_ed25519, sign_card_ed25519_with_jku, sign_card_es256, sign_card_es256_with_jku,
    verify_agent_card, verify_card_json_with_jwks,
};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::SystemRandom;
use ring::signature::{self, EcdsaKeyPair, KeyPair, UnparsedPublicKey};
use serde_json::{Value, json};

fn card() -> AgentCard {
    serde_json::from_value(json!({
        "name": "Signed Agent", "description": "signs its card", "version": "1.2.0",
        "supportedInterfaces": [
            {"url": "https://agent.example/rpc", "protocolBinding": "JSONRPC", "protocolVersion": "1.0"}
        ],
        "capabilities": {"streaming": true},
        "defaultInputModes": ["text/plain"], "defaultOutputModes": ["text/plain"],
        "skills": [{"id": "s", "name": "Ünïcode ✓", "description": "d", "tags": ["t"]}]
    }))
    .expect("card")
}

/// A fresh P-256 key pair: (PKCS#8, uncompressed public point).
fn keypair() -> (Vec<u8>, Vec<u8>) {
    let rng = SystemRandom::new();
    let pkcs8 =
        EcdsaKeyPair::generate_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
    let pair = EcdsaKeyPair::from_pkcs8(
        &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        pkcs8.as_ref(),
        &rng,
    )
    .unwrap();
    (pkcs8.as_ref().to_vec(), pair.public_key().as_ref().to_vec())
}

/// Whether `sig` is an ES256 signature, by `public`, over `payload` — the
/// JWS signing input `BASE64URL(protected) '.' BASE64URL(payload)` (RFC 7515
/// §5.2) built here, not by the code under test.
fn signs(sig: &AgentCardSignature, public: &[u8], payload: &[u8]) -> bool {
    let input = format!("{}.{}", sig.protected, URL_SAFE_NO_PAD.encode(payload));
    let raw = URL_SAFE_NO_PAD.decode(&sig.signature).expect("base64url");
    UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, public)
        .verify(input.as_bytes(), &raw)
        .is_ok()
}

/// CARD-SIGN-001: the bytes signed are the card's RFC 8785 canonical form —
/// not its serde serialization — and so the signature holds however the
/// card is later written.
#[test]
fn card_sign_001_the_signed_payload_is_the_jcs_canonical_card() {
    let card = card();
    let (pkcs8, public) = keypair();
    let sig = sign_card_es256(&card, &pkcs8, "k1").unwrap();

    let canonical = canonicalize_card(&card).unwrap();
    assert!(
        signs(&sig, &public, &canonical),
        "the signature is over the canonical bytes"
    );

    // Those bytes are JCS: canonicalizing them again changes nothing (RFC
    // 8785's output is a fixed point: sorted keys, no insignificant
    // whitespace, ECMAScript number and string forms), and they are not
    // what a plain serializer writes, which the signature does not cover.
    let parsed: Value = serde_json::from_slice(&canonical).unwrap();
    assert_eq!(canonicalize(&parsed).unwrap(), canonical);
    let plain = serde_json::to_vec(&card).unwrap();
    assert_ne!(plain, canonical, "the test must tell the two apart");
    assert!(
        !signs(&sig, &public, &plain),
        "the plain serialization is not what was signed"
    );
    let text = String::from_utf8(canonical.clone()).unwrap();
    assert!(
        text.starts_with("{\"capabilities\":"),
        "sorted keys: {text}"
    );
    assert!(!text.contains(": ") && !text.contains('\n'), "{text}");
    assert!(
        text.contains("Ünïcode ✓"),
        "JCS writes non-ASCII as itself: {text}"
    );

    // The card as a peer receives it — pretty-printed, signature attached —
    // canonicalizes to the same bytes, so it verifies.
    let mut served = serde_json::to_value(&card).unwrap();
    served["signatures"] = json!([sig]);
    let pretty: Value =
        serde_json::from_str(&serde_json::to_string_pretty(&served).unwrap()).unwrap();
    assert_eq!(canonicalize_card_json(&pretty).unwrap(), canonical);
    let key = VerifyingKey::from_bytes("ES256", &public).unwrap();
    let jwks = Jwks::new(vec![Jwk::from_verifying_key(&key, Some("k1"))]);
    verify_card_json_with_jwks(&pretty, &jwks).expect("a rewritten card still verifies");
}

/// CARD-SIGN-002: `signatures` is not in the signed payload, so attaching
/// the signature — or a second one, for key rotation — leaves it valid.
#[test]
fn card_sign_002_signatures_are_not_part_of_the_signed_payload() {
    let (pkcs8, public) = keypair();
    let (pkcs8_2, public_2) = keypair();
    let unsigned = card();
    let sig = sign_card_es256(&unsigned, &pkcs8, "k1").unwrap();

    let mut served = card();
    served.signatures = Some(vec![sig.clone()]);
    let payload = canonicalize_card(&served).unwrap();
    assert_eq!(
        payload,
        canonicalize_card(&unsigned).unwrap(),
        "a populated `signatures` adds nothing to the payload"
    );
    let parsed: Value = serde_json::from_slice(&payload).unwrap();
    assert!(parsed.get("signatures").is_none(), "{parsed}");
    assert!(signs(&sig, &public, &payload));

    // If `signatures` were signed, signing the served card would give a
    // different payload and this second signature could not verify against
    // the bytes the first one covers.
    let sig_2 = sign_card_es256(&served, &pkcs8_2, "k2").unwrap();
    assert!(signs(&sig_2, &public_2, &payload));
    served.signatures = Some(vec![sig.clone(), sig_2.clone()]);
    verify_agent_card(&served, &sig, &public).expect("the first signature still verifies");
    verify_agent_card(&served, &sig_2, &public_2).expect("so does the second");
}

/// CARD-SIGN-003 (§8.4.2): "The JWS protected header MUST include alg and
/// kid" — from every signer that is not deprecated, for both algorithms,
/// with and without `jku`. (The two kid-optional signers are deprecated
/// since 0.15.0 for exactly this reason.)
#[test]
fn card_sign_003_every_signer_writes_alg_and_kid() {
    let card = card();
    let (es, _) = keypair();
    let ed = signature::Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let ed = ed.as_ref();
    let jku = "https://agent.example/jwks.json";
    let signed = [
        (
            "ES256",
            sign_card_es256(&card, &es, "es-k").unwrap(),
            "es-k",
        ),
        (
            "ES256",
            sign_card_es256_with_jku(&card, &es, "es-j", jku).unwrap(),
            "es-j",
        ),
        (
            "EdDSA",
            sign_card_ed25519(&card, ed, "ed-k").unwrap(),
            "ed-k",
        ),
        (
            "EdDSA",
            sign_card_ed25519_with_jku(&card, ed, "ed-j", jku).unwrap(),
            "ed-j",
        ),
    ];
    for (alg, sig, kid) in signed {
        let header: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&sig.protected).unwrap()).unwrap();
        assert_eq!(header["alg"], alg, "{header}");
        assert_eq!(header["kid"], kid, "{header}");
        assert!(
            sig.header.is_none(),
            "alg and kid are protected, not unprotected"
        );
    }
}
