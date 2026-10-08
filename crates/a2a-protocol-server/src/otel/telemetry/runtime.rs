// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The private runtime OTLP export runs on.
//!
//! Both exporters need a Tokio runtime, and neither can rely on the
//! application's. tonic spawns its channel's worker onto the ambient runtime
//! while the exporter is built — which is why `init_otlp_pipeline` panics
//! outside one — and the HTTP client's connections are tasks too. The
//! OpenTelemetry SDK's batch processors and periodic reader then export from
//! threads of their own, with no runtime at all, and block on the result.
//!
//! Running that work on the application's runtime deadlocks on a
//! `current_thread` runtime at shutdown: the final flush blocks the one
//! thread the export task would run on. It also ties telemetry to the
//! application's runtime outliving it. So `Telemetry` starts one
//! `current_thread` runtime on a thread of its own, builds the exporters
//! inside it, and stops it only after every provider has flushed.

use std::io;
use std::thread::JoinHandle;

use tokio::runtime::Handle;
use tokio::sync::oneshot;

/// The export thread's name, as it appears in a debugger or `top -H`.
const THREAD_NAME: &str = "a2a-otel-export";

/// A `current_thread` Tokio runtime driven by a thread of its own.
#[derive(Debug)]
pub(super) struct ExportRuntime {
    handle: Handle,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl ExportRuntime {
    /// Starts the runtime and its thread.
    pub(super) fn start() -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .thread_name(THREAD_NAME)
            .build()?;
        let handle = runtime.handle().clone();
        let (stop, stopped) = oneshot::channel::<()>();
        let thread = std::thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || {
                // Runs every task spawned onto `handle` until told to stop —
                // or until the sender is dropped, which is the same thing.
                runtime.block_on(async {
                    let _ = stopped.await;
                });
                // The runtime drops here, on its own thread and outside any
                // async context, which is the one place dropping it is legal.
            })?;
        Ok(Self {
            handle,
            stop: Some(stop),
            thread: Some(thread),
        })
    }

    /// A handle to spawn export work onto, or to `enter` while building an
    /// exporter that spawns.
    pub(super) const fn handle(&self) -> &Handle {
        &self.handle
    }
}

impl Drop for ExportRuntime {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            // The thread only waits for the signal just sent, so this returns
            // as soon as the runtime has dropped its tasks. A panic on that
            // thread is not this caller's to propagate.
            let _ = thread.join();
        }
    }
}
