// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What [`Profile::RegulatedData`](crate::profile::Profile::RegulatedData)
//! asks of the builder, and the check every declaration of an external control
//! passes (adversarial review of 2026-11-13).

use super::RequestHandlerBuilder;
use crate::error::{ServerError, ServerResult};
use crate::profile::{ExternalControl, ProfileKind};

/// The controls the profile relies on and cannot see, each to be declared.
const MUST_DECLARE: [ExternalControl; 4] = [
    ExternalControl::TrustedProxies,
    ExternalControl::KeyCustody,
    ExternalControl::Backups,
    ExternalControl::Retention,
];

fn refuse(why: impl Into<String>) -> ServerResult<()> {
    Err(ServerError::Config(format!(
        "the regulated-data profile {}",
        why.into()
    )))
}

/// Without the features, the audit log the profile needs cannot exist.
#[cfg(not(all(feature = "audit", feature = "encryption")))]
fn refuse_without_audit_features() -> ServerResult<()> {
    refuse(
        "needs an audit log that seals what its records say about people, which \
         needs the `audit` and `encryption` features: enable both",
    )
}

impl RequestHandlerBuilder {
    fn declared(&self, control: ExternalControl) -> bool {
        self.external_controls.iter().any(|(c, _)| *c == control)
    }

    /// A blank declaration is refused whatever the profiles; the regulated-data
    /// profile's requirements are checked when it was applied.
    pub(super) fn check_declarations_and_regulated(&self) -> ServerResult<()> {
        if let Some((control, _)) = self
            .external_controls
            .iter()
            .find(|(_, how)| how.trim().is_empty())
        {
            return Err(ServerError::Config(format!(
                "the declaration of {} says nothing: say what the deployment does",
                control.name()
            )));
        }
        if !self.profiles.contains(&ProfileKind::RegulatedData) {
            return Ok(());
        }
        if !self.interceptors.has_authenticator() {
            return refuse(
                "needs an authenticating interceptor: otherwise records name no caller \
                 and any caller reaches every task",
            );
        }
        #[cfg(all(feature = "audit", feature = "encryption"))]
        self.check_regulated_audit()?;
        #[cfg(not(all(feature = "audit", feature = "encryption")))]
        refuse_without_audit_features()?;
        self.check_regulated_storage()?;
        self.check_regulated_egress()?;
        if let Some(missing) = MUST_DECLARE.iter().find(|c| !self.declared(**c)) {
            return refuse(format!(
                "needs a declaration of {} (declare_external_control(ExternalControl::{missing:?}, \
                 \"…\")): it is outside what this process can check, so the deployment says \
                 how it is met, for the record",
                missing.name()
            ));
        }
        Ok(())
    }

    /// A required audit log that seals the people its records name.
    #[cfg(all(feature = "audit", feature = "encryption"))]
    fn check_regulated_audit(&self) -> ServerResult<()> {
        let Some(log) = &self.audit else {
            return refuse("needs an audit log (with_audit)");
        };
        if !log.is_required() {
            return refuse(
                "needs a required audit log (AuditLog::require_record(true)): otherwise \
                 a call it cannot record is served unrecorded",
            );
        }
        if !log.is_private() {
            return refuse(
                "needs an audit log that seals what records say about people \
                 (AuditLog::with_privacy): otherwise callers and approvers are stored \
                 in the clear and cannot be erased one by one",
            );
        }
        Ok(())
    }

    /// Task content sealed by the store, or declared encrypted at rest.
    fn check_regulated_storage(&self) -> ServerResult<()> {
        let sealed = self
            .task_store
            .as_ref()
            .is_some_and(|store| store.seals_content());
        if sealed || self.declared(ExternalControl::EncryptionAtRest) {
            return Ok(());
        }
        refuse(
            "needs task content sealed at rest: a task store that seals it \
             (EncryptedTaskStore, feature `encryption`), or a declaration that the storage \
             is encrypted (declare_external_control(ExternalControl::EncryptionAtRest, \"…\"))",
        )
    }

    /// What the handler itself fetches or sends to: file URLs and webhooks.
    fn check_regulated_egress(&self) -> ServerResult<()> {
        if !self.refuse_private_file_urls {
            return refuse(
                "needs file URLs that name a private address refused \
                 (refuse_private_file_urls): otherwise an executor that fetches a part's URL \
                 can be pointed into the internal network",
            );
        }
        if let Some(sender) = &self.push_sender {
            if sender.allows_private_urls() {
                return refuse(
                    "found a push sender that allows private addresses \
                     (HttpPushSender::allow_private_urls): a webhook could be registered \
                     into the internal network and sent task content",
                );
            }
            if !sender.requires_https() {
                return refuse(
                    "found a push sender that sends over plain HTTP: use \
                     HttpPushSender::https_only, so task content is not sent in the clear",
                );
            }
        }
        Ok(())
    }
}
