// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What [`Telemetry::install_subscriber`](super::Telemetry::install_subscriber)
//! lets a person see: warnings and errors on standard error, and one line,
//! at startup, saying where telemetry goes.
//!
//! Without it a process that starts `Telemetry` and nothing else shows nothing
//! when export is failing — every export-side warning (the exporter's own,
//! hyper's, this crate's) is a `tracing` event, and the only layers installed
//! were the exporting ones, which deliberately ignore them. The filter reads
//! `RUST_LOG` in the `target=level` form `tracing_subscriber`'s `Targets`
//! parses, not the full `EnvFilter` syntax (span directives, regexes), which
//! would pull a regex engine in for a convenience.

use tracing_subscriber::Layer;
use tracing_subscriber::filter::{FilterExt as _, Targets, filter_fn};
use tracing_subscriber::registry::LookupSpan;

use super::is_self_target;

/// Warnings everywhere, and this module's own lines (the startup summary) at
/// `info`.
pub(super) const DEFAULT_FILTER: &str = "warn,a2a_protocol_server::otel::telemetry=info";

/// The filter `rust_log` asks for, or the default; and, when `rust_log` is
/// set but not understood, what to tell the person.
pub(super) fn filter(rust_log: Option<&str>) -> (Targets, Option<String>) {
    let default = || {
        DEFAULT_FILTER
            .parse::<Targets>()
            .unwrap_or_else(|_| Targets::new())
    };
    let Some(value) = rust_log.map(str::trim).filter(|v| !v.is_empty()) else {
        return (default(), None);
    };
    match value.parse::<Targets>() {
        Ok(targets) => (targets, None),
        Err(e) => (
            default(),
            Some(format!(
                "RUST_LOG={value:?} is not understood ({e}); only `target=level` \
                 directives are read here, so the default `{DEFAULT_FILTER}` applies"
            )),
        ),
    }
}

/// Whether an event's name is one the OpenTelemetry SDK gives to a failed
/// export (`BatchSpanProcessor.ExportError`, `PeriodicReader.ExportError`,
/// `BatchLogProcessor.Export.Error`, …). The SDK reports every failed batch;
/// `counting.rs` reports the same loss once per 30 s, with the signal and the
/// destination, so the SDK's copy would only repeat it.
fn is_sdk_export_error(event_name: &str) -> bool {
    event_name.ends_with("ExportError") || event_name.ends_with("Export.Error")
}

/// Events that pass `filter`, on standard error — except the SDK's own copy
/// of a lost export and, when `logs_on_console` (the console log exporter
/// already prints every event), anything that is not telemetry's own: those
/// are the events no exporter takes.
///
/// `app`, when given, is a crate whose own INFO lines pass too: the
/// application's, printed by default (`Telemetry::install_subscriber`). It
/// matches that crate and its modules (`app`, `app::…`) only: a `Targets`
/// directive matches by string prefix, so `a2a=info` (the CLI's) also let
/// every `a2a_protocol_*` crate's INFO lines through.
pub(super) fn layer<S>(
    filter: Targets,
    logs_on_console: bool,
    app: Option<String>,
) -> impl Layer<S> + Send + Sync + 'static
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    use tracing_subscriber::filter::LevelFilter;
    let app = app_name(app);
    let has_app = app.is_some();
    let app_info =
        filter_fn(move |meta| shows_app_info(app.as_deref(), *meta.level(), meta.target()))
            .with_max_level_hint(if has_app {
                LevelFilter::INFO
            } else {
                LevelFilter::OFF
            });
    let shown = filter_fn(move |meta| {
        !is_sdk_export_error(meta.name()) && (!logs_on_console || is_self_target(meta.target()))
    });
    tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(colour(
            std::io::IsTerminal::is_terminal(&std::io::stderr()),
            std::env::var_os("NO_COLOR").as_deref(),
        ))
        .with_filter(filter.or(app_info).and(shown))
}

/// Whether `target` is crate `app` or one of its modules.
/// The application's crate name, when one is given: an empty one is none.
fn app_name(app: Option<String>) -> Option<String> {
    app.filter(|a| !a.is_empty())
}

/// Whether an event of `level` on `target` is the application's own `info`
/// or louder, which the console shows beside the SDK's warnings.
fn shows_app_info(app: Option<&str>, level: tracing::Level, target: &str) -> bool {
    level <= tracing::Level::INFO && app.is_some_and(|app| is_app_target(app, target))
}

fn is_app_target(app: &str, target: &str) -> bool {
    target
        .strip_prefix(app)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
}

/// Whether to colour the lines: only on a terminal, and not when `NO_COLOR`
/// is set to anything but the empty string (<https://no-color.org>). A
/// redirected stderr is a file, a container runtime's log or a pipe, where
/// colour codes are noise in every line (they were, in Docker and Kubernetes
/// logs, until 2026-10-28).
fn colour(stderr_is_terminal: bool, no_color: Option<&std::ffi::OsStr>) -> bool {
    stderr_is_terminal && no_color.is_none_or(std::ffi::OsStr::is_empty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::Level;

    /// Whether `targets` lets an event of `level` on `target` through.
    fn passes(targets: &Targets, target: &str, level: Level) -> bool {
        targets.would_enable(target, &level)
    }

    #[test]
    fn lines_are_coloured_only_on_a_terminal_without_no_color() {
        use std::ffi::OsStr;
        assert!(colour(true, None));
        assert!(
            colour(true, Some(OsStr::new(""))),
            "an empty NO_COLOR is unset"
        );
        assert!(!colour(true, Some(OsStr::new("1"))));
        assert!(
            !colour(false, None),
            "a file, pipe or container log gets no codes"
        );
    }

    #[test]
    fn the_sdks_failed_export_reports_are_recognised_and_its_other_events_are_not() {
        for name in [
            "BatchSpanProcessor.ExportError",
            "BatchSpanProcessor.Flush.ExportError",
            "BatchSpanProcessor.Export.Error",
            "BatchLogProcessor.ExportError",
            "PeriodicReader.ExportError",
        ] {
            assert!(is_sdk_export_error(name), "{name}");
        }
        for name in [
            "BatchSpanProcessor.ExportingDueToTimer",
            "BatchSpanProcessor.SpanDroppingStarted",
            "event src/lib.rs:1",
        ] {
            assert!(!is_sdk_export_error(name), "{name}");
        }
    }

    #[test]
    fn by_default_warnings_pass_and_only_telemetry_may_speak_at_info() {
        let (f, bad) = filter(None);
        assert!(bad.is_none());
        assert!(passes(&f, "my_agent", Level::WARN));
        assert!(!passes(&f, "my_agent", Level::INFO));
        assert!(passes(
            &f,
            "a2a_protocol_server::otel::telemetry",
            Level::INFO
        ));
        assert!(!passes(
            &f,
            "a2a_protocol_server::otel::telemetry",
            Level::DEBUG
        ));
    }

    #[test]
    fn rust_log_replaces_the_default_in_target_equals_level_form() {
        let (f, bad) = filter(Some("my_agent=debug,error"));
        assert!(bad.is_none());
        assert!(passes(&f, "my_agent::inner", Level::DEBUG));
        assert!(!passes(&f, "other", Level::WARN));
        assert!(passes(&f, "other", Level::ERROR));
    }

    /// The application's own lines are printed at INFO by default: until
    /// 2026-10-29 `deploy-agent`'s "serving" and "connecting to PostgreSQL"
    /// lines were hidden unless `RUST_LOG` named its crate (found live). Its
    /// crate only: the CLI's `a2a` must not admit `a2a_protocol_client`.
    #[test]
    fn the_application_target_is_its_crate_and_modules_only() {
        assert!(is_app_target("deploy_agent", "deploy_agent"));
        assert!(is_app_target("deploy_agent", "deploy_agent::config"));
        assert!(!is_app_target("a2a", "a2a_protocol_client::discovery"));
        assert!(!is_app_target("deploy_agent", "deploy_agent_v2"));
    }

    #[test]
    fn the_applications_info_and_louder_is_shown_and_its_debug_is_not() {
        let app = Some("deploy_agent");
        for level in [Level::ERROR, Level::WARN, Level::INFO] {
            assert!(
                shows_app_info(app, level, "deploy_agent::config"),
                "{level}"
            );
        }
        for level in [Level::DEBUG, Level::TRACE] {
            assert!(
                !shows_app_info(app, level, "deploy_agent::config"),
                "{level}"
            );
        }
        assert!(!shows_app_info(app, Level::INFO, "a2a_protocol_client"));
        assert!(!shows_app_info(None, Level::INFO, "deploy_agent"));
        assert_eq!(
            app_name(Some("deploy_agent".into())).as_deref(),
            Some("deploy_agent")
        );
        assert_eq!(app_name(Some(String::new())), None, "an empty name is none");
        assert_eq!(app_name(None), None);
    }

    #[test]
    fn an_empty_rust_log_is_unset() {
        let (f, bad) = filter(Some("  "));
        assert!(bad.is_none() && passes(&f, "x", Level::WARN) && !passes(&f, "x", Level::INFO));
    }

    #[test]
    fn a_rust_log_not_understood_falls_back_and_says_so() {
        let (f, bad) = filter(Some("my_agent=loud"));
        let msg = bad.expect("a notice");
        assert!(
            msg.contains("my_agent=loud") && msg.contains("warn"),
            "{msg}"
        );
        assert!(passes(&f, "x", Level::WARN) && !passes(&f, "x", Level::INFO));
    }
}
