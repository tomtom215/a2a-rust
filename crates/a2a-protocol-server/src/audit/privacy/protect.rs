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

use super::{
    ACTORS_FIELD, AuditPrivacy, KEYED_DIGEST_PREFIX, PERSON_FIELDS, TEXT_FIELDS, actors_element,
    refused,
};
use crate::audit::store::AuditStore;

/// The keys of the people one record names, each as `(sid, key)`, found or
/// made by [`AuditPrivacy::resolve`] before the record is sealed.
pub(in crate::audit) struct Resolved(HashMap<String, (String, Zeroizing<Vec<u8>>)>);

impl Resolved {
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
        let mut people: Vec<&str> = Vec::new();
        let owner = record.actor.as_ref().map_or("", |a| a.subject.as_str());
        if record.actor.is_some() {
            people.push(owner);
        }
        for field in PERSON_FIELDS {
            if let Some(serde_json::Value::String(person)) = record.detail.get(field) {
                people.push(person);
            }
        }
        if let Some(serde_json::Value::Array(actors)) = record.detail.get(ACTORS_FIELD) {
            for actor in actors {
                let serde_json::Value::String(actor) = actor else {
                    return Err(refused());
                };
                people.push(actor);
            }
        }
        if TEXT_FIELDS
            .iter()
            .any(|f| matches!(record.detail.get(*f), Some(serde_json::Value::String(_))))
        {
            people.push(owner);
        }
        let mut keys = HashMap::new();
        for person in people {
            if !keys.contains_key(person) {
                let key = self.subject_key(store, chain, person).await?;
                keys.insert(person.to_owned(), key);
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
        for field in PERSON_FIELDS {
            if let Some(serde_json::Value::String(person)) = record.detail.get(field).cloned() {
                let (sid, dek) = resolved.key(&person)?;
                let sealed = self.seal_value(dek, &chain, sid, field, seq, &person)?;
                record.detail.insert(field.to_owned(), sealed.into());
            }
        }
        if let Some(serde_json::Value::Array(actors)) = record.detail.get(ACTORS_FIELD).cloned() {
            let mut sealed = Vec::with_capacity(actors.len());
            for (i, actor) in actors.iter().enumerate() {
                let serde_json::Value::String(actor) = actor else {
                    return Err(refused());
                };
                let (sid, dek) = resolved.key(actor)?;
                let field = actors_element(i);
                sealed.push(
                    self.seal_value(dek, &chain, sid, &field, seq, actor)?
                        .into(),
                );
            }
            record
                .detail
                .insert(ACTORS_FIELD.to_owned(), serde_json::Value::Array(sealed));
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
