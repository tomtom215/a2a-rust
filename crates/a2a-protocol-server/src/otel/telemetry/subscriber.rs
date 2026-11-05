// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The `tracing` side of [`Telemetry`]: the layer that turns spans and events
//! into OpenTelemetry spans and log records, and the subscriber
//! `install_subscriber` installs with it.

use opentelemetry::trace::TracerProvider as _;
use tracing_subscriber::Layer;
use tracing_subscriber::registry::LookupSpan;

use super::{SCOPE, Telemetry, diagnostics, is_self_target};

impl Telemetry {
    /// The `tracing` layer: spans become OpenTelemetry spans, and events at
    /// or above the log level become log records. Add it to the
    /// application's subscriber, next to any other layers. A signal that is
    /// off contributes nothing.
    #[must_use]
    pub fn layer<S>(&self) -> impl Layer<S> + Send + Sync + 'static
    where
        S: tracing::Subscriber + for<'a> LookupSpan<'a> + Send + Sync,
    {
        use tracing_subscriber::filter::filter_fn;
        use tracing_subscriber::layer::Identity;
        // A signal that is off contributes `Identity`, never `None`:
        // `Option<Layer>::max_level_hint` is `OFF` for `None`
        // (tracing-subscriber 0.3.23, `layer/mod.rs`), and combined with the
        // other half that switched every span off process-wide — traces with
        // logs off (the Langfuse preset) recorded nothing.
        use tracing_subscriber::filter::LevelFilter;
        // Each filter states its most verbose level (`with_max_level_hint`),
        // so `tracing` disables the callsites below every layer's level at
        // once instead of asking these closures for each event.
        let span_level = self.span_level;
        let spans: Box<dyn Layer<S> + Send + Sync> = match &self.tracer {
            Some(provider) => Box::new(
                tracing_opentelemetry::layer()
                    .with_tracer(provider.tracer(SCOPE))
                    .with_filter(
                        filter_fn(move |meta| {
                            *meta.level() <= span_level && !is_self_target(meta.target())
                        })
                        .with_max_level_hint(LevelFilter::from_level(span_level)),
                    ),
            ),
            None => Box::new(Identity::new()),
        };
        let level = self.log_level;
        let logs: Box<dyn Layer<S> + Send + Sync> = match &self.logger {
            Some(provider) => Box::new(
                opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(provider)
                    .with_filter(
                        filter_fn(move |meta| {
                            meta.is_event()
                                && *meta.level() <= level
                                && !is_self_target(meta.target())
                        })
                        .with_max_level_hint(LevelFilter::from_level(level)),
                    ),
            ),
            None => Box::new(Identity::new()),
        };
        spans.and_then(logs)
    }

    /// Installs [`layer`](Self::layer) as the process's global `tracing`
    /// subscriber, with a layer that prints warnings and errors to standard
    /// error and logs [`describe`](Self::describe) once — what a binary with
    /// no subscriber of its own needs. Without the printing layer a process
    /// whose export is failing, or that was never given a collector, shows
    /// nothing at all.
    ///
    /// `RUST_LOG` sets what is printed, in `target=level` form (default: `warn`
    /// everywhere, `info` for this module's own startup line and for the
    /// application's own crate, named after the default service name with `-`
    /// as `_`: `with_default_service_name("deploy-agent")` prints
    /// `deploy_agent`'s INFO lines). Until 2026-10-29 an application's INFO
    /// lines (where it listens, which database it opened) were hidden unless
    /// `RUST_LOG` named its crate.
    ///
    /// Under `OTEL_SDK_DISABLED=true` nothing is exported (`layer()` adds
    /// nothing), but the printing layer is still installed: the variable
    /// turns off the OpenTelemetry SDK, and says nothing about a process's own
    /// warnings and errors. Until 2026-10-31 it installed nothing, so a process
    /// told to export nothing also lost every warning, its own and the SDK's
    /// (audit C-07). Returns whether it installed a
    /// subscriber: `false` when a global subscriber was already set,
    /// which this never replaces. A process with its own subscriber adds
    /// `layer()` to it instead.
    // Called for its effect; the result is for a caller that cares.
    #[allow(clippy::must_use_candidate)]
    pub fn install_subscriber(&self) -> bool {
        use tracing_subscriber::layer::SubscriberExt as _;
        use tracing_subscriber::util::SubscriberInitExt as _;
        let rust_log = std::env::var("RUST_LOG").ok();
        let (filter, not_understood) = diagnostics::filter(rust_log.as_deref());
        // `RUST_LOG`, when set, says everything printed.
        let app = rust_log
            .as_deref()
            .is_none_or(|v| v.trim().is_empty())
            .then(|| self.app_target.clone())
            .flatten();
        let installed = tracing_subscriber::registry()
            .with(self.layer())
            .with(diagnostics::layer(
                filter,
                self.logs_on_console,
                app,
                self.console_format,
            ))
            .try_init()
            .is_ok();
        if installed {
            if self.startup_summary {
                tracing::info!("{}", self.describe());
            }
            for notice in not_understood.iter().chain(&self.notices) {
                tracing::warn!("{notice}");
            }
        }
        installed
    }
}
