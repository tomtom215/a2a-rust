// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Rotating the checkpoint key: a chain signed under the old key and then the
//! new one still verifies, given the old public key.

use std::sync::Arc;

use a2a_protocol_types::audit::{CheckpointSigner, SigningAlg};
use ring::rand::SystemRandom;
use ring::signature::Ed25519KeyPair;

use super::rec;
use crate::audit::{AuditLog, InMemoryAuditStore};

fn signer(kid: &str) -> CheckpointSigner {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    CheckpointSigner::from_pkcs8(SigningAlg::EdDsa, kid, pkcs8.as_ref()).unwrap()
}

#[tokio::test]
async fn a_rotated_checkpoint_key_keeps_the_older_checkpoints_verifiable() {
    let store = Arc::new(InMemoryAuditStore::new());
    let old = signer("audit-2026-10");
    let old_key = old.public_key();
    let before = AuditLog::new(store.clone()).with_signer(old, 0);
    before.append(rec("acme", "t1")).await.unwrap();
    before.checkpoint("acme").await.unwrap();

    // The process restarts with the new key.
    let after = AuditLog::new(store.clone()).with_signer(signer("audit-2026-11"), 0);
    after.append(rec("acme", "t2")).await.unwrap();
    after.checkpoint("acme").await.unwrap();
    let unaware = after.verify("acme").await.unwrap();
    assert!(
        !unaware.is_intact(),
        "a checkpoint under a key the log was not given is not verified"
    );

    let after = after.with_retired_key(old_key);
    let report = after.verify("acme").await.unwrap();
    assert!(report.is_intact(), "{:?}", report.failure);
    assert_eq!(report.records, 2);
    let kids: Vec<String> = after.trusted_keys().into_iter().map(|k| k.kid).collect();
    assert_eq!(kids, ["audit-2026-11", "audit-2026-10"]);
}
