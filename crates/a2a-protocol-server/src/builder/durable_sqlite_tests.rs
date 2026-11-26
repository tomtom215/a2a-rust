// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The durable profile over SQLite stores: what over a file is durable and
//! shared, and what in memory is refused. Split from `durable_tests.rs` at
//! the 500-line rule.

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

/// COR-09: a lease store over a SQLite file is durable, and the refusal of
/// one in memory names it.
#[tokio::test]
async fn a_lease_store_over_a_sqlite_file_builds_and_one_in_memory_does_not() {
    let db = Db::new();
    let leases = Db::new();
    on_file(&db)
        .await
        .with_lease_store(
            crate::store::SqliteLeaseStore::new(&leases.url())
                .await
                .unwrap(),
        )
        .build()
        .expect("a lease store over a file");
    let why = refusal(
        on_file(&db).await.with_lease_store(
            crate::store::SqliteLeaseStore::new("sqlite::memory:")
                .await
                .unwrap(),
        ),
    );
    assert!(why.contains("the lease store"), "{why}");
    assert!(why.contains("SqliteLeaseStore over a file"), "{why}");
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
    let f = Db::new();
    let shared = crate::store::traced::TracedTaskStore::wrap(Arc::new(
        SqliteTaskStore::new(&f.url()).await.unwrap(),
    ));
    assert!(
        !shared.is_process_local(),
        "the traced wrapper calls a shared store process-local"
    );
}

/// An encrypted store over a database file is not in memory: the wrapper
/// asks the store it wraps, both ways.
#[cfg(feature = "encryption")]
#[tokio::test]
async fn an_encrypted_store_over_a_file_is_shared() {
    use crate::store::TaskStore;
    use crate::store::encrypted::{AES_256_KEY_LEN, Aes256GcmCipher, EncryptedTaskStore};
    let f = Db::new();
    let cipher = Arc::new(Aes256GcmCipher::new("k", [1; AES_256_KEY_LEN]).unwrap());
    let sealed = EncryptedTaskStore::new(SqliteTaskStore::new(&f.url()).await.unwrap(), cipher);
    assert!(!sealed.is_process_local());
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
    // And over a store every replica shares, it says so.
    let shared = crate::audit::AuditedTaskStore {
        inner: Arc::new(SqliteTaskStore::new(&f.url()).await.unwrap()),
        ..audited
    };
    assert!(
        !shared.is_process_local(),
        "the audited wrapper calls a shared store process-local"
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
            let pkcs8 =
                ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
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
