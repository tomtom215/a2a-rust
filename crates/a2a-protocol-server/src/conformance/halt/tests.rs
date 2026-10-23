// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The harness against deliberately broken stores, each breaking one rule,
//! and against the in-memory store, which keeps them all. A harness whose
//! failures are not themselves tested is a gate that cannot fail.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use a2a_protocol_types::error::A2aResult;

use super::{check_handlers, check_store};
use crate::conformance::{Outcome, Report};
use crate::handler::HaltScope;
use crate::store::{HaltSignal, HaltStore, InMemoryHaltStore, StoredHalt};

/// How a [`Broken`] store breaks the contract.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flaw {
    /// None: a correct store kept in a `Vec`.
    None,
    /// A put adds a halt instead of replacing the one of its scope.
    Appends,
    /// Scopes are keyed by a bare name: tenant `all` is everything.
    Collides,
    /// A remove says it removed, and removes nothing.
    KeepsRemoved,
    /// Offers a subscription and never sends on it.
    Mute,
    /// A tenant's halt is stored as a halt on everything.
    Widens,
    /// A remove removes every halt, answering true only the first time.
    RemovesAll,
    /// A halt on everything is never kept.
    DropsAll,
    /// Pushes writes, and a cancel for a task other than the one published.
    WrongCancel,
}

/// A store in a shared `Vec`, broken as `flaw` says.
#[derive(Clone)]
struct Broken {
    halts: Arc<Mutex<Vec<StoredHalt>>>,
    flaw: Flaw,
    signals: tokio::sync::broadcast::Sender<HaltSignal>,
}

impl Broken {
    fn new(flaw: Flaw) -> Self {
        Self {
            halts: Arc::default(),
            flaw,
            signals: tokio::sync::broadcast::channel(16).0,
        }
    }

    fn key(&self, scope: &HaltScope) -> String {
        match (scope, self.flaw) {
            (HaltScope::All, Flaw::Collides) => "all".to_owned(),
            (HaltScope::Tenant(t), Flaw::Collides) => t.clone(),
            (scope, _) => format!("{scope:?}"),
        }
    }
}

impl HaltStore for Broken {
    fn load(&self) -> Pin<Box<dyn Future<Output = A2aResult<Vec<StoredHalt>>> + Send + '_>> {
        let halts = self.halts.lock().unwrap().clone();
        Box::pin(async move { Ok(halts) })
    }
    fn put<'a>(
        &'a self,
        halt: &'a StoredHalt,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        let mut halt = halt.clone();
        if self.flaw == Flaw::Widens {
            halt.scope = HaltScope::All;
        }
        if self.flaw == Flaw::DropsAll && halt.scope == HaltScope::All {
            return Box::pin(async { Ok(()) });
        }
        {
            let mut halts = self.halts.lock().unwrap();
            if self.flaw != Flaw::Appends {
                let key = self.key(&halt.scope);
                halts.retain(|h| self.key(&h.scope) != key);
            }
            halts.push(halt);
        }
        if self.flaw == Flaw::WrongCancel {
            let _ = self.signals.send(HaltSignal::Changed);
        }
        Box::pin(async { Ok(()) })
    }
    fn remove<'a>(
        &'a self,
        scope: &'a HaltScope,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        if self.flaw == Flaw::KeepsRemoved {
            return Box::pin(async { Ok(true) });
        }
        if self.flaw == Flaw::RemovesAll {
            let removed = {
                let mut halts = self.halts.lock().unwrap();
                let any = !halts.is_empty();
                halts.clear();
                any
            };
            return Box::pin(async move { Ok(removed) });
        }
        let key = self.key(scope);
        let removed = {
            let mut halts = self.halts.lock().unwrap();
            let before = halts.len();
            halts.retain(|h| self.key(&h.scope) != key);
            halts.len() < before
        };
        if self.flaw == Flaw::WrongCancel {
            let _ = self.signals.send(HaltSignal::Changed);
        }
        Box::pin(async move { Ok(removed) })
    }
    fn signals(&self) -> Option<tokio::sync::broadcast::Receiver<HaltSignal>> {
        matches!(self.flaw, Flaw::Mute | Flaw::WrongCancel).then(|| self.signals.subscribe())
    }
    fn publish_cancel<'a>(
        &'a self,
        tenant: &'a str,
        _task: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        if self.flaw == Flaw::WrongCancel {
            let _ = self.signals.send(HaltSignal::Cancel {
                tenant: tenant.to_owned(),
                task: "some-other-task".to_owned(),
            });
        }
        Box::pin(async { Ok(()) })
    }
}

/// The checks of `report` that failed, by name.
fn failed(report: &Report) -> Vec<&'static str> {
    report
        .results()
        .iter()
        .filter(|r| r.outcome == Outcome::Fail)
        .map(|r| r.name)
        .collect()
}

async fn grade_store(store: Broken) -> Report {
    let open = move || {
        let store = store.clone();
        async move { Arc::new(store) as Arc<dyn HaltStore> }
    };
    check_store(&open).await
}

#[tokio::test]
async fn the_in_memory_store_passes_every_check() {
    let store = InMemoryHaltStore::new();
    let open = move || {
        let store = store.clone();
        async move { Arc::new(store) as Arc<dyn HaltStore> }
    };
    let report = check_store(&open).await;
    report.assert_pass();
    assert_eq!(
        report.graded(),
        report.results().len(),
        "it pushes, so nothing is skipped"
    );
    let h = check_handlers(&open).await;
    h.assert_pass();
}

#[tokio::test]
async fn a_correct_store_that_does_not_push_passes_with_the_push_checks_skipped() {
    let report = grade_store(Broken::new(Flaw::None)).await;
    report.assert_pass();
    assert_eq!(report.results().len() - report.graded(), 2);
}

#[tokio::test]
async fn each_flaw_is_named() {
    for (flaw, named) in [
        (Flaw::Appends, "a_put_replaces_the_halt_of_its_scope"),
        (Flaw::Collides, "scopes_do_not_collide"),
        (Flaw::KeepsRemoved, "a_remove_removes_exactly_its_scope"),
        (Flaw::KeepsRemoved, "a_removal_is_seen_by_another_handle"),
        (Flaw::RemovesAll, "a_remove_removes_exactly_its_scope"),
        (Flaw::DropsAll, "scopes_do_not_collide"),
        (Flaw::WrongCancel, "a_cancel_is_pushed_to_another_handle"),
        (Flaw::Mute, "a_write_is_pushed_to_another_handle"),
    ] {
        let report = grade_store(Broken::new(flaw)).await;
        assert!(!report.is_pass(), "{report}");
        assert!(failed(&report).contains(&named), "{named}: {report}");
    }
}

/// A store that already holds a halt is not graded as fresh. The mutation
/// gate found the emptiness test untested: passing any store survived.
#[tokio::test]
async fn a_store_that_starts_halted_is_not_fresh() {
    let store = Broken::new(Flaw::None);
    store.halts.lock().unwrap().push(StoredHalt::new(
        HaltScope::Tenant("left-over".into()),
        "r",
        "by",
    ));
    let report = grade_store(store).await;
    assert!(
        failed(&report).contains(&"a_fresh_store_is_empty"),
        "{report}"
    );
}

/// A store whose second handle sees nothing the first wrote, as a store
/// kept in each process's memory would across replicas.
#[tokio::test]
async fn a_store_each_handle_keeps_alone_fails_the_cross_replica_checks() {
    let open = || async { Arc::new(Broken::new(Flaw::None)) as Arc<dyn HaltStore> };
    let store = check_store(&open).await;
    assert!(
        failed(&store).contains(&"another_handle_reads_what_was_written"),
        "{store}"
    );
    let handlers = check_handlers(&open).await;
    for named in [
        "a_halt_stops_the_other_replicas_tasks",
        "a_halt_refuses_new_work_on_the_other_replica",
        "a_restarted_replica_comes_back_halted",
    ] {
        assert!(failed(&handlers).contains(&named), "{named}: {handlers}");
    }
    for passed in [
        "a_halt_stops_this_replicas_tasks",
        "a_halt_refuses_new_work_here",
    ] {
        assert!(!failed(&handlers).contains(&passed), "{passed}: {handlers}");
    }
}

/// A tenant's halt that reaches every tenant still refuses that tenant on
/// the other replica; it fails because the other tenants are not spared.
#[tokio::test]
async fn a_tenant_halt_that_halts_everyone_is_caught() {
    let store = Broken::new(Flaw::Widens);
    let open = move || {
        let store = store.clone();
        async move { Arc::new(store) as Arc<dyn HaltStore> }
    };
    let handlers = check_handlers(&open).await;
    assert!(
        failed(&handlers).contains(&"a_tenant_halt_spares_other_tenants"),
        "{handlers}"
    );
}

/// The wrapper the harness builds handlers on passes the store's
/// subscription and cancels through: without them a pushing store is
/// graded as one that polls. The mutation gate found the forwarding
/// untested.
#[tokio::test]
async fn the_shared_store_passes_its_subscription_through() {
    let pushing = super::Shared(Arc::new(InMemoryHaltStore::new()));
    let mut rx = pushing.signals().expect("a pushing store");
    pushing.publish_cancel("t1", "task-9").await.unwrap();
    assert_eq!(
        rx.try_recv().expect("the cancel was pushed"),
        HaltSignal::Cancel {
            tenant: "t1".to_owned(),
            task: "task-9".to_owned()
        }
    );
    let polling = super::Shared(Arc::new(Broken::new(Flaw::None)));
    assert!(polling.signals().is_none());
}
