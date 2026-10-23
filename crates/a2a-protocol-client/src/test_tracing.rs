// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! One global `tracing` subscriber for this crate's tests, installed once and
//! never removed.
//!
//! `tracing` caches each callsite's interest, and the global maximum level,
//! process-wide, and recomputes them from whichever subscribers exist at the
//! time. A test that installs only a scoped (`with_default`) subscriber can
//! therefore find its span or event disabled by a computation another test's
//! thread made without it: `plaintext_tests.rs` met this on its warning
//! (CI runs 39245951120, 39320253336), and `call_span/tests.rs` on the call
//! span, which came back disabled in 7 of 300 local runs of the lib tests
//! (macOS CI on `b398d31a`). Rebuilding the cache after installing a scoped
//! subscriber was not enough: still 4 in 300, because a rebuild running
//! concurrently on another thread can store a result computed before the
//! scoped subscriber registered. A global subscriber that enables
//! everything takes part in every computation from the moment it is
//! installed, so none can disable a callsite again; scoped subscribers then
//! see every span and event they ask for.

#![cfg(feature = "tracing")]

use std::cell::Cell;
use std::sync::OnceLock;

use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

thread_local! {
    /// WARN events seen on this thread while armed; `None` when not.
    static WARNINGS: Cell<Option<usize>> = const { Cell::new(None) };
}

struct CountThisThread;

impl<S: tracing::Subscriber> Layer<S> for CountThisThread {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        if *event.metadata().level() == tracing::Level::WARN {
            WARNINGS.with(|c| c.set(c.get().map(|n| n + 1)));
        }
    }
}

/// Installs the global subscriber, once, then rebuilds the interest cache.
///
/// The rebuild closes the window after `Dispatch::new` registers the
/// subscriber and before `set_global_default` marks it initialized, in which
/// a callsite registering on another thread takes its interest from that
/// thread's default alone (tracing-core 0.1.36, `Rebuilder::JustOne`).
pub fn install() {
    static INSTALLED: OnceLock<Result<(), String>> = OnceLock::new();
    INSTALLED
        .get_or_init(|| {
            tracing::subscriber::set_global_default(
                tracing_subscriber::registry().with(CountThisThread),
            )
            .map_err(|e| e.to_string())
        })
        .as_ref()
        .expect("the test subscriber is the global one");
    tracing::callsite::rebuild_interest_cache();
}

/// The WARN events this thread emits while `f` runs, counted by the global
/// subscriber.
pub fn warnings_in(f: impl FnOnce()) -> usize {
    install();
    WARNINGS.with(|c| c.set(Some(0)));
    f();
    WARNINGS.with(|c| c.replace(None)).expect("armed above")
}
