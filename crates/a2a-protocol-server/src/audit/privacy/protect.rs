// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Protecting a record: resolving the keys of the people it names (the part
//! that reads the store, run before an append begins) and sealing it with them
//! (the part that reads nothing, run inside the append). See
//! [`AuditLog::append_locked`](crate::audit::AuditLog) for why they are apart.

use std::collections::HashMap;

use a2a_protocol_types::audit::AuditRecord;
use a2a_protocol_types::error::{A2aError, A2aResult};
use zeroize::Zeroizing;

use super::fields::{TEXT_FIELDS, people_in};
use super::{AuditPrivacy, KEYED_DIGEST_PREFIX, refused};
use crate::audit::store::AuditStore;

/// The keys of the people one record names, each as `(sid, key)`, found or
/// made by [`AuditPrivacy::resolve`] before the record is sealed.
pub(in crate::audit) struct Resolved(HashMap<String, (String, Zeroizing<Vec<u8>>)>);

impl Resolved {
    /// The handles of the keys, one per person, for the store to check are
    /// still there when it stores the record (`AuditStore::append_next_naming`).
    pub(in crate::audit) fn sids(&self) -> Vec<String> {
        let mut sids: Vec<String> = self.0.values().map(|(sid, _)| sid.clone()).collect();
        sids.sort_unstable();
        sids.dedup();
        sids
    }

    /// `person`'s key, which [`AuditPrivacy::resolve`] found for every person
    /// the record names.
    fn key(&self, person: &str) -> A2aResult<&(String, Zeroizing<Vec<u8>>)> {
        self.0.get(person).ok_or_else(|| {
            A2aError::internal("an audit subject key was not resolved before sealing")
        })
    }
}

impl AuditPrivacy {
    /// The key of every person `record` names, found or made, **before** the
    /// record is sealed. This is the only part of protecting a record that
    /// touches the store, and it must run before the store's append begins:
    /// the PostgreSQL store holds a connection for the whole of an append, so
    /// a store read while sealing needs a second one, and appends to different
    /// chains that each hold one while waiting for another exhaust the pool
    /// between them (adversarial review A2, 2026-11-12).
    pub(in crate::audit) async fn resolve(
        &self,
        store: &dyn AuditStore,
        record: &AuditRecord,
    ) -> A2aResult<Resolved> {
        let chain = record.chain.as_str();
        let mut people: Vec<String> = Vec::new();
        let owner = record.actor.as_ref().map_or("", |a| a.subject.as_str());
        if record.actor.is_some() {
            people.push(owner.to_owned());
        }
        people.extend(people_in(&record.detail)?.into_iter().map(|(_, p)| p));
        if TEXT_FIELDS
            .iter()
            .any(|f| matches!(record.detail.get(*f), Some(serde_json::Value::String(_))))
        {
            people.push(owner.to_owned());
        }
        let mut keys = HashMap::new();
        for person in people {
            if let std::collections::hash_map::Entry::Vacant(slot) = keys.entry(person) {
                let key = self.subject_key(store, chain, slot.key()).await?;
                slot.insert(key);
            }
        }
        Ok(Resolved(keys))
    }

    /// Seals what `record` says about people, and keys its digests, for the
    /// record about to be sealed at `seq`, with the keys `resolved` holds. It
    /// reads nothing from any store.
    pub(in crate::audit) fn protect(
        &self,
        resolved: &Resolved,
        record: &mut AuditRecord,
        seq: u64,
    ) -> A2aResult<()> {
        let chain = record.chain.clone();
        let owner = record
            .actor
            .as_ref()
            .map_or_else(String::new, |a| a.subject.clone());
        let mut owner_key: Option<&(String, Zeroizing<Vec<u8>>)> =
            if let Some(actor) = record.actor.as_mut() {
                let key = resolved.key(&actor.subject)?;
                actor.subject =
                    self.seal_value(&key.1, &chain, &key.0, "actor", seq, &actor.subject)?;
                Some(key)
            } else {
                None
            };
        for (slot, person) in people_in(&record.detail)? {
            let (sid, dek) = resolved.key(&person)?;
            let sealed = self.seal_value(dek, &chain, sid, &slot.context(), seq, &person)?;
            slot.set(&mut record.detail, sealed);
        }
        for field in TEXT_FIELDS {
            if let Some(serde_json::Value::String(text)) = record.detail.get(field).cloned() {
                if owner_key.is_none() {
                    owner_key = Some(resolved.key(&owner)?);
                }
                let (sid, dek) = owner_key.ok_or_else(refused)?;
                let sealed = self.seal_value(dek, &chain, sid, field, seq, &text)?;
                record.detail.insert(field.to_owned(), sealed.into());
            }
        }
        for (name, digest) in &mut record.digests {
            let mac = self.mac(
                &self.active,
                &serde_json::json!(["a2a-audit-digest/v1", chain, seq, name, digest]),
            );
            *digest = format!("{KEYED_DIGEST_PREFIX}{}:{mac}", self.active);
        }
        Ok(())
    }
}
