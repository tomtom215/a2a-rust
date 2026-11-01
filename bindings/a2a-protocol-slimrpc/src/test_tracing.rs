// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! One global `tracing` subscriber for this crate's unit tests, installed once
//! and never removed.
//!
//! `tracing` caches each callsite's interest, and the global maximum level,
//! process-wide, and recomputes them from whichever subscribers exist at the
//! time. A test that installs only a scoped (`set_default`) subscriber can
//! therefore find its span disabled by a computation another test's thread
//! made without it. `an_answer_the_wire_cannot_carry_fails_its_span` met this:
//! the call was recorded, but the span recorded no field, in 4 of 100 runs of
//! the library tests and in none of 200 runs of that test alone. Rebuilding
//! the cache after installing the scoped subscriber is not enough, since a
//! rebuild running at the same moment on another thread can store a result
//! computed before the scoped subscriber registered. A global subscriber that
//! enables everything takes part in every computation from the moment it is
//! installed, so none can disable a callsite again; scoped subscribers then
//! see every span they ask for. The client crate's `test_tracing` module
//! records the same fault and the same remedy.

use std::sync::OnceLock;

/// Installs the global subscriber, once, then rebuilds the interest cache.
///
/// The rebuild closes the window after `Dispatch::new` registers the
/// subscriber and before `set_global_default` marks it initialized, in which a
/// callsite registering on another thread takes its interest from that
/// thread's default alone.
pub(crate) fn install() {
    static INSTALLED: OnceLock<Result<(), String>> = OnceLock::new();
    INSTALLED
        .get_or_init(|| {
            tracing::subscriber::set_global_default(tracing_subscriber::registry())
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .expect("the test subscriber is the global one");
    tracing::callsite::rebuild_interest_cache();
}
