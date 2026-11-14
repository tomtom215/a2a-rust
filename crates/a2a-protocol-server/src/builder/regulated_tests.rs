// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `Profile::RegulatedData` and declarations of external controls
//! (adversarial review of 2026-11-13).

use crate::profile::{ExternalControl, Profile};
use crate::{BearerTokenAuthInterceptor, RequestHandlerBuilder, agent_executor};

struct Idle;
agent_executor!(Idle, |_ctx, _q| async { Ok(()) });

fn refusal(b: RequestHandlerBuilder) -> String {
    match b.build() {
        Err(crate::error::ServerError::Config(m)) => m,
        other => panic!("expected a refusal, got {:?}", other.map(|_| ())),
    }
}

fn authenticated() -> BearerTokenAuthInterceptor {
    BearerTokenAuthInterceptor::with_labelled_tokens([("t", "alice")])
}

/// The four declarations the profile needs.
#[cfg(all(feature = "audit", feature = "encryption"))]
fn declared(b: RequestHandlerBuilder) -> RequestHandlerBuilder {
    b.declare_external_control(
        ExternalControl::TrustedProxies,
        "none: TLS ends at the pod; no forwarded header is read",
    )
    .declare_external_control(
        ExternalControl::KeyCustody,
        "KEK in the cloud KMS, rotated yearly",
    )
    .declare_external_control(
        ExternalControl::Backups,
        "nightly, restore rehearsed quarterly",
    )
    .declare_external_control(ExternalControl::Retention, "purge_expired daily, 90 days")
}

#[test]
fn a_blank_declaration_is_refused_without_any_profile() {
    let why = refusal(
        RequestHandlerBuilder::new(Idle)
            .without_agent_card()
            .declare_external_control(ExternalControl::Backups, "  "),
    );
    assert!(
        why.contains("declaration of backups and restore says nothing"),
        "{why}"
    );
}

#[test]
fn regulated_data_is_refused_beside_local_development() {
    let why = refusal(
        RequestHandlerBuilder::new(Idle)
            .without_agent_card()
            .with_profile(Profile::LocalDevelopment)
            .with_profile(Profile::RegulatedData),
    );
    assert!(why.contains("RegulatedData"), "{why}");
}

#[test]
fn an_unauthenticated_handler_is_refused() {
    let why = refusal(
        RequestHandlerBuilder::new(Idle)
            .without_agent_card()
            .with_profile(Profile::RegulatedData),
    );
    assert!(why.contains("authenticating interceptor"), "{why}");
}

/// Without the features the profile needs, the refusal says which.
#[cfg(not(all(feature = "audit", feature = "encryption")))]
#[test]
fn without_audit_and_encryption_the_profile_names_the_features() {
    let why = refusal(
        RequestHandlerBuilder::new(Idle)
            .without_agent_card()
            .with_profile(Profile::RegulatedData)
            .with_interceptor(authenticated()),
    );
    assert!(why.contains("`audit` and `encryption` features"), "{why}");
}

#[cfg(all(feature = "audit", feature = "encryption"))]
mod complete {
    use std::sync::Arc;

    use super::*;
    use crate::audit::{AuditLog, AuditPrivacy, HMAC_KEY_LEN, InMemoryAuditStore};
    use crate::push::HttpPushSender;
    use crate::store::InMemoryTaskStore;
    use crate::store::encrypted::{AES_256_KEY_LEN, Aes256GcmCipher, EncryptedTaskStore};

    fn cipher() -> Arc<Aes256GcmCipher> {
        Arc::new(Aes256GcmCipher::new("k", [3; AES_256_KEY_LEN]).unwrap())
    }

    fn log(required: bool, private: bool) -> Arc<AuditLog> {
        let mut log = AuditLog::new(Arc::new(InMemoryAuditStore::new())).require_record(required);
        if private {
            log = log
                .with_privacy(AuditPrivacy::new(cipher(), "h1", [4; HMAC_KEY_LEN]).unwrap())
                .unwrap();
        }
        Arc::new(log)
    }

    /// Everything the profile needs, but for what each test leaves out.
    fn regulated() -> RequestHandlerBuilder {
        declared(
            RequestHandlerBuilder::new(Idle)
                .without_agent_card()
                .with_profile(Profile::RegulatedData)
                .with_interceptor(authenticated())
                .share_tasks_across_callers()
                .with_audit(log(true, true))
                .with_task_store(EncryptedTaskStore::new(InMemoryTaskStore::new(), cipher()))
                .refuse_private_file_urls(),
        )
    }

    #[test]
    fn a_complete_configuration_builds_and_reports_its_declarations() {
        let h = regulated()
            .with_push_sender(HttpPushSender::new().https_only())
            .build()
            .expect("everything it needs is there");
        let r = h.profile_report();
        assert_eq!(r.profiles, ["RegulatedData"]);
        assert_eq!(r.declared.len(), 4, "{r}");
        assert!(
            r.not_verifiable.contains(&"each declaration is true"),
            "{r}"
        );
        let text = r.to_string();
        assert!(
            text.contains("declared, not verified: key custody: KEK in the cloud KMS"),
            "{text}"
        );
    }

    #[test]
    fn the_audit_log_must_be_set_required_and_private() {
        let unaudited = declared(
            RequestHandlerBuilder::new(Idle)
                .without_agent_card()
                .with_profile(Profile::RegulatedData)
                .with_interceptor(authenticated()),
        );
        assert!(refusal(unaudited).contains("needs an audit log (with_audit)"));
        let why = refusal(regulated().with_audit(log(false, true)));
        assert!(why.contains("require_record"), "{why}");
        let why = refusal(regulated().with_audit(log(true, false)));
        assert!(why.contains("with_privacy"), "{why}");
    }

    #[test]
    fn content_must_be_sealed_or_declared_encrypted_at_rest() {
        let plain = || regulated().with_task_store(InMemoryTaskStore::new());
        let why = refusal(plain());
        assert!(why.contains("sealed at rest"), "{why}");
        plain()
            .declare_external_control(ExternalControl::EncryptionAtRest, "LUKS on every volume")
            .build()
            .expect("declared encrypted at rest");
    }

    #[test]
    fn egress_must_refuse_private_addresses_and_plain_http() {
        let open_files = declared(
            RequestHandlerBuilder::new(Idle)
                .without_agent_card()
                .with_profile(Profile::RegulatedData)
                .with_interceptor(authenticated())
                .share_tasks_across_callers()
                .with_audit(log(true, true))
                .with_task_store(EncryptedTaskStore::new(InMemoryTaskStore::new(), cipher())),
        );
        assert!(refusal(open_files).contains("refuse_private_file_urls"));
        let why = refusal(
            regulated().with_push_sender(HttpPushSender::new().https_only().allow_private_urls()),
        );
        assert!(why.contains("allows private addresses"), "{why}");
        let why = refusal(regulated().with_push_sender(HttpPushSender::new()));
        assert!(why.contains("plain HTTP"), "{why}");
    }

    #[test]
    fn each_external_control_must_be_declared() {
        for (left_out, name) in [
            (ExternalControl::TrustedProxies, "trusted proxies"),
            (ExternalControl::KeyCustody, "key custody"),
            (ExternalControl::Backups, "backups and restore"),
            (ExternalControl::Retention, "retention"),
        ] {
            let mut b = regulated();
            b.external_controls.retain(|(c, _)| *c != left_out);
            let why = refusal(b);
            assert!(
                why.contains(&format!("needs a declaration of {name}")),
                "{why}"
            );
        }
    }

    #[test]
    fn a_control_declared_twice_keeps_the_last_declaration() {
        let h = regulated()
            .declare_external_control(ExternalControl::Backups, "hourly snapshots")
            .build()
            .unwrap();
        let backups: Vec<_> = h
            .profile_report()
            .declared
            .iter()
            .filter(|(c, _)| *c == ExternalControl::Backups)
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(backups[0].1, "hourly snapshots");
    }

    /// The encrypted store says it seals, and the audited wrapper asks the
    /// store it wraps.
    #[test]
    fn sealing_is_answered_by_the_store_and_forwarded() {
        use crate::store::TaskStore;
        assert!(EncryptedTaskStore::new(InMemoryTaskStore::new(), cipher()).seals_content());
        assert!(!InMemoryTaskStore::new().seals_content());
        let audited = crate::audit::AuditedTaskStore {
            inner: Arc::new(EncryptedTaskStore::new(InMemoryTaskStore::new(), cipher())),
            log: log(false, false),
            metrics: Arc::new(crate::metrics::NoopMetrics),
        };
        assert!(
            audited.seals_content(),
            "the audited wrapper does not forward"
        );
    }

    /// The traced wrapper answers as the store it wraps, either way.
    #[cfg(all(feature = "tracing", feature = "sqlite"))]
    #[tokio::test]
    async fn the_traced_wrapper_answers_as_the_store_it_wraps() {
        use crate::store::SqliteTaskStore;
        use crate::store::traced::TracedTaskStore;
        let sqlite = || async { SqliteTaskStore::new("sqlite::memory:").await.unwrap() };
        let plain = TracedTaskStore::wrap(Arc::new(sqlite().await));
        assert!(plain.database().is_some(), "not wrapped");
        assert!(!plain.seals_content());
        let sealed =
            TracedTaskStore::wrap(Arc::new(EncryptedTaskStore::new(sqlite().await, cipher())));
        assert!(
            sealed.seals_content(),
            "the traced wrapper does not forward"
        );
    }
}
