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

/// The in-memory stores say they keep their contents in this process, the
/// tenant-aware one and the push outbox as well as the plain task store.
#[test]
fn the_in_memory_stores_say_they_are_process_local() {
    use crate::push::PushOutbox;
    use crate::store::TaskStore;
    assert!(InMemoryTaskStore::new().is_process_local());
    assert!(crate::store::TenantAwareInMemoryTaskStore::new().is_process_local());
    assert!(crate::push::InMemoryPushOutbox::new().is_process_local());
}

#[cfg(feature = "sqlite")]
#[cfg(test)]
#[path = "durable_sqlite_tests.rs"]
mod sqlite;

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
