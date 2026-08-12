// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Agent-card authenticity (spec §5.6 — signed agent cards).

use super::Check;

const LABEL: &str = "Agent card signing (sign, verify, detect tampering)";

/// Signs a card, verifies it, then tampers with it and requires verification to
/// fail.
///
/// The third step is the one that matters. Signing and verifying a card you
/// just signed passes against a `verify` that returns `Ok(())` unconditionally,
/// and against one that hashes the wrong bytes — both would be catastrophic in
/// a deployment that trusts card signatures to decide which agents to talk to.
/// Only presenting a *modified* card and requiring a rejection distinguishes a
/// real signature from a decorative one.
#[cfg(feature = "signing")]
pub(super) fn card_signing() -> Check {
    use a2a_protocol_types::signing::{sign_agent_card, verify_agent_card};
    use ring::rand::SystemRandom;
    use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};

    let rng = SystemRandom::new();
    let pkcs8 = match EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng) {
        Ok(document) => document,
        Err(e) => return Check::fail(LABEL, format!("generating a P-256 key: {e}")),
    };
    let key_pair =
        match EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng) {
            Ok(pair) => pair,
            Err(e) => return Check::fail(LABEL, format!("loading the key: {e}")),
        };
    let public_key = key_pair.public_key().as_ref().to_vec();

    let card = super::plain_card("http://127.0.0.1:9200", "Triage Agent");
    let signature = match sign_agent_card(&card, pkcs8.as_ref(), Some("incident-demo-key")) {
        Ok(signature) => signature,
        Err(e) => return Check::fail(LABEL, format!("signing: {e}")),
    };
    if let Err(e) = verify_agent_card(&card, &signature, &public_key) {
        return Check::fail(
            LABEL,
            format!("a freshly signed card failed to verify: {e}"),
        );
    }

    // Tamper with the field an attacker would rewrite: the address callers
    // dial. Note this is `supported_interfaces[0].url`, not the deprecated
    // top-level `AgentCard::url` — that one is `#[serde(skip_serializing)]`
    // because A2A v1.0 removed it, so it is absent from the canonical bytes and
    // rewriting it correctly changes nothing. Tampering there would report a
    // signature failure that is really a serialization detail.
    let Some(interface) = card.supported_interfaces.first() else {
        return Check::fail(
            LABEL,
            "the demo card advertises no interface to tamper with",
        );
    };
    let mut tampered = card.clone();
    tampered.supported_interfaces[0].url = format!("{}@impostor.invalid", interface.url);
    match verify_agent_card(&tampered, &signature, &public_key) {
        Err(_) => Check::pass(
            LABEL,
            "ES256 signature verified, and a redirected interface url was rejected",
        ),
        Ok(()) => Check::fail(
            LABEL,
            "a card whose interface url was rewritten still VERIFIED — callers could be redirected",
        ),
    }
}

#[cfg(not(feature = "signing"))]
pub(super) fn card_signing() -> Check {
    Check::skipped(LABEL, "signing")
}
