// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `Profile::LocalDevelopment`, `Profile::DurableProduction` and the report of
//! what a handler's profiles guarantee (adversarial review of 2026-11-13).

use crate::profile::Profile;
use crate::store::InMemoryTaskStore;
#[cfg(any(feature = "sqlite", feature = "signing"))]
use crate::store::{InMemoryHaltStore, InMemoryLeaseStore};
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

fn durable() -> RequestHandlerBuilder {
    RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_profile(Profile::DurableProduction)
}

#[test]
fn local_development_builds_in_memory_and_says_what_it_gives_up() {
    let h = RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_profile(Profile::LocalDevelopment)
        .build()
        .expect("nothing is required of a local build");
    let r = h.profile_report();
    assert_eq!(r.profiles, ["LocalDevelopment"]);
    assert_eq!(r.checked, Vec::<&str>::new(), "{r}");
    assert_eq!(r.disabled.len(), 1, "{r}");
    assert_eq!(r.disabled[0].0, "Profile::LocalDevelopment");
    assert!(r.disabled[0].1.contains("a restart loses them"), "{r}");
}

#[test]
fn local_development_is_refused_beside_a_production_profile() {
    for (profile, name) in [
        (Profile::Production, "Production"),
        (Profile::DurableProduction, "DurableProduction"),
    ] {
        let why = refusal(
            RequestHandlerBuilder::new(Idle)
                .without_agent_card()
                .with_profile(Profile::LocalDevelopment)
                .with_profile(profile),
        );
        assert!(why.contains("local-development"), "{why}");
        assert!(why.contains(name), "{why}");
    }
}

#[test]
fn durable_production_names_each_missing_piece() {
    let why = refusal(durable().with_task_store(InMemoryTaskStore::new()));
    assert!(why.contains("authenticating interceptor"), "{why}");

    let why = refusal(durable().with_interceptor(authenticated()));
    assert!(why.contains("with_task_store"), "{why}");

    let why = refusal(
        durable()
            .with_interceptor(authenticated())
            .with_task_store(InMemoryTaskStore::new()),
    );
    assert!(why.contains("the task store"), "{why}");
    assert!(why.contains("a restart loses every task"), "{why}");
}

/// Without the profile, an in-memory build is as it always was, and its
/// report is empty.
#[test]
fn without_a_profile_nothing_changes_and_nothing_is_claimed() {
    let h = RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .build()
        .unwrap();
    assert_eq!(h.profile_report().profiles, Vec::<&str>::new());
    assert_eq!(
        h.profile_report().to_string(),
        "no deployment profile: nothing was checked"
    );
}

/// A wrapper is asked about the store it wraps: an encrypted store over
/// memory is still in memory.
#[cfg(feature = "encryption")]
#[test]
fn a_wrapped_in_memory_store_is_still_refused() {
    use crate::store::encrypted::{AES_256_KEY_LEN, Aes256GcmCipher, EncryptedTaskStore};
    use std::sync::Arc;
    let cipher = Arc::new(Aes256GcmCipher::new("k", [1; AES_256_KEY_LEN]).unwrap());
    let why = refusal(
        durable()
            .with_interceptor(authenticated())
            .with_task_store(EncryptedTaskStore::new(InMemoryTaskStore::new(), cipher)),
    );
    assert!(why.contains("the task store"), "{why}");
}

#[cfg(feature = "sqlite")]
mod sqlite {
    use std::sync::Arc;

    use super::*;
    use crate::push::{InMemoryPushOutbox, SqlitePushOutbox, TenantAwareSqlitePushConfigStore};
    use crate::store::{SqliteHaltStore, SqliteTaskStore, TenantAwareSqliteTaskStore};

    /// A database file of its own for each call, removed by the guard.
    struct Db(std::path::PathBuf);

    impl Db {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!("a2a-durable-{}-{n}.db", std::process::id())))
        }

        fn url(&self) -> String {
            format!("sqlite://{}?mode=rwc", self.0.display())
        }
    }

    impl Drop for Db {
        fn drop(&mut self) {
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
            }
        }
    }

    async fn on_file(db: &Db) -> RequestHandlerBuilder {
        durable()
            .with_interceptor(authenticated())
            .with_task_store(TenantAwareSqliteTaskStore::new(&db.url()).await.unwrap())
    }

    #[tokio::test]
    async fn a_task_store_over_a_file_builds_and_one_in_memory_does_not() {
        let db = Db::new();
        let h = on_file(&db).await.build().expect("a durable task store");
        let r = h.profile_report();
        assert_eq!(r.profiles, ["DurableProduction"]);
        assert!(r.checked.iter().any(|c| c.contains("task store")), "{r}");
        assert!(
            r.not_verifiable.iter().any(|c| c.contains("backed up")),
            "{r}"
        );

        let why = refusal(
            durable().with_interceptor(authenticated()).with_task_store(
                TenantAwareSqliteTaskStore::new("sqlite::memory:")
                    .await
                    .unwrap(),
            ),
        );
        assert!(why.contains("the task store"), "{why}");
    }

    #[tokio::test]
    async fn halts_and_audit_kept_in_memory_are_refused() {
        let db = Db::new();
        let why = refusal(on_file(&db).await.with_halt_store(InMemoryHaltStore::new()));
        assert!(why.contains("the halt store"), "{why}");
        let why = refusal(
            on_file(&db)
                .await
                .with_lease_store(InMemoryLeaseStore::new()),
        );
        assert!(why.contains("the lease store"), "{why}");
        let halts = Db::new();
        on_file(&db)
            .await
            .with_halt_store(SqliteHaltStore::new(&halts.url()).await.unwrap())
            .build()
            .expect("a halt store over a file");
    }

    #[cfg(feature = "audit")]
    #[tokio::test]
    async fn an_audit_store_in_memory_is_refused() {
        use crate::audit::{AuditLog, InMemoryAuditStore};
        let db = Db::new();
        let log = Arc::new(AuditLog::new(Arc::new(InMemoryAuditStore::new())));
        let why = refusal(on_file(&db).await.with_audit(log));
        assert!(why.contains("the audit store"), "{why}");
    }

    fn pushing_card() -> a2a_protocol_types::agent_card::AgentCard {
        let mut card = a2a_protocol_types::agent_card::AgentCard::new(
            "t",
            "0.0.0",
            a2a_protocol_types::agent_card::AgentInterface::jsonrpc("http://127.0.0.1:1"),
        )
        .with_description("A test agent")
        .with_skill(
            a2a_protocol_types::AgentSkill::new("test", "Test", "A test skill").with_tags(["test"]),
        )
        .with_security_schemes(std::collections::HashMap::from([(
            "bearer".to_owned(),
            a2a_protocol_types::security::SecurityScheme::Http(
                a2a_protocol_types::security::HttpAuthSecurityScheme::new("Bearer".to_owned()),
            ),
        )]));
        card.capabilities = card.capabilities.with_push_notifications(true);
        card
    }

    /// A card that promises push needs both the configs and the deliveries
    /// owed to outlive the process.
    #[tokio::test]
    async fn push_needs_a_durable_config_store_and_outbox() {
        let db = Db::new();
        let why = refusal(
            on_file(&db)
                .await
                .with_agent_card(pushing_card())
                .with_push_sender(crate::push::HttpPushSender::new()),
        );
        assert!(
            why.contains("needs a push-config store (with_push_config_store)"),
            "{why}"
        );

        let why = refusal(
            on_file(&db)
                .await
                .with_agent_card(pushing_card())
                .with_push_sender(crate::push::HttpPushSender::new())
                .with_push_config_store(crate::push::InMemoryPushConfigStore::new()),
        );
        assert!(why.contains("found the push-config store"), "{why}");

        let configs = Db::new();
        let with_configs = |b: RequestHandlerBuilder, url: String| async move {
            b.with_agent_card(pushing_card())
                .with_push_sender(crate::push::HttpPushSender::new())
                .with_push_config_store(TenantAwareSqlitePushConfigStore::new(&url).await.unwrap())
        };
        let why = refusal(with_configs(on_file(&db).await, configs.url()).await);
        assert!(
            why.contains("needs a push outbox (with_push_outbox)"),
            "{why}"
        );

        let why = refusal(
            with_configs(on_file(&db).await, configs.url())
                .await
                .with_push_outbox(Arc::new(InMemoryPushOutbox::new())),
        );
        assert!(why.contains("the push outbox"), "{why}");

        let outbox = Db::new();
        with_configs(on_file(&db).await, configs.url())
            .await
            .with_push_outbox(Arc::new(
                SqlitePushOutbox::new(&outbox.url()).await.unwrap(),
            ))
            .build()
            .expect("durable configs and a durable outbox");
    }

    /// Every SQLite store says whether it is in memory, and each wrapper that
    /// forwards to a task store asks the store it wraps.
    #[tokio::test]
    async fn each_sqlite_store_and_wrapper_answers_from_its_database() {
        let memory = answers("sqlite::memory:").await;
        let f = Db::new();
        let file = answers(&f.url()).await;
        for ((name, in_memory), (_, on_file)) in memory.into_iter().zip(file) {
            assert!(
                in_memory,
                "the {name} store over sqlite::memory: says it is shared"
            );
            assert!(
                !on_file,
                "the {name} store over a file says it is in memory"
            );
        }
    }

    /// The traced wrapper, which exists with the `tracing` feature only.
    #[cfg(feature = "tracing")]
    #[tokio::test]
    async fn the_traced_wrapper_asks_the_store_it_wraps() {
        let traced = crate::store::traced::TracedTaskStore::wrap(Arc::new(
            SqliteTaskStore::new("sqlite::memory:").await.unwrap(),
        ));
        assert!(
            traced.is_process_local(),
            "the traced wrapper does not forward"
        );
    }

    /// What each SQLite store over `url` answers.
    async fn answers(url: &str) -> Vec<(&'static str, bool)> {
        use crate::push::{PushConfigStore, PushOutbox, SqlitePushConfigStore};
        use crate::store::{HaltStore, TaskStore};
        let task = SqliteTaskStore::new(url).await.unwrap();
        let tenant_task = TenantAwareSqliteTaskStore::new(url).await.unwrap();
        let halt = SqliteHaltStore::new(url).await.unwrap();
        let configs = SqlitePushConfigStore::new(url).await.unwrap();
        let tenant_configs = TenantAwareSqlitePushConfigStore::new(url).await.unwrap();
        let outbox = SqlitePushOutbox::new(url).await.unwrap();
        vec![
            ("task", task.is_process_local()),
            ("tenant task", tenant_task.is_process_local()),
            ("halt", halt.is_process_local()),
            ("push config", configs.is_process_local()),
            ("tenant push config", tenant_configs.is_process_local()),
            ("outbox", outbox.is_process_local()),
        ]
    }

    #[cfg(feature = "audit")]
    #[tokio::test]
    async fn the_audit_store_and_the_audited_wrapper_answer_too() {
        use crate::audit::{AuditLog, AuditStore, SqliteAuditStore};
        use crate::store::TaskStore;
        let f = Db::new();
        assert!(
            SqliteAuditStore::new("sqlite::memory:")
                .await
                .unwrap()
                .is_process_local()
        );
        assert!(
            !SqliteAuditStore::new(&f.url())
                .await
                .unwrap()
                .is_process_local()
        );
        let audited = crate::audit::AuditedTaskStore {
            inner: Arc::new(InMemoryTaskStore::new()),
            log: Arc::new(AuditLog::new(Arc::new(
                crate::audit::InMemoryAuditStore::new(),
            ))),
            metrics: Arc::new(crate::metrics::NoopMetrics),
        };
        assert!(
            audited.is_process_local(),
            "the audited wrapper does not forward"
        );
    }

    /// An escape hatch is reported with what it gives up.
    #[tokio::test]
    async fn sharing_tasks_across_callers_is_reported() {
        let db = Db::new();
        let h = on_file(&db)
            .await
            .share_tasks_across_callers()
            .build()
            .unwrap();
        let r = h.profile_report();
        assert!(
            r.disabled
                .iter()
                .any(|(hatch, _)| *hatch == "share_tasks_across_callers()"),
            "{r}"
        );
        assert!(
            r.to_string()
                .contains("disabled by share_tasks_across_callers()"),
            "{r}"
        );
    }

    /// `sqlite::memory:` hands out turn claims but no other process can see
    /// it, so the production profile does not hold it to the rules of a
    /// shared task store. Before the task store said it was process-local,
    /// an in-memory halt store beside it was refused as if it were shared.
    #[tokio::test]
    async fn an_in_memory_sqlite_task_store_is_not_taken_for_a_shared_one() {
        let b = RequestHandlerBuilder::new(Idle)
            .without_agent_card()
            .with_profile(Profile::Production)
            .with_interceptor(authenticated())
            .with_task_store(SqliteTaskStore::new("sqlite::memory:").await.unwrap())
            .with_halt_store(InMemoryHaltStore::new())
            .with_lease_store(InMemoryLeaseStore::new())
            .share_tasks_across_callers();
        // What is left to refuse is the missing signer, not the halt store.
        if cfg!(feature = "signing") {
            #[cfg(feature = "signing")]
            {
                let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(
                    &ring::rand::SystemRandom::new(),
                )
                .unwrap();
                let signer =
                    a2a_protocol_types::provenance::ContentSigner::ed25519("k", pkcs8.as_ref())
                        .unwrap();
                b.with_content_signer(signer)
                    .build()
                    .expect("not a shared store");
            }
        } else {
            let why = refusal(b);
            assert!(why.contains("content signer"), "{why}");
        }
    }
}

/// A hatch that turns a guarantee off removes it from what was checked, and
/// is reported with what it gives up.
#[cfg(feature = "signing")]
#[test]
fn a_disabled_guarantee_is_not_also_listed_as_checked() {
    let pkcs8 =
        ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    let signer =
        || a2a_protocol_types::provenance::ContentSigner::ed25519("k", pkcs8.as_ref()).unwrap();
    let production = || {
        RequestHandlerBuilder::new(Idle)
            .without_agent_card()
            .with_profile(Profile::Production)
            .with_interceptor(authenticated())
            .with_halt_store(InMemoryHaltStore::new())
            .with_lease_store(InMemoryLeaseStore::new())
            .with_content_signer(signer())
    };
    let shared_rule = |c: &&str| c.starts_with("over a task store several");
    let strict = production().build().unwrap();
    assert!(strict.profile_report().checked.iter().any(shared_rule));
    assert_eq!(strict.profile_report().disabled, Vec::new());

    let relaxed = production().allow_process_local_stores().build().unwrap();
    let r = relaxed.profile_report();
    assert!(!r.checked.iter().any(shared_rule), "{r}");
    assert_eq!(r.disabled.len(), 1, "{r}");
    assert_eq!(r.disabled[0].0, "allow_process_local_stores()");
}
