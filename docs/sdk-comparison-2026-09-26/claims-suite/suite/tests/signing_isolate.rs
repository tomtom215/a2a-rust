// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Isolates the signing verification failure seen in card.rs. No network needed for (a)-(c);
//! (d) goes over the wire. Ports 7550-7599.
use std::sync::Arc;
use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::types::signing::{canonicalize_card, sign_agent_card, verify_agent_card};
use a2a_protocol_sdk::types::extensions::AgentExtension;
use claims_suite::common::*;

fn base(desc: &str, exts: bool) -> AgentCard {
    let mut c = AgentCard::new("card-agent", "1.0.0", AgentInterface::jsonrpc("http://127.0.0.1:7599"));
    c.description = desc.into();
    if exts {
        c.capabilities.extensions = Some(vec![
            AgentExtension::new(a2a_protocol_sdk::types::idempotency::IDEMPOTENCY_EXTENSION_URI),
            AgentExtension::new(a2a_protocol_sdk::types::failure::FAILURE_EXTENSION_URI),
        ]);
    }
    c
}

fn check(label: &str, desc: &str, exts: bool, kp: &rcgen::KeyPair) -> bool {
    let mut c = base(desc, exts);
    let sig = sign_agent_card(&c, &kp.serialize_der(), Some("k")).unwrap();
    c.signatures = Some(vec![sig.clone()]);
    let local = verify_agent_card(&c, &sig, &kp.public_key_der()).is_ok();
    let json = serde_json::to_string(&c).unwrap();
    let back: AgentCard = serde_json::from_str(&json).unwrap();
    let rt = verify_agent_card(&back, &sig, &kp.public_key_der()).is_ok();
    let same = canonicalize_card(&c).unwrap() == canonicalize_card(&back).unwrap();
    println!("{label}: local_verify={local} serde_roundtrip_verify={rt} canonical_bytes_equal_after_roundtrip={same}");
    if !same {
        println!("  before: {}", String::from_utf8_lossy(&canonicalize_card(&c).unwrap()));
        println!("  after : {}", String::from_utf8_lossy(&canonicalize_card(&back).unwrap()));
    }
    local && rt
}

#[tokio::test]
async fn isolate() {
    let kp = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let a = check("ascii, no extensions", "v1", false, &kp);
    let b = check("ascii, declared extensions", "v1", true, &kp);
    let c = check("non-ascii desc", "Grüße 🚀 \"quoted\" 1e3", true, &kp);

    // Over the wire, ASCII card with declared extensions.
    let p = port_in(7550, 50);
    let mut card = base("v1", true);
    card.supported_interfaces[0].url = format!("http://127.0.0.1:{p}");
    let sig = sign_agent_card(&card, &kp.serialize_der(), Some("k")).unwrap();
    card.signatures = Some(vec![sig]);
    let signed_canon = canonicalize_card(&card).unwrap();
    let (agent, _) = CtlAgent::new();
    let h = Arc::new(RequestHandlerBuilder::new(agent).with_agent_card(card).build().unwrap());
    serve_with_addr(format!("127.0.0.1:{p}"), JsonRpcDispatcher::new(h)).await.unwrap();
    let raw = reqwest::get(format!("http://127.0.0.1:{p}/.well-known/agent-card.json")).await.unwrap().text().await.unwrap();
    println!("served JSON: {raw}");
    let fetched = resolve_agent_card(&format!("http://127.0.0.1:{p}")).await.unwrap();
    let fetched_canon = canonicalize_card(&fetched).unwrap();
    println!("signed canonical : {}", String::from_utf8_lossy(&signed_canon));
    println!("fetched canonical: {}", String::from_utf8_lossy(&fetched_canon));
    let d = verify_agent_card(&fetched, &fetched.signatures.as_ref().unwrap()[0], &kp.public_key_der());
    println!("wire ascii verify: {d:?}");
    println!("RESULT a={a} b={b} c={c} wire={}", d.is_ok());
}
