// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `Telemetry::install_subscriber` installs the telemetry's layer, with a
//! layer that prints warnings to standard error, as the global `tracing`
//! subscriber, and never replaces one already set. Its own binary because the
//! global subscriber is process state.
//!
//! Until 0.16 it installed nothing when no trace or log provider was built, so
//! "a process run without a collector keeps no subscriber at all". That left a
//! process with no collector showing nothing — not its own warnings, not how to
//! turn export on — so it now always installs one (except under
//! `OTEL_SDK_DISABLED`, which `src/otel/telemetry/tests.rs` covers).

#![cfg(feature = "otlp")]

use a2a_protocol_server::otel::Telemetry;

#[test]
fn installs_once_whatever_was_built_and_never_replaces_one() {
    // Nothing exported at all: still a subscriber, for the warnings and the
    // line saying how to turn export on.
    let off = Telemetry::builder()
        .with_traces(false)
        .with_metrics(false)
        .with_logs(false)
        .install_globally(false)
        .build()
        .expect("builds");
    assert!(off.describe().starts_with("telemetry: nothing is exported"));
    assert!(!tracing::dispatcher::has_been_set());
    assert!(
        off.install_subscriber(),
        "a process with nothing exported still gets its warnings printed"
    );
    assert!(tracing::dispatcher::has_been_set());

    // Whatever else is built after that finds the slot taken, and says so by
    // returning false; it does not replace it.
    let traces = Telemetry::builder()
        .with_otlp_endpoint("http://127.0.0.1:9")
        .with_metrics(false)
        .with_logs(false)
        .install_globally(false)
        .build()
        .expect("builds");
    assert!(
        !traces.install_subscriber(),
        "a global subscriber is never replaced"
    );
}
