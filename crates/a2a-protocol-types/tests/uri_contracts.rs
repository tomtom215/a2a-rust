// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Each extension URI this SDK defines, pinned to its wire contract.
//!
//! A2A v1.0.1 binds this SDK as the author of its extensions: "A new URI
//! MUST be created for breaking changes to an extension" (§4.6.3). The URI
//! constants all end in `/v1`, and until now nothing tied what `/v1` means to
//! anything a machine checks: RELEASING.md asked a person to notice.
//!
//! Each test below builds the contract document for one URI from the code
//! itself (the metadata key, the field and variant names a peer reads, a
//! fully populated sample of every value as it serializes, the tokens and
//! what they mean) and compares it with
//! `tests/golden/uri_contracts/<name>.json`. A change to the contract with
//! the URI unchanged fails, and so does a changed URI with the old snapshot.
//! Whether a change is breaking stays a person's call; the failure says how
//! to record either answer:
//!
//! ```text
//! A2A_BLESS_URI_CONTRACTS=1 cargo test -p a2a-protocol-types --all-features --test uri_contracts
//! ```
//!
//! The snapshots do not depend on features. What only the `signing` feature
//! can observe (the JWS header and algorithms, the digest format) is written
//! here as a constant that goes into the snapshot, and checked against the
//! code when that feature is on, which is how CI runs these tests.
//!
//! The WebSocket binding's twin (§5.8) is the server crate's
//! `tests/uri_contract_websocket.rs`.

#[path = "uri_contracts/golden.rs"]
mod golden;
#[path = "uri_contracts/probe.rs"]
mod probe;

use std::time::Duration;

use a2a_protocol_types::approval::{
    APPROVAL_EXTENSION_URI, APPROVAL_METADATA_KEY, ApprovalDecision, ApprovalRequest, Decision,
};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::ErrorCode;
use a2a_protocol_types::failure::{
    FAILURE_EXTENSION_URI, FAILURE_METADATA_KEY, FailureClass, class_of, set_class,
};
use a2a_protocol_types::idempotency::{
    IDEMPOTENCY_EXTENSION_URI, IDEMPOTENCY_METADATA_KEY, key_of, set_key, validate_key,
};
use a2a_protocol_types::lease::{LEASE_EXTENSION_URI, LEASE_METADATA_KEY, Lease};
use a2a_protocol_types::message::{Message, Part};
use a2a_protocol_types::provenance::{
    ContentSignature, PROVENANCE_EXTENSION_URI, PROVENANCE_METADATA_KEY, Provenance,
    mark_ai_generated, provenance_of,
};
use serde_json::{Value, json};

use probe::wire_names;

/// What a carrier's `extensions` and `metadata` look like on the wire once
/// an extension has written to it.
fn carried<T: serde::Serialize>(content: &T) -> Value {
    let wire = serde_json::to_value(content).expect("content serializes");
    json!({ "extensions": wire["extensions"], "metadata": wire["metadata"] })
}

#[test]
fn failure_extension_contract() {
    let mut status = Message::agent_text("m1", "upstream refused");
    set_class(&mut status, FailureClass::PolicyRefusal);

    let classes: serde_json::Map<String, Value> = FailureClass::ALL
        .iter()
        .map(|c| {
            (
                c.as_str().to_owned(),
                json!({ "retryable": c.is_retryable(), "needsHuman": c.needs_human() }),
            )
        })
        .collect();
    // The class a failed task's status carries when its executor returned a
    // bare error: which codes exist is the spec's, the class each gets is
    // this extension's.
    let from_codes: serde_json::Map<String, Value> = (-32768..=-32000)
        .filter_map(|n| ErrorCode::try_from(n).ok())
        .map(|code| {
            let class = FailureClass::from(code);
            (code.as_i32().to_string(), json!(class.as_str()))
        })
        .collect();
    // An unrecognised token from a newer peer reads as a class, not absence.
    let mut newer = Message::agent_text("m2", "x");
    newer.metadata = Some(json!({ FAILURE_METADATA_KEY: "a-class-from-v2" }));

    golden::check(
        "failure",
        FAILURE_EXTENSION_URI,
        json!({
            "metadataKey": FAILURE_METADATA_KEY,
            "carrier": "Message.metadata of the failed task's status message",
            "statusMessage": carried(&status),
            "classes": classes,
            "unrecognisedClassReadsAs": class_of(&newer).map(FailureClass::as_str),
            "classOfAnExecutorErrorByCode": from_codes,
        }),
    );
}

#[test]
fn provenance_extension_contract() {
    let mut message = Message::agent_text("m1", "Your refund was issued.");
    mark_ai_generated(&mut message, Some("billing-agent/2.3"));
    let mut artifact = Artifact::new("a1", vec![Part::text("report")]);
    mark_ai_generated(&mut artifact, Some("billing-agent/2.3"));

    let signature: ContentSignature = serde_json::from_value(
        json!({ "protected": "eyJhbGciOiJFZERTQSIsImtpZCI6ImsxIn0", "signature": "c2ln" }),
    )
    .expect("a signature from its documented fields");
    let mut full = provenance_of(&message)
        .expect("readable")
        .expect("marked above");
    full.signature = Some(signature);

    // A marker with nothing in it reads as "not AI-generated".
    let mut empty = Message::agent_text("m2", "x");
    empty.metadata = Some(json!({ PROVENANCE_METADATA_KEY: {} }));
    let read_empty = provenance_of(&empty).expect("an empty object is provenance");

    golden::check(
        "provenance",
        PROVENANCE_EXTENSION_URI,
        json!({
            "metadataKey": PROVENANCE_METADATA_KEY,
            "fields": {
                "Provenance": wire_names::<Provenance>(),
                "ContentSignature": wire_names::<ContentSignature>(),
            },
            "value": serde_json::to_value(&full).expect("provenance serializes"),
            "emptyObjectReadsAs": serde_json::to_value(read_empty).expect("serializes"),
            "markedMessage": carried(&message),
            "markedArtifact": carried(&artifact),
            "signature": signature_contract(),
        }),
    );
}

/// The signature scheme, which only the `signing` feature can exercise:
/// checked against the code by `the_signature_contract_is_what_the_code_signs`.
fn signature_contract() -> Value {
    json!({
        "form": "JWS (RFC 7515), detached payload; protected and signature are base64url without padding",
        "protectedHeaderKeys": ["alg", "kid"],
        "algorithms": ["ES256", "EdDSA"],
        "signingInput": "protected + \".\" + base64url(RFC 8785 JCS of the whole content with provenance.signature removed)",
    })
}

#[cfg(feature = "signing")]
#[test]
fn the_signature_contract_is_what_the_code_signs() {
    use a2a_protocol_types::provenance::{ContentSigner, sign_content};
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use ring::signature::{ED25519, Ed25519KeyPair, KeyPair as _, UnparsedPublicKey};

    let contract = signature_contract();
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).expect("a key");
    let public = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())
        .expect("the key parses")
        .public_key()
        .as_ref()
        .to_vec();
    let mut message = Message::agent_text("m1", "signed");
    mark_ai_generated(&mut message, Some("g"));
    let signer = ContentSigner::ed25519("k1", pkcs8.as_ref()).expect("a signer");
    sign_content(&mut message, &signer).expect("signs");

    let sig = provenance_of(&message)
        .expect("readable")
        .and_then(|p| p.signature)
        .expect("signed");
    let header: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&sig.protected).expect("base64url"))
            .expect("a JSON header");
    let mut keys: Vec<&str> = header
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(json!(keys), contract["protectedHeaderKeys"]);
    assert_eq!(header["alg"], contract["algorithms"][1]);
    assert_eq!(header["kid"], "k1");
    let es256 = ring::signature::EcdsaKeyPair::generate_pkcs8(
        &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        &rng,
    )
    .expect("a key");
    let es = ContentSigner::es256("k2", es256.as_ref()).expect("a signer");
    assert_eq!(json!([es.alg(), signer.alg()]), contract["algorithms"]);

    // The signing input is the one the contract states.
    let mut unsigned = serde_json::to_value(&message).expect("serializes");
    unsigned["metadata"][PROVENANCE_METADATA_KEY]
        .as_object_mut()
        .expect("provenance is an object")
        .remove("signature");
    let jcs = a2a_protocol_types::signing::canonicalize(&unsigned).expect("canonical");
    let input = format!("{}.{}", sig.protected, URL_SAFE_NO_PAD.encode(jcs));
    UnparsedPublicKey::new(&ED25519, &public)
        .verify(
            input.as_bytes(),
            &URL_SAFE_NO_PAD.decode(&sig.signature).expect("base64url"),
        )
        .expect("the signature covers the stated signing input");
}

#[test]
fn idempotency_extension_contract() {
    const KEY: &str = "8f14e45fceea167a5a36dedd4bea2543";
    let mut message = Message::user_text("m1", "book it");
    set_key(&mut message, KEY).expect("a valid key");

    let accepted: String = (0u8..=0x7f)
        .map(char::from)
        .filter(|c| validate_key(&format!("{}{c}", "a".repeat(15))).is_ok())
        .collect();
    let lengths: Vec<usize> = (0..=300)
        .filter(|n| validate_key(&"a".repeat(*n)).is_ok())
        .collect();
    let mut undeclared = Message::user_text("m2", "x");
    undeclared.metadata = Some(json!({ IDEMPOTENCY_METADATA_KEY: KEY }));

    golden::check(
        "idempotency",
        IDEMPOTENCY_EXTENSION_URI,
        json!({
            "metadataKey": IDEMPOTENCY_METADATA_KEY,
            "carrier": "Message.metadata of a SendMessage or SendStreamingMessage request",
            "message": carried(&message),
            "key": {
                "type": "string",
                "minLength": lengths.first(),
                "maxLength": lengths.last(),
                "acceptedAsciiCharacters": accepted,
                "nonAsciiAccepted": validate_key(&format!("{}\u{e9}", "a".repeat(15))).is_ok(),
            },
            "honouredWithoutDeclaringTheUri": key_of(&undeclared) == Ok(Some(KEY)),
        }),
    );
}

#[test]
fn lease_extension_contract() {
    let mut message = Message::user_text("m1", "summarise these");
    Lease::new(Duration::from_millis(30_000)).attach(&mut message);

    golden::check(
        "lease",
        LEASE_EXTENSION_URI,
        json!({
            "metadataKey": LEASE_METADATA_KEY,
            "fields": { "Lease": wire_names::<Lease>() },
            "grant": carried(&message),
            "renewal": {
                "method": "GetTask",
                "activatedBy": format!("the {} service parameter naming the URI", a2a_protocol_types::A2A_EXTENSIONS_HEADER),
                "effect": "the task's deadline moves to now + ttlMs",
            },
            "lapse": "the task is stopped and ends TASK_STATE_CANCELED",
        }),
    );
}

#[test]
fn approval_extension_contract() {
    let mut request = ApprovalRequest::new("req-1", "Refund EUR 40 to order 1182", DIGEST_SAMPLE);
    request.requested_by = Some("alice".to_owned());
    let mut ask = Message::agent_text("m1", "Approve this refund?");
    request.attach(&mut ask);
    let mut answer = Message::user_text("m2", "yes");
    ApprovalDecision::approve(&request)
        .with_comment("checked the order")
        .attach(&mut answer);

    golden::check(
        "approval",
        APPROVAL_EXTENSION_URI,
        json!({
            "metadataKey": APPROVAL_METADATA_KEY,
            "fields": {
                "ApprovalRequest": wire_names::<ApprovalRequest>(),
                "ApprovalDecision": wire_names::<ApprovalDecision>(),
                "Decision": wire_names::<Decision>(),
            },
            "request": { "carrier": "status message of the input-required task", "message": carried(&ask) },
            "decision": { "carrier": "the message that continues the task", "message": carried(&answer) },
            "decisionTokens": [Decision::Approve, Decision::Deny],
            "digest": DIGEST_FORMAT,
        }),
    );
}

const DIGEST_SAMPLE: &str =
    "sha256:0000000000000000000000000000000000000000000000000000000000000000";

/// Checked against `action_digest` by `the_digest_format_is_what_the_code_writes`.
const DIGEST_FORMAT: &str = "sha256:<64 lowercase hex> over the RFC 8785 JCS of the action";

#[cfg(feature = "signing")]
#[test]
fn the_digest_format_is_what_the_code_writes() {
    let digest =
        a2a_protocol_types::approval::action_digest(&json!({"refund": 40})).expect("a digest");
    let hex = digest.strip_prefix("sha256:").expect("the sha256: prefix");
    assert_eq!(hex.len(), 64, "{digest}");
    assert!(
        hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "{digest}"
    );
    assert_eq!(DIGEST_SAMPLE.len(), digest.len());
}
