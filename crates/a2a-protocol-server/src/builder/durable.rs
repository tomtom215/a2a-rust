// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What [`Profile::LocalDevelopment`](crate::profile::Profile::LocalDevelopment)
//! and [`Profile::DurableProduction`](crate::profile::Profile::DurableProduction)
//! ask of the builder, and the [`ProfileReport`] every profile contributes to
//! (adversarial review of 2026-11-13).

use super::{CallerIsolation, CardSetting, RequestHandlerBuilder, TenantIsolation};
use crate::error::{ServerError, ServerResult};
use crate::profile::{ProfileKind, ProfileReport};

/// The guarantees an escape hatch can turn off, named once so that the list
/// of what was checked and the hatch that removes it from that list agree.
const SHARED_STORES: &str = "over a task store several processes share, the halt, lease, \
                             push-config and audit stores are shared too";
const TENANT_PARTITIONS: &str = "the task and push-config stores partition by tenant";
const AUTHENTICATED: &str = "an authenticating interceptor names every caller";

/// The escape hatches, and the guarantee each removes from what was checked.
const HATCHES: [(&str, Option<&str>); 4] = [
    ("Profile::LocalDevelopment", None),
    ("allow_process_local_stores()", Some(SHARED_STORES)),
    ("accept_unisolated_tenants()", Some(TENANT_PARTITIONS)),
    ("share_tasks_across_callers()", None),
];

#[cfg(feature = "audit")]
const AUDITABLE: &[&str] = &[
    AUTHENTICATED,
    "the audit log is required: a call it cannot record is refused",
    "the audit log signs checkpoints",
    "an approval gate checks every approval",
];
#[cfg(feature = "audit")]
const AUDITABLE_UNSEEN: &[&str] = &[
    "the audit store is backed up and a copy is kept elsewhere",
    "two approvers are two people: the gate tells identities apart, not persons",
];
const PRODUCTION: &[&str] = &[
    AUTHENTICATED,
    "a halt store and a lease store are set",
    "what this agent sends is signed",
    SHARED_STORES,
];
const PRODUCTION_UNSEEN: &[&str] = &[
    "TLS is terminated as ServeConfig::production requires, or by a proxy in front",
    "forwarded headers are trusted only from the proxies named",
];
const MULTI_TENANT: &[&str] = &[
    AUTHENTICATED,
    "the tenant resolver was declared authorized",
    "a request no tenant is found for is refused",
    TENANT_PARTITIONS,
    "every tenant has a concurrency limit",
];
const MULTI_TENANT_UNSEEN: &[&str] = &[
    "the tenant resolver is in fact authorized (the builder was told so)",
    "tenant limits hold for the fleet (they are per replica)",
];
const DURABLE: &[&str] = &[
    AUTHENTICATED,
    "the task store keeps its tasks beyond this process",
    "with push notifications declared, the push-config store and the push outbox keep their \
     rows beyond this process",
    "a halt, lease or audit store that is set keeps its state beyond this process",
];
const DURABLE_UNSEEN: &[&str] = &[
    "the databases are backed up and a restore has been rehearsed",
    "the database outlives the loss of its host",
    "a tool loop's step journal is durable (it belongs to the executor, which the handler \
     cannot see)",
    "the keys of encrypted stores, audit privacy and the journal cipher are held and rotated \
     outside this process",
    "retention jobs (purge_expired, AuditLog::purge, StepJournal::sweep) are scheduled",
];

const REGULATED: &[&str] = &[
    AUTHENTICATED,
    "the audit log is required and seals what records say about people",
    "task content is sealed by the store, or declared encrypted at rest",
    "file URLs naming a private address are refused",
    "a push sender, when set, refuses private addresses and plain HTTP",
    "trusted proxies, key custody, backups and retention are declared",
];
const REGULATED_UNSEEN: &[&str] = &[
    "each declaration is true",
    "what an executor or a tool sends out (a tool's own egress is the tool's)",
    "lawful basis, purpose, and every other legal duty",
];

/// What a profile verifies and what it relies on without being able to:
/// `(name, checked, not verifiable)`.
const fn guarantees(
    kind: ProfileKind,
) -> (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
) {
    match kind {
        #[cfg(feature = "audit")]
        ProfileKind::Auditable => ("Auditable", AUDITABLE, AUDITABLE_UNSEEN),
        ProfileKind::Production => ("Production", PRODUCTION, PRODUCTION_UNSEEN),
        ProfileKind::MultiTenant => ("MultiTenant", MULTI_TENANT, MULTI_TENANT_UNSEEN),
        ProfileKind::LocalDevelopment => ("LocalDevelopment", &[], &[]),
        ProfileKind::DurableProduction => ("DurableProduction", DURABLE, DURABLE_UNSEEN),
        ProfileKind::RegulatedData => ("RegulatedData", REGULATED, REGULATED_UNSEEN),
    }
}

/// The refusal for a store that keeps its state in this process.
fn process_local(store: &str, effect: &str, fix: &str) -> ServerError {
    ServerError::Config(format!(
        "the durable-production profile found {store}, which keeps its state in this \
         process alone: {effect}. {fix}"
    ))
}

/// The refusal for a store the profile needs and the builder was not given.
fn missing(store: &str, call: &str, effect: &str) -> ServerError {
    ServerError::Config(format!(
        "the durable-production profile needs {store} ({call}): {effect}"
    ))
}

impl RequestHandlerBuilder {
    fn has_profile(&self, kind: ProfileKind) -> bool {
        self.profiles.contains(&kind)
    }

    fn card_pushes(&self) -> bool {
        matches!(
            &self.agent_card,
            CardSetting::Card(card) if card.capabilities.push_notifications == Some(true)
        )
    }

    /// `LocalDevelopment` with a profile that claims the opposite is refused;
    /// `DurableProduction`'s requirements are checked.
    pub(super) fn check_local_and_durable(&self) -> ServerResult<()> {
        if self.has_profile(ProfileKind::LocalDevelopment) {
            for (kind, name) in [
                (ProfileKind::Production, "Production"),
                (ProfileKind::DurableProduction, "DurableProduction"),
                (ProfileKind::RegulatedData, "RegulatedData"),
            ] {
                if self.has_profile(kind) {
                    return Err(ServerError::Config(format!(
                        "the local-development profile is for state that may be lost and \
                         data that need not be protected, and the {name} profile says \
                         otherwise: apply one"
                    )));
                }
            }
        }
        if self.has_profile(ProfileKind::DurableProduction) {
            if !self.interceptors.has_authenticator() {
                return Err(ServerError::Config(
                    "the durable-production profile needs an authenticating interceptor: \
                     otherwise any caller reaches every method and every task"
                        .to_owned(),
                ));
            }
            self.check_durable_task_store()?;
            self.check_durable_push()?;
            self.check_durable_side_stores()?;
        }
        Ok(())
    }

    fn check_durable_task_store(&self) -> ServerResult<()> {
        match &self.task_store {
            None => Err(missing(
                "a task store",
                "with_task_store",
                "the default keeps every task in this process, and a restart loses them",
            )),
            Some(store) if store.is_process_local() => Err(process_local(
                "the task store",
                "a restart loses every task, and its event log",
                "Use SqliteTaskStore over a file (feature `sqlite`) or PostgresTaskStore \
                 (feature `postgres`), or their TenantAware forms",
            )),
            Some(_) => Ok(()),
        }
    }

    /// Checked only when the card promises push notifications.
    fn check_durable_push(&self) -> ServerResult<()> {
        if !self.card_pushes() {
            return Ok(());
        }
        let promised = "the card promises push notifications, and";
        match &self.push_config_store {
            None => {
                return Err(missing(
                    "a push-config store",
                    "with_push_config_store",
                    &format!("{promised} the default keeps every webhook in this process"),
                ));
            }
            Some(s) if s.is_process_local() => {
                return Err(process_local(
                    "the push-config store",
                    &format!("{promised} every webhook registered is lost at a restart"),
                    "Use SqlitePushConfigStore over a file or PostgresPushConfigStore, or \
                     their TenantAware forms",
                ));
            }
            Some(_) => {}
        }
        match &self.push_outbox {
            None => Err(missing(
                "a push outbox",
                "with_push_outbox",
                &format!(
                    "{promised} without one a delivery owed when the process stops is never made"
                ),
            )),
            Some(o) if o.is_process_local() => Err(process_local(
                "the push outbox",
                &format!("{promised} a delivery owed when the process stops is never made"),
                "Use SqlitePushOutbox over a file or PostgresPushOutbox",
            )),
            Some(_) => Ok(()),
        }
    }

    /// The halt, lease and audit stores, each only when one is set.
    fn check_durable_side_stores(&self) -> ServerResult<()> {
        if self
            .halt_store
            .as_ref()
            .is_some_and(|s| s.is_process_local())
        {
            return Err(process_local(
                "the halt store",
                "a restart forgets every halt",
                "Use SqliteHaltStore over a file or PostgresHaltStore",
            ));
        }
        if self
            .lease_store
            .as_ref()
            .is_some_and(|s| s.is_process_local())
        {
            return Err(process_local(
                "the lease store",
                "a restart forgets every lease, and a delegated task runs on unwatched",
                "Use PostgresLeaseStore",
            ));
        }
        #[cfg(feature = "audit")]
        if self
            .audit
            .as_ref()
            .is_some_and(|log| log.store().is_process_local())
        {
            return Err(process_local(
                "the audit store",
                "a restart loses the record of what the agent did",
                "Use SqliteAuditStore over a file or PostgresAuditStore",
            ));
        }
        Ok(())
    }

    /// The escape hatches this builder was given, each with what it gives up.
    fn hatches_given(&self) -> Vec<(&'static str, &'static str)> {
        let given = [
            self.has_profile(ProfileKind::LocalDevelopment),
            self.has_profile(ProfileKind::Production) && self.allow_process_local_stores,
            self.has_profile(ProfileKind::MultiTenant)
                && matches!(self.tenant_isolation, TenantIsolation::SharedAccepted),
            !self.profiles.is_empty()
                && matches!(self.caller_isolation, Some(CallerIsolation::Shared)),
        ];
        let gives_up = [
            "tasks, webhooks, halts and leases may live in this process alone: a restart loses \
             them and no other process sees them",
            "halt, lease, push-config and audit stores are not checked against a task store \
             several processes share: a halt or a webhook may reach one replica only",
            "the task and push-config stores need not partition by tenant",
            "callers in one tenant reach each other's tasks",
        ];
        HATCHES
            .iter()
            .zip(given)
            .zip(gives_up)
            .filter(|((_, given), _)| *given)
            .map(|(((call, _), _), lost)| (*call, lost))
            .collect()
    }

    /// The report of what this builder's profiles guarantee, with the escape
    /// hatches it was given; each hatch is also logged at WARN, here.
    pub(super) fn profile_report(&self) -> ProfileReport {
        let mut r = ProfileReport::default();
        for kind in &self.profiles {
            let (name, checked, unverifiable) = guarantees(*kind);
            ProfileReport::add(&mut r.profiles, &[name]);
            ProfileReport::add(&mut r.checked, checked);
            ProfileReport::add(&mut r.not_verifiable, unverifiable);
        }
        r.declared.clone_from(&self.external_controls);
        for (call, lost) in self.hatches_given() {
            // A guarantee a hatch turns off is not also listed as checked.
            if let Some((_, Some(negated))) = HATCHES.iter().find(|(c, _)| *c == call) {
                r.checked.retain(|c| c != negated);
            }
            trace_warn!(
                escape_hatch = call,
                gives_up = lost,
                "a deployment profile is relaxed"
            );
            r.disabled.push((call, lost));
        }
        r
    }
}
