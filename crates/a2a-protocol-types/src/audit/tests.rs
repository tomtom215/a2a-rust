// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, Ed25519KeyPair};

use super::*;

fn es256_signer(kid: &str) -> CheckpointSigner {
    let pkcs8 =
        EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
            .unwrap();
    CheckpointSigner::from_pkcs8(SigningAlg::Es256, kid, pkcs8.as_ref()).unwrap()
}

fn ed25519_signer(kid: &str) -> CheckpointSigner {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    CheckpointSigner::from_pkcs8(SigningAlg::EdDsa, kid, pkcs8.as_ref()).unwrap()
}

/// A sealed chain of `n` records, `seq` 1..=n.
fn chain(n: u64) -> Vec<AuditRecord> {
    let mut out: Vec<AuditRecord> = Vec::new();
    for seq in 1..=n {
        let mut r = AuditRecord::new("acme", kind::CALL);
        r.method = Some("GetTask".to_owned());
        r.task_id = Some(format!("t-{seq}"));
        let prev = out.last().map(|p| p.hash.clone());
        r.seal(seq, prev, format!("2026-10-08T12:00:{seq:02}.000Z"))
            .unwrap();
        out.push(r);
    }
    out
}

fn checkpoint(signer: &CheckpointSigner, records: &[AuditRecord], seq: u64) -> Checkpoint {
    let r = records.iter().find(|r| r.seq == seq).unwrap();
    let mut c = Checkpoint::new("checkpoint", "acme", seq, r.hash.clone(), r.time.clone());
    signer.sign(&mut c).unwrap();
    c
}

// ── Known answers, computed outside this crate ───────────────────────────────

#[test]
fn digest_bytes_matches_the_fips_180_test_vector() {
    // FIPS 180-2 Appendix B.1: SHA-256("abc").
    assert_eq!(
        digest_bytes(b"abc"),
        "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

/// The record hash agrees with an independent implementation: Python's
/// `json.dumps(sort_keys=True, separators=(",", ":"))`, which is RFC 8785 for
/// this all-ASCII, integer-only record, and `hashlib.sha256`.
#[test]
fn record_hash_matches_an_independent_implementation() {
    let mut r = AuditRecord::new("acme", kind::CALL);
    r.actor = Some(Actor::new("alice", Some("jwt".to_owned())));
    r.method = Some("SendMessage".to_owned());
    r.outcome = Some(Outcome::ok());
    r.task_id = Some("t-1".to_owned());
    r.digests.insert("message".to_owned(), digest_bytes(b"x"));
    r.seal(1, None, "2026-10-08T12:00:00.000Z".to_owned())
        .unwrap();
    assert_eq!(
        r.hash,
        "sha256:ee0380f398876301cf4dfb6d01ea80064f57017db708b4fd7eacb2d71c946860"
    );
}

// ── Sealing ──────────────────────────────────────────────────────────────────

#[test]
fn seal_refuses_positions_the_chain_cannot_hold() {
    let mut r = AuditRecord::new("", kind::CALL);
    assert!(r.seal(0, None, String::new()).is_err());
    assert!(
        r.seal(MAX_SEQ + 1, Some("sha256:x".into()), String::new())
            .is_err()
    );
    assert!(r.seal(1, Some("sha256:x".into()), String::new()).is_err());
    assert!(r.seal(2, None, String::new()).is_err());
    assert!(
        r.seal(MAX_SEQ, Some("sha256:x".into()), String::new())
            .is_ok()
    );
    assert!(r.seal(1, None, String::new()).is_ok());
}

#[test]
fn a_record_round_trips_through_json_and_still_verifies() {
    let records = chain(3);
    let json = serde_json::to_string(&records).unwrap();
    let back: Vec<AuditRecord> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, records);
    assert!(verify_chain(&back, &[], &[]).is_intact());
}

// ── The hash chain ───────────────────────────────────────────────────────────

#[test]
fn an_intact_chain_verifies() {
    let r = verify_chain(&chain(5), &[], &[]);
    assert!(r.is_intact(), "{r:?}");
    assert_eq!(r.range, Some((1, 5)));
    assert_eq!(r.unsigned_tail(), 5);
}

#[test]
fn an_edited_field_breaks_the_record() {
    let mut records = chain(5);
    records[2].task_id = Some("t-other".to_owned());
    let r = verify_chain(&records, &[], &[]);
    assert_eq!(r.failure.unwrap().seq, 3);
}

#[test]
fn a_resealed_edit_breaks_the_next_link() {
    // An editor who recomputes the edited record's hash still cannot fix the
    // record after it, whose `prev` names the original.
    let mut records = chain(5);
    records[2].task_id = Some("t-other".to_owned());
    records[2].hash = records[2].compute_hash().unwrap();
    let f = verify_chain(&records, &[], &[]).failure.unwrap();
    assert_eq!(f.seq, 4);
    assert!(f.reason.contains("prev"), "{f:?}");
}

#[test]
fn a_deleted_record_is_a_gap() {
    let mut records = chain(5);
    records.remove(2);
    let f = verify_chain(&records, &[], &[]).failure.unwrap();
    assert_eq!(f.seq, 4);
    assert!(f.reason.contains("missing or reordered"), "{f:?}");
}

#[test]
fn reordered_records_are_found() {
    let mut records = chain(5);
    records.swap(1, 2);
    assert!(!verify_chain(&records, &[], &[]).is_intact());
}

#[test]
fn a_record_from_another_chain_is_found() {
    let mut records = chain(3);
    records[1].chain = "other".to_owned();
    records[1].hash = records[1].compute_hash().unwrap();
    let f = verify_chain(&records, &[], &[]).failure.unwrap();
    assert!(f.reason.contains("chain"), "{f:?}");
}

#[test]
fn a_chain_that_lost_its_start_needs_an_anchor() {
    let records = chain(5);
    let f = verify_chain(&records[2..], &[], &[]).failure.unwrap();
    assert!(f.reason.contains("no anchor"), "{f:?}");
}

// ── Checkpoints ──────────────────────────────────────────────────────────────

#[test]
fn checkpoints_verify_under_es256_and_ed25519() {
    for signer in [es256_signer("k1"), ed25519_signer("k1")] {
        let records = chain(6);
        let cps = [
            checkpoint(&signer, &records, 3),
            checkpoint(&signer, &records, 5),
        ];
        let r = verify_chain(&records, &cps, &[signer.public_key()]);
        assert!(r.is_intact(), "{r:?}");
        assert_eq!(r.checkpoints_verified, 2);
        assert_eq!(r.signed_through, Some(5));
        assert_eq!(r.unsigned_tail(), 1);
    }
}

#[test]
fn truncating_the_tail_past_a_checkpoint_is_found() {
    let signer = es256_signer("k1");
    let records = chain(6);
    let cps = [checkpoint(&signer, &records, 5)];
    let f = verify_chain(&records[..4], &cps, &[signer.public_key()])
        .failure
        .unwrap();
    assert_eq!(f.seq, 5);
    assert!(f.reason.contains("tail was removed"), "{f:?}");
}

#[test]
fn a_checkpoint_signed_by_an_untrusted_key_fails() {
    let records = chain(3);
    let cps = [checkpoint(&es256_signer("k1"), &records, 2)];
    let other = es256_signer("k1");
    let f = verify_chain(&records, &cps, &[other.public_key()])
        .failure
        .unwrap();
    assert!(f.reason.contains("does not verify"), "{f:?}");
}

#[test]
fn a_key_is_never_tried_under_another_algorithm() {
    let signer = ed25519_signer("k1");
    let records = chain(2);
    let cps = [checkpoint(&signer, &records, 2)];
    let mut wrong_alg = signer.public_key();
    wrong_alg.alg = SigningAlg::Es256;
    let f = verify_chain(&records, &cps, &[wrong_alg]).failure.unwrap();
    assert!(f.reason.contains("no trusted EdDSA key"), "{f:?}");
}

#[test]
fn an_edited_checkpoint_fails() {
    let signer = es256_signer("k1");
    let records = chain(3);
    let mut cp = checkpoint(&signer, &records, 2);
    cp.time = "2030-01-01T00:00:00.000Z".to_owned();
    let f = verify_chain(&records, &[cp], &[signer.public_key()])
        .failure
        .unwrap();
    assert!(f.reason.contains("does not verify"), "{f:?}");
}

#[test]
fn a_checkpoint_naming_another_hash_fails() {
    let signer = es256_signer("k1");
    let records = chain(3);
    let mut cp = Checkpoint::new("checkpoint", "acme", 2, records[0].hash.clone(), "t");
    signer.sign(&mut cp).unwrap();
    let f = verify_chain(&records, &[cp], &[signer.public_key()])
        .failure
        .unwrap();
    assert!(f.reason.contains("different hash"), "{f:?}");
}

#[test]
fn an_unsigned_checkpoint_is_not_trusted() {
    let records = chain(3);
    let cp = Checkpoint::new("checkpoint", "acme", 2, records[1].hash.clone(), "t");
    let f = verify_chain(&records, &[cp], &[]).failure.unwrap();
    assert!(f.reason.contains("unsigned"), "{f:?}");
}

#[test]
fn a_signed_anchor_lets_a_purged_chain_verify() {
    let signer = ed25519_signer("k1");
    let records = chain(6);
    let mut anchor = Checkpoint::new("anchor", "acme", 3, records[2].hash.clone(), "t");
    signer.sign(&mut anchor).unwrap();
    let r = verify_chain(&records[3..], &[anchor], &[signer.public_key()]);
    assert!(r.is_intact(), "{r:?}");
    assert_eq!(r.anchored_at, Some(3));
}

#[test]
fn a_forged_anchor_does_not_rescue_a_chain() {
    let signer = ed25519_signer("k1");
    let records = chain(6);
    let mut anchor = Checkpoint::new("anchor", "acme", 3, records[1].hash.clone(), "t");
    signer.sign(&mut anchor).unwrap();
    let f = verify_chain(&records[3..], &[anchor], &[signer.public_key()])
        .failure
        .unwrap();
    assert!(f.reason.contains("prev"), "{f:?}");
}

#[test]
fn the_signer_never_prints_its_key() {
    let s = format!("{:?}", es256_signer("k1"));
    assert!(s.contains("redacted") && s.contains("k1"), "{s}");
}

// ── Keys made by OpenSSL ─────────────────────────────────────────────────────
//
// Throwaway keys generated for these tests with OpenSSL 3.0.13 — never used
// for anything else. The commands are the ones the book gives operators.

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// `openssl genpkey -algorithm ed25519 -outform DER`: PKCS#8 *v1*, with no
/// embedded public key, which is what OpenSSL writes.
const OPENSSL_ED25519_PKCS8_V1: &str = "302e020100300506032b657004220420d5fbda7c2a7b292e5f0d5ef73d8d1d77d8a298274fc21fa6557bde4d46feb9a6";

/// `openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 |
/// openssl pkcs8 -topk8 -nocrypt -outform DER`.
const OPENSSL_P256_PKCS8: &str = "308187020100301306072a8648ce3d020106082a8648ce3d030107046d306b0201010420c431c6cc265810a7c3791f96962770cb81f3bd8063be280c5abd09cd0e0b9094a14403420004c3f6e344f0a2615e4e48d135fcd54f882249c4337a2ffab2e6e5e398f4e7d7a62b63c1c009b156ccb085e23b4c26c98175d90e5c6ea382f3576aa2db83fa0456";

/// `openssl genpkey -algorithm EC … -outform DER` without `pkcs8 -topk8`:
/// SEC1 `ECPrivateKey`, not PKCS#8.
const OPENSSL_P256_SEC1: &str = "30770201010420c431c6cc265810a7c3791f96962770cb81f3bd8063be280c5abd09cd0e0b9094a00a06082a8648ce3d030107a14403420004c3f6e344f0a2615e4e48d135fcd54f882249c4337a2ffab2e6e5e398f4e7d7a62b63c1c009b156ccb085e23b4c26c98175d90e5c6ea382f3576aa2db83fa0456";

#[test]
fn an_openssl_ed25519_key_signs_checkpoints() {
    let signer =
        CheckpointSigner::from_pkcs8(SigningAlg::EdDsa, "ossl", &unhex(OPENSSL_ED25519_PKCS8_V1))
            .unwrap();
    let records = chain(2);
    let cps = [checkpoint(&signer, &records, 2)];
    assert!(verify_chain(&records, &cps, &[signer.public_key()]).is_intact());
}

#[test]
fn an_openssl_p256_key_signs_checkpoints() {
    let signer =
        CheckpointSigner::from_pkcs8(SigningAlg::Es256, "ossl", &unhex(OPENSSL_P256_PKCS8))
            .unwrap();
    let records = chain(2);
    let cps = [checkpoint(&signer, &records, 2)];
    assert!(verify_chain(&records, &cps, &[signer.public_key()]).is_intact());
}

#[test]
fn a_sec1_key_is_refused_with_the_command_that_converts_it() {
    let err = CheckpointSigner::from_pkcs8(SigningAlg::Es256, "ossl", &unhex(OPENSSL_P256_SEC1))
        .unwrap_err();
    assert!(err.to_string().contains("openssl pkcs8 -topk8"), "{err}");
}

/// A chain purged twice, verified against everything a real store holds for
/// it and beside it: the earlier anchor, an old checkpoint the purge left
/// behind, one exactly at the first remaining record, and another chain's
/// checkpoints at the same and later positions. Only this chain's newest
/// anchor and its checkpoints in range may count.
#[test]
fn a_twice_purged_chain_verifies_among_everything_else_in_the_store() {
    let signer = ed25519_signer("k1");
    let records = chain(12);
    let kept = &records[8..]; // seq 9..=12
    let cp = |kind: &str, chain: &str, seq: u64, hash: &str| {
        let mut c = Checkpoint::new(kind, chain, seq, hash, "t");
        signer.sign(&mut c).unwrap();
        c
    };
    let checkpoints = vec![
        cp("checkpoint", "globex", 8, "sha256:other"),
        cp("anchor", "acme", 4, &records[3].hash),
        cp("checkpoint", "acme", 5, &records[4].hash),
        cp("anchor", "acme", 8, &records[7].hash),
        cp("checkpoint", "acme", 9, &records[8].hash),
        cp("checkpoint", "acme", 12, &records[11].hash),
        cp("checkpoint", "globex", 20, "sha256:other"),
    ];
    let r = verify_chain(kept, &checkpoints, &[signer.public_key()]);
    assert!(r.is_intact(), "{r:?}");
    assert_eq!(r.anchored_at, Some(8));
    assert_eq!(r.range, Some((9, 12)));
    assert_eq!(r.checkpoints_verified, 2, "seq 9 and seq 12, nothing else");
    assert_eq!(r.signed_through, Some(12));
}

#[test]
fn the_largest_sequence_number_is_two_to_the_53_minus_one() {
    assert_eq!(MAX_SEQ, 9_007_199_254_740_991);
    let mut r = AuditRecord::new("acme", kind::CALL);
    r.seal(9_007_199_254_740_991, Some("sha256:p".into()), "t".into())
        .expect("the largest exact double");
    assert!(
        r.seal(9_007_199_254_740_992, Some("sha256:p".into()), "t".into())
            .is_err()
    );
}

/// Python: `hashlib.sha256(b'{"a":1,"b":[true,null]}')`.
#[test]
fn digest_of_is_sha256_over_the_canonical_json() {
    #[derive(serde::Serialize)]
    struct S {
        b: Vec<Option<bool>>,
        a: u8,
    }
    let want = "sha256:1cc69c7fa23616ca2ec3ee70d24390a6225c8832db8a4c814c7e0e7f942f8668";
    assert_eq!(
        digest_of(&S {
            b: vec![Some(true), None],
            a: 1
        })
        .unwrap(),
        want
    );
}

#[test]
fn a_signer_reports_its_key_id() {
    assert_eq!(es256_signer("audit-2026-10").kid(), "audit-2026-10");
}
