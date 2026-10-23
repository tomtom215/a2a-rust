// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Kill-switch conformance: grading a [`HaltStore`], and the halt it
//! carries between replicas (ADR 0019, ADR 0024).
//!
//! A halt is the operator's last control over agents that misbehave, so a
//! store that loses one, or keeps one it was told to remove, fails at the
//! worst moment. Adopters write their own stores; this grades one the way
//! [`check`](super::check) grades an executor.
//!
//! ```rust,ignore
//! #[tokio::test]
//! async fn my_halt_store_conforms() {
//!     // Each call opens the same backing store, as a second replica would,
//!     // or this one after a restart. Start from an empty one.
//!     let open = || async { Arc::new(MyHaltStore::connect(URL).await) as Arc<dyn HaltStore> };
//!     check_store(&open).await.assert_pass();
//!     check_handlers(&open).await.assert_pass();
//! }
//! ```
//!
//! - [`check_store`] grades the [`HaltStore`] contract: a write replaces the
//!   halt of its scope, a removal removes exactly its scope, scopes do not
//!   collide, another handle (a replica, a restarted process) reads what
//!   was written, and a store that pushes delivers each write and each
//!   cancel to the other handle.
//! - [`check_handlers`] builds handlers on the store, as replicas, and grades
//!   the kill switch end to end: a halt stops the tasks running on this
//!   replica and on another, refuses new work on both, survives a restart,
//!   is lifted everywhere by a resume on any replica, and a tenant's halt
//!   spares the other tenants.
//!
//! # What it cannot tell you
//!
//! It cannot make the store fail, so the rule that a store which cannot be
//! read answers with an error and not with no halts is not graded. It uses
//! its own poll interval (50 ms) and its own executor, which honours
//! cancellation, so it grades the store and the handler's wiring, not your
//! interval or whether your executor stops when told; the executor harness
//! ([`check`](super::check)) grades that. It runs each check once, so it
//! cannot find a race. A pushed signal is graded by its arrival after a
//! write: a store whose listener announces itself with a spurious signal,
//! late, could pass a push check it would fail; the harness waits half a
//! second for such signals first.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::error::A2aResult;

use super::{CheckResult, Report};
use crate::handler::HaltScope;
use crate::store::{HaltSignal, HaltStore, StoredHalt};

mod handlers;
#[cfg(test)]
mod tests;

pub use handlers::check_handlers;

/// How long a check waits for something to happen before it fails.
const BOUND: Duration = Duration::from_secs(10);

/// An adopter's store behind an `Arc`, as a [`HaltStore`] the builder takes.
pub(super) struct Shared(pub(super) Arc<dyn HaltStore>);

impl HaltStore for Shared {
    fn load(&self) -> Pin<Box<dyn Future<Output = A2aResult<Vec<StoredHalt>>> + Send + '_>> {
        self.0.load()
    }
    fn put<'a>(
        &'a self,
        halt: &'a StoredHalt,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.0.put(halt)
    }
    fn remove<'a>(
        &'a self,
        scope: &'a HaltScope,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        self.0.remove(scope)
    }
    fn signals(&self) -> Option<tokio::sync::broadcast::Receiver<HaltSignal>> {
        self.0.signals()
    }
    fn publish_cancel<'a>(
        &'a self,
        tenant: &'a str,
        task: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.0.publish_cancel(tenant, task)
    }
}

/// The halts `store` holds, sorted, or the error as text.
async fn loaded(store: &dyn HaltStore) -> Result<Vec<StoredHalt>, String> {
    let mut halts = store
        .load()
        .await
        .map_err(|e| format!("load failed: {e}"))?;
    halts.sort_by_key(|h| format!("{:?}", h.scope));
    Ok(halts)
}

fn sorted(mut halts: Vec<StoredHalt>) -> Vec<StoredHalt> {
    halts.sort_by_key(|h| format!("{:?}", h.scope));
    halts
}

/// Grades the [`HaltStore`] that `open` opens. Each call to `open` must open
/// the same backing store, starting empty, as another replica would, or
/// this process after a restart. The store is left empty.
pub async fn check_store<F, Fut>(open: &F) -> Report
where
    F: Fn() -> Fut + Sync,
    Fut: Future<Output = Arc<dyn HaltStore>> + Send,
{
    let store = open().await;
    let halts = Halts::new();
    let mut results = vec![fresh(&*store).await];
    let (written, expected) = writes(&*store, &halts).await;
    results.extend(written);
    let reopened = open().await;
    results.push(reads_elsewhere(&*reopened, &expected).await);
    results.extend(removals(&*store, &*reopened, &halts).await);
    results.extend(pushes(&*store, &*reopened).await);
    Report {
        subject: "halt store",
        results,
    }
}

/// The halts the store checks write.
struct Halts {
    all: StoredHalt,
    acme: StoredHalt,
    /// A tenant named like the other scope: a store that keys scopes by a
    /// bare string makes the two one halt.
    named_all: StoredHalt,
    replaced: StoredHalt,
}

impl Halts {
    fn new() -> Self {
        Self {
            all: StoredHalt::new(HaltScope::All, "incident", "oncall"),
            acme: StoredHalt::new(HaltScope::Tenant("acme".into()), "spend", "alice"),
            named_all: StoredHalt::new(HaltScope::Tenant("all".into()), "odd name", "bob"),
            replaced: StoredHalt::new(HaltScope::Tenant("acme".into()), "spend, again", "carol"),
        }
    }
}

async fn fresh(store: &dyn HaltStore) -> CheckResult {
    const NAME: &str = "a_fresh_store_is_empty";
    match loaded(store).await {
        Ok(h) if h.is_empty() => CheckResult::pass(NAME, "no halts"),
        Ok(h) => CheckResult::fail(
            NAME,
            format!("{} halt(s) already stored; grade an empty store", h.len()),
        ),
        Err(e) => CheckResult::fail(NAME, e),
    }
}

/// Writes four halts, the last replacing the second, and grades what is
/// loaded; returns the results and the halts the store should now hold.
async fn writes(store: &dyn HaltStore, h: &Halts) -> (Vec<CheckResult>, Vec<StoredHalt>) {
    const REPLACES: &str = "a_put_replaces_the_halt_of_its_scope";
    const COLLIDE: &str = "scopes_do_not_collide";
    let mut wrote = Ok(());
    for halt in [&h.all, &h.acme, &h.named_all, &h.replaced] {
        if let Err(e) = store.put(halt).await {
            wrote = Err(format!("put failed: {e}"));
        }
    }
    let expected = sorted(vec![h.all.clone(), h.replaced.clone(), h.named_all.clone()]);
    let after = match wrote {
        Ok(()) => loaded(store).await,
        Err(e) => Err(e),
    };
    let replaces = match &after {
        Ok(got) if *got == expected => {
            CheckResult::pass(REPLACES, "the second halt of a tenant replaced the first")
        }
        Ok(got) => CheckResult::fail(REPLACES, format!("expected {expected:?}, loaded {got:?}")),
        Err(e) => CheckResult::fail(REPLACES, e.clone()),
    };
    let collide = match &after {
        Ok(got)
            if got.iter().any(|x| x.scope == HaltScope::All)
                && got
                    .iter()
                    .any(|x| x.scope == HaltScope::Tenant("all".into())) =>
        {
            CheckResult::pass(
                COLLIDE,
                "the halt of everything and the halt of a tenant named `all` are two halts",
            )
        }
        Ok(got) => CheckResult::fail(
            COLLIDE,
            format!("the halt of everything and of tenant `all` were merged: {got:?}"),
        ),
        Err(e) => CheckResult::fail(COLLIDE, e.clone()),
    };
    (vec![replaces, collide], expected)
}

async fn reads_elsewhere(reopened: &dyn HaltStore, expected: &[StoredHalt]) -> CheckResult {
    const NAME: &str = "another_handle_reads_what_was_written";
    match loaded(reopened).await {
        Ok(h) if h == expected => {
            CheckResult::pass(NAME, "a second handle on the store loaded the same halts")
        }
        Ok(h) => CheckResult::fail(
            NAME,
            format!("written {expected:?}, another handle loaded {h:?}"),
        ),
        Err(e) => CheckResult::fail(NAME, e),
    }
}

/// Removes a tenant's halt twice, then the rest, and grades what each
/// handle loads after.
async fn removals(store: &dyn HaltStore, reopened: &dyn HaltStore, h: &Halts) -> Vec<CheckResult> {
    const EXACT: &str = "a_remove_removes_exactly_its_scope";
    const SEEN: &str = "a_removal_is_seen_by_another_handle";
    let tenant = HaltScope::Tenant("acme".into());
    let removal = async {
        let first = store.remove(&tenant).await.map_err(|e| e.to_string())?;
        let second = store.remove(&tenant).await.map_err(|e| e.to_string())?;
        let left = loaded(store).await?;
        Ok::<_, String>((first, second, left))
    }
    .await;
    let rest = sorted(vec![h.all.clone(), h.named_all.clone()]);
    let exact = match removal {
        Ok((true, false, left)) if left == rest => {
            CheckResult::pass(EXACT, "true, then false, and the other halts stayed")
        }
        Ok((first, second, left)) => CheckResult::fail(
            EXACT,
            format!(
                "remove answered {first} then {second} (want true then false); left {left:?}, want {rest:?}"
            ),
        ),
        Err(e) => CheckResult::fail(EXACT, e),
    };
    let emptied = async {
        for scope in [HaltScope::All, HaltScope::Tenant("all".into())] {
            store.remove(&scope).await.map_err(|e| e.to_string())?;
        }
        loaded(reopened).await
    }
    .await;
    let seen = match emptied {
        Ok(left) if left.is_empty() => {
            CheckResult::pass(SEEN, "the other handle loads no halts once all are removed")
        }
        Ok(left) => CheckResult::fail(
            SEEN,
            format!("all removed, another handle still loads {left:?}"),
        ),
        Err(e) => CheckResult::fail(SEEN, e),
    };
    vec![exact, seen]
}

/// The push checks: what `writer` writes, `reader`'s subscription is told.
async fn pushes(writer: &dyn HaltStore, reader: &dyn HaltStore) -> Vec<CheckResult> {
    let Some(mut rx) = reader.signals() else {
        let why = "the store does not push: handlers sharing it learn of a halt by polling";
        return vec![
            CheckResult::skip("a_write_is_pushed_to_another_handle", why),
            CheckResult::skip("a_cancel_is_pushed_to_another_handle", why),
        ];
    };
    // Settle: a listener may announce itself with a signal of its own.
    let _ = tokio::time::timeout(Duration::from_millis(500), async {
        while rx.recv().await.is_ok() {}
    })
    .await;
    while rx.try_recv().is_ok() {}

    let scope = HaltScope::Tenant("conformance-push".into());
    let mut out = Vec::new();
    let write = async {
        writer
            .put(&StoredHalt::new(scope.clone(), "push", "conformance"))
            .await
            .map_err(|e| format!("put failed: {e}"))?;
        next(&mut rx, |s| *s == HaltSignal::Changed)
            .await
            .ok_or("no signal within 10 s of a put")?;
        writer
            .remove(&scope)
            .await
            .map_err(|e| format!("remove failed: {e}"))?;
        next(&mut rx, |s| *s == HaltSignal::Changed)
            .await
            .ok_or("no signal within 10 s of a remove")?;
        Ok::<_, String>(())
    }
    .await;
    out.push(match write {
        Ok(()) => CheckResult::pass(
            "a_write_is_pushed_to_another_handle",
            "a put and a remove each signalled the other handle",
        ),
        Err(e) => CheckResult::fail("a_write_is_pushed_to_another_handle", e),
    });

    out.push(pushed_cancel(writer, &mut rx).await);
    out
}

/// A cancel published on `writer` reaches `rx`, another handle's
/// subscription.
async fn pushed_cancel(
    writer: &dyn HaltStore,
    rx: &mut tokio::sync::broadcast::Receiver<HaltSignal>,
) -> CheckResult {
    let want = HaltSignal::Cancel {
        tenant: "conformance".to_owned(),
        task: "task-1".to_owned(),
    };
    let cancel = async {
        writer
            .publish_cancel("conformance", "task-1")
            .await
            .map_err(|e| format!("publish_cancel failed: {e}"))?;
        next(rx, |s| *s == want)
            .await
            .ok_or_else(|| "the cancel did not reach the other handle within 10 s".to_owned())
    }
    .await;
    match cancel {
        Ok(_) => CheckResult::pass(
            "a_cancel_is_pushed_to_another_handle",
            "the other handle was told which task to stop",
        ),
        Err(e) => CheckResult::fail("a_cancel_is_pushed_to_another_handle", e),
    }
}

/// The first signal matching `want` within [`BOUND`], skipping others.
async fn next(
    rx: &mut tokio::sync::broadcast::Receiver<HaltSignal>,
    want: impl Fn(&HaltSignal) -> bool + Send + Sync,
) -> Option<HaltSignal> {
    tokio::time::timeout(BOUND, async {
        loop {
            match rx.recv().await {
                Ok(s) if want(&s) => return Some(s),
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
    .await
    .ok()
    .flatten()
}
