// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A request naming a tenant is refused unless the stores can keep tenants
//! apart.
//!
//! Before this, the default `RequestHandlerBuilder` accepted a `tenant` field
//! and served it from a store that ignores tenants, so tenant B could get,
//! list, subscribe to and cancel tenant A's task. That was measured on
//! 2026-10-02 as five of five cross-tenant probes succeeding
//! (`docs/sdk-comparison-2026-10-02.md` §5.1). Each test below pins one cell
//! of the decision: which stores, which tenant, whether the opt-out is set.

use std::collections::HashMap;

use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;

use crate::agent_executor;
use crate::builder::RequestHandlerBuilder;
use crate::error::ServerError;
use crate::push::InMemoryPushConfigStore;
use crate::store::TenantAwareInMemoryTaskStore;
use crate::tenant_resolver::HeaderTenantResolver;

struct DummyExecutor;
agent_executor!(DummyExecutor, |_ctx, _queue| async { Ok(()) });

fn is_refusal(result: &Result<String, ServerError>) -> bool {
    matches!(result, Err(ServerError::UnsupportedOperation(msg)) if msg.contains("cannot isolate tenants"))
}

#[tokio::test]
async fn the_default_store_refuses_a_client_named_tenant() {
    let handler = RequestHandlerBuilder::new(DummyExecutor).build().unwrap();
    let result = handler.resolve_tenant("GetTask", None, Some("acme")).await;
    assert!(is_refusal(&result), "got {result:?}");
}

/// The empty tenant is the shared partition every store has, so a
/// single-tenant deployment is untouched.
#[tokio::test]
async fn the_default_store_still_serves_requests_without_a_tenant() {
    let handler = RequestHandlerBuilder::new(DummyExecutor).build().unwrap();
    assert_eq!(
        handler.resolve_tenant("GetTask", None, None).await.unwrap(),
        ""
    );
    assert_eq!(
        handler
            .resolve_tenant("GetTask", None, Some(""))
            .await
            .unwrap(),
        ""
    );
}

/// A tenant from a resolver is no safer than one the client named: the
/// store still shares every record.
#[tokio::test]
async fn the_default_store_refuses_a_resolved_tenant() {
    let handler = RequestHandlerBuilder::new(DummyExecutor)
        .with_tenant_resolver(HeaderTenantResolver::default())
        .build()
        .unwrap();
    let mut headers = HashMap::new();
    headers.insert("x-tenant-id".to_owned(), "acme".to_owned());
    let result = handler
        .resolve_tenant("GetTask", Some(&headers), None)
        .await;
    assert!(is_refusal(&result), "got {result:?}");
}

/// Configuring only the task store is enough: the unset push-config store
/// follows it, rather than refusing every tenant over a store nobody chose.
#[tokio::test]
async fn a_tenant_aware_task_store_alone_serves_tenants() {
    let handler = RequestHandlerBuilder::new(DummyExecutor)
        .with_task_store(TenantAwareInMemoryTaskStore::new())
        .build()
        .unwrap();
    assert!(handler.push_config_store.isolates_tenants());
    assert_eq!(
        handler
            .resolve_tenant("GetTask", None, Some("acme"))
            .await
            .unwrap(),
        "acme"
    );
}

/// A push-config store that was chosen and does not isolate still refuses:
/// push configs would be shared even though tasks are not.
#[tokio::test]
async fn an_explicit_shared_push_store_refuses_tenants() {
    let handler = RequestHandlerBuilder::new(DummyExecutor)
        .with_task_store(TenantAwareInMemoryTaskStore::new())
        .with_push_config_store(InMemoryPushConfigStore::new())
        .build()
        .unwrap();
    let result = handler.resolve_tenant("GetTask", None, Some("acme")).await;
    assert!(is_refusal(&result), "got {result:?}");
}

#[tokio::test]
async fn the_opt_out_serves_tenants_from_shared_stores() {
    let handler = RequestHandlerBuilder::new(DummyExecutor)
        .accept_unisolated_tenants()
        .build()
        .unwrap();
    assert_eq!(
        handler
            .resolve_tenant("GetTask", None, Some("acme"))
            .await
            .unwrap(),
        "acme"
    );
}

/// The refusal happens before anything is written: a refused send leaves
/// no task behind for any tenant to find.
#[tokio::test]
async fn a_refused_send_creates_no_task() {
    let handler = RequestHandlerBuilder::new(DummyExecutor).build().unwrap();
    let mut params = MessageSendParams::new(Message::new(
        MessageId::new("m-1"),
        MessageRole::User,
        vec![Part::text("hello")],
    ));
    params.tenant = Some("acme".to_owned());
    let err = handler
        .on_send_message(params, false, None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, ServerError::UnsupportedOperation(_)),
        "got {err:?}"
    );
    assert_eq!(handler.task_store.count().await.unwrap(), 0);
}

/// Each store's own answer. The handler trusts it, so a tenant-aware store
/// that forgot to override would refuse every tenant, and a shared store
/// that claimed isolation would reopen the leak. The Postgres pair is
/// checked in `tests/postgres_store_tests.rs`, which needs a live database.
#[tokio::test]
async fn each_store_reports_whether_it_isolates() {
    use crate::push::{PushConfigStore, TenantAwareInMemoryPushConfigStore};
    use crate::store::{InMemoryTaskStore, TaskStore};
    assert!(TenantAwareInMemoryTaskStore::new().isolates_tenants());
    assert!(TenantAwareInMemoryPushConfigStore::new().isolates_tenants());
    assert!(!InMemoryTaskStore::new().isolates_tenants());
    assert!(!InMemoryPushConfigStore::new().isolates_tenants());
    #[cfg(feature = "sqlite")]
    {
        use crate::push::{SqlitePushConfigStore, TenantAwareSqlitePushConfigStore};
        use crate::store::{SqliteTaskStore, TenantAwareSqliteTaskStore};
        let url = "sqlite::memory:";
        assert!(
            TenantAwareSqliteTaskStore::new(url)
                .await
                .unwrap()
                .isolates_tenants()
        );
        assert!(
            TenantAwareSqlitePushConfigStore::new(url)
                .await
                .unwrap()
                .isolates_tenants()
        );
        assert!(!SqliteTaskStore::new(url).await.unwrap().isolates_tenants());
        assert!(
            !SqlitePushConfigStore::new(url)
                .await
                .unwrap()
                .isolates_tenants()
        );
    }
}
