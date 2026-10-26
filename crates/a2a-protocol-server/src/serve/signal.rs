// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The signal a platform sends to stop a process.

use std::future::Future;

/// Resolves when the platform asks this process to stop: `SIGTERM` (what a
/// container runtime, `systemd` and `kubectl delete pod` send) or `SIGINT`
/// (Ctrl-C) on Unix, Ctrl-C elsewhere.
///
/// Pass it to [`Server::serve_with_shutdown`](super::Server::serve_with_shutdown)
/// so the server stops accepting, lets in-flight tasks end, and *returns*;
/// whatever follows it in `main` then runs, and a
/// [`Telemetry`](crate::otel::Telemetry) guard still alive there is dropped,
/// which flushes. Waiting on `tokio::signal::ctrl_c()` alone is the common
/// mistake: a pod is stopped with `SIGTERM`, which nothing then handles, so the
/// process is ended with no drain and no flush. Measured 2026-10-26 on
/// `hello-agent`: after `SIGTERM` the collector received no spans and no
/// metrics.
///
/// The handlers are registered when this is *called*, not when the future is
/// first polled, so a signal that arrives between the two is not lost to the
/// default action. It must be called inside a Tokio runtime. If the handlers
/// cannot be registered it warns and falls back to Ctrl-C alone, rather than
/// pretending the drain will happen.
///
/// ```rust,no_run
/// # use std::sync::Arc;
/// # use a2a_protocol_server::{serve::{Server, shutdown_signal}, dispatch::JsonRpcDispatcher};
/// # async fn run(dispatcher: JsonRpcDispatcher) -> std::io::Result<()> {
/// let server = Server::bind("0.0.0.0:3000").await?;
/// server.serve_with_shutdown(dispatcher, shutdown_signal()).await;
/// // Here the signal has arrived and the work has ended: flush, then exit.
/// # Ok(())
/// # }
/// ```
pub fn shutdown_signal() -> impl Future<Output = ()> + Send + 'static {
    #[cfg(unix)]
    let registered = {
        use tokio::signal::unix::{SignalKind, signal};
        match (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
        ) {
            (Ok(terminate), Ok(interrupt)) => Some((terminate, interrupt)),
            (terminate, interrupt) => {
                let _why = terminate.err().or_else(|| interrupt.err());
                trace_warn!(
                    error = ?_why,
                    "cannot listen for SIGTERM and SIGINT; only Ctrl-C will stop the server \
                     gracefully"
                );
                None
            }
        }
    };
    async move {
        #[cfg(unix)]
        if let Some((mut terminate, mut interrupt)) = registered {
            tokio::select! {
                _ = terminate.recv() => { trace_info!("SIGTERM received; shutting down"); }
                _ = interrupt.recv() => { trace_info!("SIGINT received; shutting down"); }
            }
            return;
        }
        let _ = tokio::signal::ctrl_c().await;
        trace_info!("Ctrl-C received; shutting down");
    }
}
