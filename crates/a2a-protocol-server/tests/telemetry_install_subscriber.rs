// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `Telemetry::install_subscriber` installs the telemetry's layer as the
//! global `tracing` subscriber only when a trace or log provider was built,
//! and never replaces one already set. Its own binary because the global
//! subscriber is process state.

#![cfg(feature = "otlp")]

use a2a_protocol_server::otel::Telemetry;

#[test]
fn installs_once_and_only_when_traces_or_logs_were_built() {
    let off = Telemetry::builder()
        .with_traces(false)
        .with_metrics(false)
        .with_logs(false)
        .install_globally(false)
        .build()
        .expect("builds");
    assert!(
        !off.install_subscriber(),
        "nothing to install with no signal"
    );
    assert!(!tracing::dispatcher::has_been_set());

    let metrics_only = Telemetry::builder()
        .with_otlp_endpoint("http://127.0.0.1:9")
        .with_traces(false)
        .with_logs(false)
        .install_globally(false)
        .build()
        .expect("builds");
    assert!(
        !metrics_only.install_subscriber(),
        "metrics need no subscriber"
    );
    assert!(!tracing::dispatcher::has_been_set());

    let traces = Telemetry::builder()
        .with_otlp_endpoint("http://127.0.0.1:9")
        .with_metrics(false)
        .with_logs(false)
        .install_globally(false)
        .build()
        .expect("builds");
    assert!(traces.install_subscriber());
    assert!(tracing::dispatcher::has_been_set());
    assert!(
        !traces.install_subscriber(),
        "a global subscriber is never replaced"
    );
}
