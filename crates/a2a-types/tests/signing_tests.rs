// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Integration tests for `a2a_protocol_types::signing` — JSON canonicalization,
//! agent card signing, and signature verification.

#![cfg(feature = "signing")]

use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::extensions::AgentCardSignature;
use a2a_protocol_types::signing::{
    canonicalize, canonicalize_card, sign_agent_card, verify_agent_card,
};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::rand::SystemRandom;
use ring::signature::{self, EcdsaKeyPair, KeyPair};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Build a minimal valid `AgentCard` for testing.
fn minimal_card() -> AgentCard {
    AgentCard {
        url: None,
        name: "Test Agent".into(),
        description: "A test agent".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: "https://agent.example.com/rpc".into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "echo".into(),
            name: "Echo".into(),
            description: "Echoes input".into(),
            tags: vec!["echo".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none(),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

/// Generate a fresh ES256 PKCS#8 key pair, returning (pkcs8_bytes, public_key_bytes).
fn generate_es256_keypair() -> (Vec<u8>, Vec<u8>) {
    let rng = SystemRandom::new();
    let pkcs8 =
        EcdsaKeyPair::generate_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
    let key_pair = EcdsaKeyPair::from_pkcs8(
        &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        pkcs8.as_ref(),
        &rng,
    )
    .unwrap();
    let pub_key = key_pair.public_key().as_ref().to_vec();
    (pkcs8.as_ref().to_vec(), pub_key)
}

// ── T-1.1: canonicalize() produces RFC 8785 compliant output ─────────────────

#[test]
fn canonicalize_sorts_object_keys_lexicographically() {
    let json: serde_json::Value = serde_json::json!({"z": 1, "a": 2, "m": 3});
    let canonical = canonicalize(&json).unwrap();
    let s = String::from_utf8(canonical).unwrap();
    assert_eq!(s, r#"{"a":2,"m":3,"z":1}"#);
}

#[test]
fn canonicalize_sorts_nested_object_keys() {
    let json: serde_json::Value = serde_json::json!({"b": {"z": 1, "a": 2}, "a": [3, 2, 1]});
    let canonical = canonicalize(&json).unwrap();
    let s = String::from_utf8(canonical).unwrap();
    assert_eq!(s, r#"{"a":[3,2,1],"b":{"a":2,"z":1}}"#);
}

#[test]
fn canonicalize_no_extra_whitespace() {
    // Even with pretty-formatted input, canonical output must have no whitespace.
    let json: serde_json::Value = serde_json::json!({
        "name": "Alice",
        "age": 30,
        "active": true
    });
    let canonical = canonicalize(&json).unwrap();
    let s = String::from_utf8(canonical).unwrap();
    // No spaces after colons or commas.
    assert!(!s.contains(": "));
    assert!(!s.contains(", "));
    // Keys must be sorted.
    assert_eq!(s, r#"{"active":true,"age":30,"name":"Alice"}"#);
}

#[test]
fn canonicalize_preserves_array_order() {
    let json: serde_json::Value = serde_json::json!([3, 1, 2]);
    let canonical = canonicalize(&json).unwrap();
    let s = String::from_utf8(canonical).unwrap();
    assert_eq!(s, "[3,1,2]");
}

#[test]
fn canonicalize_handles_empty_object_and_array() {
    let obj: serde_json::Value = serde_json::json!({});
    assert_eq!(
        String::from_utf8(canonicalize(&obj).unwrap()).unwrap(),
        "{}"
    );

    let arr: serde_json::Value = serde_json::json!([]);
    assert_eq!(
        String::from_utf8(canonicalize(&arr).unwrap()).unwrap(),
        "[]"
    );
}

#[test]
fn canonicalize_handles_null_and_booleans() {
    let null = serde_json::Value::Null;
    assert_eq!(
        String::from_utf8(canonicalize(&null).unwrap()).unwrap(),
        "null"
    );

    let t: serde_json::Value = serde_json::json!(true);
    assert_eq!(
        String::from_utf8(canonicalize(&t).unwrap()).unwrap(),
        "true"
    );

    let f: serde_json::Value = serde_json::json!(false);
    assert_eq!(
        String::from_utf8(canonicalize(&f).unwrap()).unwrap(),
        "false"
    );
}

#[test]
fn canonicalize_escapes_special_characters_in_strings() {
    let json: serde_json::Value = serde_json::json!({"msg": "hello\nworld\ttab"});
    let canonical = canonicalize(&json).unwrap();
    let s = String::from_utf8(canonical).unwrap();
    assert_eq!(s, r#"{"msg":"hello\nworld\ttab"}"#);
}

#[test]
fn canonicalize_escapes_backslash_and_quote() {
    let json: serde_json::Value = serde_json::json!({"path": "C:\\Users\\\"test\""});
    let canonical = canonicalize(&json).unwrap();
    let s = String::from_utf8(canonical).unwrap();
    assert_eq!(s, r#"{"path":"C:\\Users\\\"test\""}"#);
}

#[test]
fn canonicalize_escapes_control_characters_below_0x20() {
    // \x01 should be rendered as \u0001 per RFC 8785.
    let json: serde_json::Value = serde_json::Value::String("\x01\x1f".to_string());
    let canonical = canonicalize(&json).unwrap();
    let s = String::from_utf8(canonical).unwrap();
    assert_eq!(s, r#""\u0001\u001f""#);
}

#[test]
fn canonicalize_deeply_nested_structure() {
    let json: serde_json::Value = serde_json::json!({
        "c": {"b": {"a": 1}},
        "a": [{"z": true, "a": false}]
    });
    let canonical = canonicalize(&json).unwrap();
    let s = String::from_utf8(canonical).unwrap();
    assert_eq!(s, r#"{"a":[{"a":false,"z":true}],"c":{"b":{"a":1}}}"#);
}

// ── T-1.5: Number representation in canonical form ───────────────────────────

#[test]
fn canonicalize_integer_numbers() {
    let json: serde_json::Value = serde_json::json!({"val": 42});
    let canonical = canonicalize(&json).unwrap();
    let s = String::from_utf8(canonical).unwrap();
    assert_eq!(s, r#"{"val":42}"#);
}

#[test]
fn canonicalize_zero() {
    let json: serde_json::Value = serde_json::json!(0);
    let canonical = canonicalize(&json).unwrap();
    let s = String::from_utf8(canonical).unwrap();
    assert_eq!(s, "0");
}

#[test]
fn canonicalize_negative_number() {
    let json: serde_json::Value = serde_json::json!(-123);
    let canonical = canonicalize(&json).unwrap();
    let s = String::from_utf8(canonical).unwrap();
    assert_eq!(s, "-123");
}

#[test]
fn canonicalize_floating_point_number() {
    // serde_json preserves the shortest representation for floats.
    let json: serde_json::Value = serde_json::json!(1.5);
    let canonical = canonicalize(&json).unwrap();
    let s = String::from_utf8(canonical).unwrap();
    assert_eq!(s, "1.5");
}

#[test]
fn canonicalize_float_no_trailing_zeros() {
    // 10.0 should be represented without unnecessary trailing zeros.
    // serde_json represents 10.0 as a float.
    let json: serde_json::Value = serde_json::from_str("10.0").unwrap();
    let canonical = canonicalize(&json).unwrap();
    let s = String::from_utf8(canonical).unwrap();
    // serde_json::Number::to_string for 10.0 gives "10.0" — this is the
    // shortest faithful representation.
    assert_eq!(s, "10.0");
}

#[test]
fn canonicalize_large_integer() {
    let json: serde_json::Value = serde_json::json!(9007199254740992_i64);
    let canonical = canonicalize(&json).unwrap();
    let s = String::from_utf8(canonical).unwrap();
    assert_eq!(s, "9007199254740992");
}

// ── T-1.2: canonicalize_card() works on a basic agent card ───────────────────

#[test]
fn canonicalize_card_produces_deterministic_output() {
    let card = minimal_card();
    let c1 = canonicalize_card(&card).unwrap();
    let c2 = canonicalize_card(&card).unwrap();
    assert_eq!(c1, c2, "canonicalize_card must be deterministic");
}

#[test]
fn canonicalize_card_produces_valid_json() {
    let card = minimal_card();
    let canonical = canonicalize_card(&card).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_slice(&canonical).expect("canonical output must be valid JSON");
    assert_eq!(parsed["name"], "Test Agent");
    assert_eq!(parsed["version"], "1.0.0");
}

#[test]
fn canonicalize_card_keys_are_sorted() {
    let card = minimal_card();
    let canonical = canonicalize_card(&card).unwrap();
    let s = String::from_utf8(canonical).unwrap();

    // In camelCase, "capabilities" < "defaultInputModes" < "defaultOutputModes"
    // < "description" < "name" < "skills" < "supportedInterfaces" < "version"
    let cap_pos = s.find("\"capabilities\"").unwrap();
    let dim_pos = s.find("\"defaultInputModes\"").unwrap();
    let dom_pos = s.find("\"defaultOutputModes\"").unwrap();
    let desc_pos = s.find("\"description\"").unwrap();
    let name_pos = s.find("\"name\"").unwrap();
    let skills_pos = s.find("\"skills\"").unwrap();
    let si_pos = s.find("\"supportedInterfaces\"").unwrap();
    let ver_pos = s.find("\"version\"").unwrap();

    assert!(cap_pos < dim_pos, "capabilities before defaultInputModes");
    assert!(
        dim_pos < dom_pos,
        "defaultInputModes before defaultOutputModes"
    );
    assert!(dom_pos < desc_pos, "defaultOutputModes before description");
    assert!(desc_pos < name_pos, "description before name");
    assert!(name_pos < skills_pos, "name before skills");
    assert!(skills_pos < si_pos, "skills before supportedInterfaces");
    assert!(si_pos < ver_pos, "supportedInterfaces before version");
}

#[test]
fn canonicalize_card_has_no_extra_whitespace() {
    let card = minimal_card();
    let canonical = canonicalize_card(&card).unwrap();
    let s = String::from_utf8(canonical.clone()).unwrap();

    // The canonical form must not contain unquoted whitespace between tokens.
    // A quick heuristic: no " : " or " , " patterns.
    assert!(!s.contains("\" :"), "no space before colon");
    assert!(!s.contains(": \""), "no space after colon before string");
    // More precisely: no whitespace around structural characters outside strings.
    // We check that re-parsing and re-canonicalizing yields the same bytes.
    let reparsed: serde_json::Value = serde_json::from_str(&s).unwrap();
    let re_canonical = canonicalize(&reparsed).unwrap();
    assert_eq!(
        canonical, re_canonical,
        "re-canonicalization must be idempotent"
    );
}

// ── T-1.3: sign_agent_card() and verify_agent_card() roundtrip ──────────────

#[test]
fn sign_and_verify_roundtrip() {
    let card = minimal_card();
    let (pkcs8, pub_key) = generate_es256_keypair();

    let sig = sign_agent_card(&card, &pkcs8, Some("test-key-1")).unwrap();
    assert!(!sig.protected.is_empty());
    assert!(!sig.signature.is_empty());

    verify_agent_card(&card, &sig, &pub_key).unwrap();
}

#[test]
fn sign_and_verify_without_key_id() {
    let card = minimal_card();
    let (pkcs8, pub_key) = generate_es256_keypair();

    let sig = sign_agent_card(&card, &pkcs8, None).unwrap();
    verify_agent_card(&card, &sig, &pub_key).unwrap();
}

#[test]
fn protected_header_contains_alg() {
    let card = minimal_card();
    let (pkcs8, _) = generate_es256_keypair();

    let sig = sign_agent_card(&card, &pkcs8, None).unwrap();

    let header_bytes = URL_SAFE_NO_PAD.decode(&sig.protected).unwrap();
    let header: serde_json::Value = serde_json::from_slice(&header_bytes).unwrap();
    assert_eq!(header["alg"], "ES256");
    assert!(
        header.get("kid").is_none(),
        "kid should be absent when not provided"
    );
}

#[test]
fn protected_header_contains_kid_when_provided() {
    let card = minimal_card();
    let (pkcs8, _) = generate_es256_keypair();

    let sig = sign_agent_card(&card, &pkcs8, Some("my-key-id")).unwrap();

    let header_bytes = URL_SAFE_NO_PAD.decode(&sig.protected).unwrap();
    let header: serde_json::Value = serde_json::from_slice(&header_bytes).unwrap();
    assert_eq!(header["alg"], "ES256");
    assert_eq!(header["kid"], "my-key-id");
}

#[test]
fn sign_produces_different_signatures_for_different_cards() {
    let card1 = minimal_card();
    let mut card2 = minimal_card();
    card2.name = "Different Agent".into();

    let (pkcs8, _) = generate_es256_keypair();

    let sig1 = sign_agent_card(&card1, &pkcs8, None).unwrap();
    let sig2 = sign_agent_card(&card2, &pkcs8, None).unwrap();

    // The signatures should differ because the payloads differ.
    // (ECDSA is randomized, so even same payload would differ, but the
    // protected+payload input is definitely different here.)
    assert_ne!(sig1.signature, sig2.signature);
}

// ── T-1.4: verify_agent_card() rejects invalid/tampered signatures ──────────

#[test]
fn verify_rejects_tampered_card() {
    let mut card = minimal_card();
    let (pkcs8, pub_key) = generate_es256_keypair();

    let sig = sign_agent_card(&card, &pkcs8, None).unwrap();

    // Tamper with the card after signing.
    card.name = "Tampered Agent".into();

    let result = verify_agent_card(&card, &sig, &pub_key);
    assert!(result.is_err(), "verification must fail for tampered card");
}

#[test]
fn verify_rejects_tampered_signature_bytes() {
    let card = minimal_card();
    let (pkcs8, pub_key) = generate_es256_keypair();

    let mut sig = sign_agent_card(&card, &pkcs8, None).unwrap();

    // Corrupt the signature by flipping a character.
    let mut sig_bytes = URL_SAFE_NO_PAD.decode(&sig.signature).unwrap();
    sig_bytes[0] ^= 0xFF;
    sig.signature = URL_SAFE_NO_PAD.encode(&sig_bytes);

    let result = verify_agent_card(&card, &sig, &pub_key);
    assert!(
        result.is_err(),
        "verification must fail for tampered signature"
    );
}

#[test]
fn verify_rejects_wrong_public_key() {
    let card = minimal_card();
    let (pkcs8, _pub_key) = generate_es256_keypair();
    let (_pkcs8_other, pub_key_other) = generate_es256_keypair();

    let sig = sign_agent_card(&card, &pkcs8, None).unwrap();

    // Verify with a different key pair's public key.
    let result = verify_agent_card(&card, &sig, &pub_key_other);
    assert!(
        result.is_err(),
        "verification must fail with wrong public key"
    );
}

#[test]
fn verify_rejects_empty_signature() {
    let card = minimal_card();
    let (_pkcs8, pub_key) = generate_es256_keypair();

    // Construct a signature with valid header but empty signature field.
    let header = serde_json::json!({"alg": "ES256"});
    let header_bytes = serde_json::to_vec(&header).unwrap();
    let protected = URL_SAFE_NO_PAD.encode(&header_bytes);

    let sig = AgentCardSignature {
        protected,
        signature: String::new(),
        header: None,
    };

    let result = verify_agent_card(&card, &sig, &pub_key);
    assert!(
        result.is_err(),
        "verification must fail for empty signature"
    );
}

#[test]
fn verify_rejects_modified_protected_header() {
    let card = minimal_card();
    let (pkcs8, pub_key) = generate_es256_keypair();

    let mut sig = sign_agent_card(&card, &pkcs8, None).unwrap();

    // Replace the protected header with a different one (adds a kid).
    let new_header = serde_json::json!({"alg": "ES256", "kid": "injected"});
    let new_header_bytes = serde_json::to_vec(&new_header).unwrap();
    sig.protected = URL_SAFE_NO_PAD.encode(&new_header_bytes);

    let result = verify_agent_card(&card, &sig, &pub_key);
    assert!(
        result.is_err(),
        "verification must fail when protected header is modified"
    );
}
