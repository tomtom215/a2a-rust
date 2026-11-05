// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The builder's options for what `Telemetry` adds beyond the three signals:
//! the console format, the Prometheus listener, process and runtime metrics.

use super::TelemetryBuilder;

impl TelemetryBuilder {
    /// Under `OTEL_METRICS_EXPORTER=prometheus`, binds no listener of its
    /// own: the application serves the text of
    /// [`Telemetry::prometheus`](crate::otel::Telemetry::prometheus) on its own
    /// port, behind its own authentication and connection limits (audit
    /// C-16). `OTEL_EXPORTER_PROMETHEUS_HOST` and `_PORT` are then not read.
    #[cfg(feature = "otel-prometheus")]
    #[must_use]
    pub const fn without_prometheus_listener(mut self) -> Self {
        self.prometheus_listener = false;
        self
    }

    /// Reports no process or Tokio runtime metrics. By default, when metrics
    /// are exported, `process.cpu.time`, `process.memory.usage`,
    /// `process.memory.virtual`, `process.thread.count` and
    /// `process.unix.file_descriptor.count` are reported on Linux, and
    /// `tokio.runtime.worker.count`, `tokio.runtime.task.count` and
    /// `tokio.runtime.global_queue.depth` for the runtime `build` runs in
    /// (audit C-13).
    #[must_use]
    pub const fn without_process_metrics(mut self) -> Self {
        self.process_metrics = false;
        self
    }

    /// Reports the Tokio runtime metrics of the runtime `handle` drives,
    /// instead of the one `build` runs in (if any). For an application that
    /// builds `Telemetry` before entering its runtime.
    #[must_use]
    pub fn with_tokio_runtime(mut self, handle: tokio::runtime::Handle) -> Self {
        self.runtime = Some(handle);
        self
    }

    /// The format `install_subscriber` prints in: text (the default) or one
    /// JSON object per line with `trace_id`, `span_id` and `trace_flags`
    /// inside a span (audit C-06). Wins over `A2A_LOG_FORMAT` (`json` or
    /// `text`), which applies when this is not called.
    #[must_use]
    pub const fn with_console_format(mut self, format: super::super::ConsoleFormat) -> Self {
        self.console_format = Some(format);
        self
    }
}
