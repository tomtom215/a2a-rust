// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `Profile::MultiTenant`: what it applies, and each thing it refuses to build
//! without.

use std::collections::HashMap;

use a2a_protocol_types::params::TaskQueryParams;

use crate::error::ServerError;
use crate::profile::Profile;
use crate::push::{InMemoryPushConfigStore, TenantAwareInMemoryPushConfigStore};
use crate::store::{InMemoryTaskStore, TenantAwareInMemoryTaskStore};
use crate::tenant_config::{PerTenantConfig, TenantLimits};
use crate::tenant_resolver::HeaderTenantResolver;
use crate::{BearerTokenAuthInterceptor, RequestHandlerBuilder, agent_executor};

struct Idle;
agent_executor!(Idle, |_ctx, _q| async { Ok(()) });

fn limits() -> PerTenantConfig {
    PerTenantConfig::default()
        .with_default(TenantLimits::default().with_max_concurrent_tasks(Some(4)))
}

/// Everything the profile asks for.
fn complete() -> RequestHandlerBuilder {
    RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_profile(Profile::MultiTenant)
        .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([(
            "t-alice", "alice",
        )]))
        .with_authorized_tenant_resolver(HeaderTenantResolver::default())
        .with_tenant_config(limits())
}

fn refusal(b: RequestHandlerBuilder) -> String {
    match b.build() {
        Err(ServerError::Config(m)) => m,
        other => panic!("expected a refusal, got {:?}", other.map(|_| ())),
    }
}

fn headers(token: &str, tenant: Option<&str>) -> HashMap<String, String> {
    let mut h = HashMap::from([("authorization".to_owned(), format!("Bearer {token}"))]);
    if let Some(t) = tenant {
        h.insert("x-tenant-id".to_owned(), t.to_owned());
    }
    h
}

async fn get(
    h: &crate::RequestHandler,
    token: &str,
    tenant: Option<&str>,
) -> Result<a2a_protocol_types::task::Task, ServerError> {
    h.on_get_task(
        TaskQueryParams::new("t-1".to_owned()),
        Some(&headers(token, tenant)),
    )
    .await
}

/// A complete configuration serves a named caller in a resolved tenant (its
/// task is merely not there), and refuses what the profile exists to refuse.
#[tokio::test]
async fn the_profile_serves_named_callers_in_resolved_tenants_and_refuses_the_rest() {
    let h = complete().build().expect("everything it needs is there");

    let served = get(&h, "t-alice", Some("acme")).await;
    assert!(
        matches!(served, Err(ServerError::TaskNotFound(_))),
        "{served:?}"
    );

    // No tenant could be found: refused, not served from the shared partition.
    let untenanted = get(&h, "t-alice", None).await;
    assert!(
        matches!(untenanted, Err(ref e) if !matches!(e, ServerError::TaskNotFound(_))),
        "{untenanted:?}"
    );

    // A credential that names nobody is refused, not kept in the tenant's
    // shared records as it is by default.
    let unnamed = RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_profile(Profile::MultiTenant)
        .with_interceptor(BearerTokenAuthInterceptor::new(["anon"]))
        .with_authorized_tenant_resolver(HeaderTenantResolver::default())
        .with_tenant_config(limits())
        .build()
        .unwrap();
    let refused = get(&unnamed, "anon", Some("acme")).await;
    assert!(
        matches!(refused, Err(ServerError::Protocol(_))),
        "{refused:?}"
    );
}

#[test]
fn each_missing_piece_is_named() {
    let anonymous = RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_profile(Profile::MultiTenant)
        .with_authorized_tenant_resolver(HeaderTenantResolver::default())
        .with_tenant_config(limits());
    let why = refusal(anonymous);
    assert!(
        why.contains("the multi-tenant profile needs an authenticating interceptor"),
        "{why}"
    );

    let no_resolver = RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_profile(Profile::MultiTenant)
        .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([(
            "t", "a",
        )]))
        .with_tenant_config(limits());
    assert!(refusal(no_resolver).contains("with_authorized_tenant_resolver"));

    // A resolver the deployment has not asserted is authorized.
    let unasserted = RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_profile(Profile::MultiTenant)
        .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([(
            "t", "a",
        )]))
        .with_tenant_resolver(HeaderTenantResolver::default())
        .with_tenant_config(limits());
    let why = refusal(unasserted);
    assert!(
        why.contains("a tenant the client chose isolates nothing"),
        "{why}"
    );

    let unbudgeted = RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_profile(Profile::MultiTenant)
        .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([(
            "t", "a",
        )]))
        .with_authorized_tenant_resolver(HeaderTenantResolver::default());
    assert!(refusal(unbudgeted).contains("with_tenant_config"));
}

#[test]
fn every_tenant_needs_a_concurrency_limit() {
    let unlimited_default = complete()
        .with_tenant_config(PerTenantConfig::default().with_default(TenantLimits::default()));
    let why = refusal(unlimited_default);
    assert!(why.contains("the default has none"), "{why}");

    let unlimited_override = complete().with_tenant_config(limits().with_override(
        "big-customer",
        TenantLimits::default().with_max_concurrent_tasks(None),
    ));
    let why = refusal(unlimited_override);
    assert!(why.contains("big-customer has none"), "{why}");

    complete()
        .with_tenant_config(limits().with_override(
            "big-customer",
            TenantLimits::default().with_max_concurrent_tasks(Some(64)),
        ))
        .build()
        .expect("an override with its own limit is finite");
}

#[test]
fn stores_that_share_records_across_tenants_are_refused() {
    // Either store alone is enough to share records across tenants, and it is
    // the profile that refuses, before the isolation check every handler has.
    let profile = "the multi-tenant profile needs a task store and a push-config store";
    let shared_task = complete().with_task_store(InMemoryTaskStore::new());
    let shared_push = complete().with_push_config_store(InMemoryPushConfigStore::new());
    let shared_task_only = complete()
        .with_task_store(InMemoryTaskStore::new())
        .with_push_config_store(TenantAwareInMemoryPushConfigStore::new());
    for b in [shared_task, shared_push, shared_task_only] {
        let why = refusal(b);
        assert!(why.starts_with(profile), "{why}");
    }

    let accepted = complete().accept_unisolated_tenants();
    let why = refusal(accepted);
    assert!(why.starts_with(profile), "{why}");
    assert!(why.contains("accept_unisolated_tenants"), "{why}");
}

/// Sharing tasks between callers is allowed when it is chosen and the store
/// keeps tenants apart; the profile does not override the choice.
#[test]
fn sharing_across_callers_is_an_explicit_choice_that_needs_a_tenant_aware_store() {
    let without_store = complete().share_tasks_across_callers();
    let why = refusal(without_store);
    assert!(why.contains("share tasks across callers"), "{why}");

    complete()
        .share_tasks_across_callers()
        .with_task_store(TenantAwareInMemoryTaskStore::new())
        .build()
        .expect("chosen, with a store that partitions by tenant");
}

/// The profile does not override a choice made before it was applied.
#[test]
fn a_choice_to_share_made_before_the_profile_is_kept() {
    let b = RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .share_tasks_across_callers()
        .with_profile(Profile::MultiTenant)
        .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([(
            "t-alice", "alice",
        )]))
        .with_authorized_tenant_resolver(HeaderTenantResolver::default())
        .with_tenant_config(limits());
    // Kept as a choice to share: the default store would then share records
    // across tenants, which the profile names.
    let why = refusal(b);
    assert!(why.contains("share tasks across callers"), "{why}");
}

#[test]
fn without_the_profile_nothing_changes() {
    RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_tenant_resolver(HeaderTenantResolver::default())
        .build()
        .expect("a plain resolver, no limits, no authenticator: as before");
}
