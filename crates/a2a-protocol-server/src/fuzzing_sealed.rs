// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The readers of sealed values stored at rest, for the fuzz targets in
//! `fuzz/`: `Aes256GcmCipher`'s open (key-id length, key id, nonce, tag),
//! a sealed task part's (its JSON shape, version, base64), a sealed push
//! config's (prefix, base64) and the audit log's (prefix, handle, base64,
//! the wrapped subject key, the keyed digest's key id and hex). Whoever can
//! write the store chooses these bytes. Each forwarder runs the reader
//! unchanged, under fixed test keys, to completion: every cipher here
//! answers at once, so a poll loop with a no-op waker is enough.

use std::future::Future;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll, Waker};

use a2a_protocol_types::message::Part;

use crate::store::encrypted::{Aes256GcmCipher, ContentCipher, Sealer};

/// The key the fuzz ring seals with, under the id `"k"`.
pub const FUZZ_KEY: [u8; 32] = [0x5a; 32];
/// A second key in the ring, under the id `"old"`, which only opens.
pub const FUZZ_OLD_KEY: [u8; 32] = [0xa5; 32];

/// Runs `f` to completion on this thread.
fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(out) = f.as_mut().poll(&mut cx) {
            return out;
        }
    }
}

fn ring() -> Option<&'static Arc<Aes256GcmCipher>> {
    static RING: OnceLock<Option<Arc<Aes256GcmCipher>>> = OnceLock::new();
    RING.get_or_init(|| {
        Aes256GcmCipher::new("k", FUZZ_KEY)
            .and_then(|r| r.with_key("old", FUZZ_OLD_KEY))
            .ok()
            .map(Aes256GcmCipher::into_arc)
    })
    .as_ref()
}

/// `Aes256GcmCipher::seal` under the fuzz ring.
#[must_use]
pub fn ring_seal(plaintext: &[u8], context: &[u8]) -> Option<Vec<u8>> {
    block_on(ContentCipher::seal(&**ring()?, plaintext.to_vec(), context)).ok()
}

/// `Aes256GcmCipher::open` under the fuzz ring.
#[must_use]
pub fn ring_open(sealed: &[u8], context: &[u8]) -> Option<Vec<u8>> {
    block_on(ContentCipher::open(&**ring()?, sealed.to_vec(), context)).ok()
}

fn sealer(accept_unsealed: bool) -> Option<Sealer<'static>> {
    Some(Sealer {
        cipher: &**ring()?,
        partition: String::new(),
        accept_unsealed,
    })
}

/// `part` sealed as `EncryptedTaskStore` stores it, under `context`.
#[must_use]
pub fn part_seal(part: &Part, context: &[u8]) -> Option<Part> {
    block_on(sealer(false)?.seal_part(part, context)).ok()
}

/// A stored part opened as `EncryptedTaskStore` reads it, under `context`.
#[must_use]
pub fn part_open(part: Part, context: &[u8], accept_unsealed: bool) -> Option<Part> {
    block_on(sealer(accept_unsealed)?.open_part(part, context)).ok()
}

#[cfg(any(feature = "sqlite", feature = "postgres"))]
fn sealing(accept_unsealed: bool) -> Option<crate::push::cipher::Sealing> {
    let cipher: Arc<dyn crate::push::PushConfigCipher> = ring()?.clone();
    let sealing = crate::push::cipher::Sealing::default().with_cipher(cipher);
    Some(if accept_unsealed {
        sealing.accepting_unsealed()
    } else {
        sealing
    })
}

/// A push config as an SQL store writes it with the fuzz ring.
#[cfg(any(feature = "sqlite", feature = "postgres"))]
#[must_use]
pub fn push_encode(
    config: &a2a_protocol_types::push::TaskPushNotificationConfig,
    context: &[u8],
) -> Option<String> {
    block_on(sealing(false)?.encode_text(config, context)).ok()
}

/// A stored push config as an SQL store with the fuzz ring reads it.
#[cfg(any(feature = "sqlite", feature = "postgres"))]
#[must_use]
pub fn push_decode(
    stored: &str,
    context: &[u8],
    accept_unsealed: bool,
) -> Option<a2a_protocol_types::push::TaskPushNotificationConfig> {
    block_on(sealing(accept_unsealed)?.decode_text(stored, context)).ok()
}

#[cfg(feature = "audit")]
pub use audit::{AuditFixture, audit_digest_matches, audit_fixture, audit_reveal};

#[cfg(feature = "audit")]
mod audit {
    use std::sync::{Arc, OnceLock};

    use a2a_protocol_types::audit::{Actor, AuditRecord, digest_value};

    use super::{block_on, ring};
    use crate::audit::{AuditLog, AuditPrivacy, AuditStore, InMemoryAuditStore, SubjectKey};

    const HMAC_KEY: [u8; 32] = [0x3c; 32];

    /// A protected audit record, the subject keys it was sealed under, and
    /// the content its digest `"message"` is of.
    #[derive(Debug)]
    pub struct AuditFixture {
        /// The record as the store holds it.
        pub record: AuditRecord,
        /// The store's subject keys for the record's chain.
        pub keys: Vec<SubjectKey>,
        /// The content of the digest `"message"`.
        pub content: serde_json::Value,
    }

    fn log(store: Arc<InMemoryAuditStore>) -> Option<AuditLog> {
        let kek = ring()?.clone();
        let privacy = AuditPrivacy::new(kek, "h1", HMAC_KEY).ok()?;
        AuditLog::new(store).with_privacy(privacy).ok()
    }

    /// One record, protected once per process: actor, `requestedBy`,
    /// `reason`, two `actors` and a digest.
    #[must_use]
    pub fn audit_fixture() -> Option<&'static AuditFixture> {
        static FIXTURE: OnceLock<Option<AuditFixture>> = OnceLock::new();
        FIXTURE
            .get_or_init(|| {
                let store = Arc::new(InMemoryAuditStore::new());
                let log = log(Arc::clone(&store))?;
                let content = serde_json::json!({"text": "fuzz"});
                let mut record = AuditRecord::new("fuzz", "call");
                record.actor = Some(Actor::new("alice", None));
                record.detail.insert("requestedBy".into(), "bob".into());
                record.detail.insert("reason".into(), "why".into());
                record
                    .detail
                    .insert("actors".into(), serde_json::json!(["agent", "alice"]));
                record
                    .digests
                    .insert("message".into(), digest_value(&content).ok()?);
                let record = block_on(log.append(record)).ok()?;
                let keys = block_on(store.subject_keys("fuzz")).ok()?;
                Some(AuditFixture {
                    record,
                    keys,
                    content,
                })
            })
            .as_ref()
    }

    /// `AuditLog::reveal` of `record` over a store holding `keys`: the
    /// revealed actor, `requestedBy`, `reason` and `actors`, and the fields
    /// whose key is gone, or `None` when it refuses.
    #[must_use]
    pub fn audit_reveal(
        keys: &[SubjectKey],
        record: &AuditRecord,
    ) -> Option<(AuditRecord, Vec<String>)> {
        let store = Arc::new(InMemoryAuditStore::new());
        for key in keys {
            block_on(store.put_subject_key(&record.chain, key)).ok()?;
        }
        let revealed = block_on(log(store)?.reveal(record)).ok()?;
        Some((revealed.record, revealed.erased))
    }

    /// `AuditLog::digest_matches`, or `None` when it refuses.
    #[must_use]
    pub fn audit_digest_matches(
        record: &AuditRecord,
        name: &str,
        content: &serde_json::Value,
    ) -> Option<bool> {
        let store = Arc::new(InMemoryAuditStore::new());
        log(store)?.digest_matches(record, name, content).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwards_the_ring() {
        let sealed = ring_seal(b"x", b"c").expect("seals");
        assert_eq!(&sealed[..2], b"\x01k");
        assert_eq!(ring_open(&sealed, b"c").as_deref(), Some(&b"x"[..]));
        assert_eq!(ring_open(&sealed, b"d"), None);
    }

    #[test]
    fn forwards_the_part() {
        let sealed = part_seal(&Part::text("t"), b"c").expect("seals");
        assert_ne!(sealed, Part::text("t"));
        assert_eq!(
            part_open(sealed.clone(), b"c", false),
            Some(Part::text("t"))
        );
        assert_eq!(part_open(sealed, b"d", false), None);
        assert_eq!(part_open(Part::text("t"), b"c", false), None);
        assert_eq!(
            part_open(Part::text("t"), b"c", true),
            Some(Part::text("t"))
        );
    }

    #[cfg(any(feature = "sqlite", feature = "postgres"))]
    #[test]
    fn forwards_the_push_config() {
        use a2a_protocol_types::push::TaskPushNotificationConfig;
        let config = TaskPushNotificationConfig::new("t", "https://example.com/h");
        let stored = push_encode(&config, b"c").expect("seals");
        assert!(stored.starts_with("a2a-sealed:v1:"), "{stored}");
        let plain = serde_json::to_string(&config).expect("json");
        let read = push_decode(&stored, b"c", false).expect("opens");
        assert_eq!(serde_json::to_string(&read).expect("json"), plain);
        assert!(push_decode(&stored, b"d", false).is_none());
        assert!(push_decode(&plain, b"c", false).is_none());
        assert!(push_decode(&plain, b"c", true).is_some());
    }

    #[cfg(feature = "audit")]
    #[test]
    fn forwards_the_audit_log() {
        let fx = audit_fixture().expect("fixture");
        let (record, erased) = audit_reveal(&fx.keys, &fx.record).expect("reveals");
        assert_eq!(erased, Vec::<String>::new());
        assert_eq!(record.actor.expect("actor").subject, "alice");
        assert_eq!(
            record.detail["actors"],
            serde_json::json!(["agent", "alice"])
        );
        let (_, erased) = audit_reveal(&[], &fx.record).expect("reveals");
        assert_eq!(erased.len(), 5, "{erased:?}");
        let mut altered = fx.record.clone();
        altered.seq += 1;
        assert!(audit_reveal(&fx.keys, &altered).is_none());
        assert_eq!(
            audit_digest_matches(&fx.record, "message", &fx.content),
            Some(true)
        );
        assert_eq!(
            audit_digest_matches(&fx.record, "message", &serde_json::json!(1)),
            Some(false)
        );
    }
}
