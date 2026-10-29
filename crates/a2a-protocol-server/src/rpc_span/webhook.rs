// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The `CLIENT` span each push-notification POST runs in, and the trace
//! context that POST carries.
//!
//! A webhook delivery is an outbound HTTP request, so its span is the HTTP
//! conventions' client span (`docs/http/http-spans.md`,
//! `open-telemetry/semantic-conventions@55c98cf`, 2026-10-20):
//!
//! * kind `CLIENT`, named `{http.request.method}` — `POST`. A webhook URL has
//!   no low-cardinality template, and the conventions forbid falling back to
//!   its path ("Instrumentation MUST NOT default to using URI path as a
//!   `{target}`").
//! * `http.request.method`, `server.address`, `server.port` and `url.full`
//!   (Required); `http.response.status_code` when a response came back;
//!   `error.type` and `Error` status when the request failed — for a client a
//!   4xx included ("SHOULD be set to `Error` in case of `SpanKind.CLIENT`"),
//!   the status code as the type, `timeout` for an attempt that ran out of
//!   time and `transport` for one that got no response.
//! * one span per attempt, a retry carrying `http.request.resend_count`
//!   (the conventions' "HTTP request retries and redirects").
//!
//! `url.full` is scrubbed: a webhook URL routinely carries its receiver's
//! secret in the path or query (audit O16), and the conventions say
//! "Sensitive content provided in `url.full` SHOULD be scrubbed when
//! instrumentations can identify it". The origin is kept; a path or query
//! becomes `/REDACTED`.
//!
//! With the `otel` feature each attempt is also measured in
//! `http.client.request.duration` (`docs/http/http-metrics.md`, same
//! revision): seconds, the conventions' advised buckets, and the span's
//! `http.request.method`, `server.address` and `server.port` (Required),
//! `http.response.status_code` and `error.type` (Conditionally Required),
//! on the global meter provider as it is at the measurement, under the
//! `a2a.server` scope. The URL is not an attribute: it would carry the
//! receiver's secret and an unbounded number of values.
//!
//! With the `otel` feature the POST carries this span's own context as
//! `traceparent` (and `tracestate`), so a receiver that continues the trace
//! is the child of a span the exporter has. When nothing records the span —
//! no OpenTelemetry layer, or a call under `InboundTracePolicy::Drop` — no
//! trace context is sent: there is no span for it to name.

use std::future::Future;

/// The span of one webhook POST.
#[derive(Debug)]
pub struct WebhookSpan {
    #[cfg(feature = "tracing")]
    span: tracing::Span,
    /// `server.address` and `server.port`, for the duration metric.
    #[cfg(feature = "otel")]
    target: (String, u16),
}

impl WebhookSpan {
    /// Opens the span for one POST to `url`; `resend_count` is 0 for the
    /// first attempt and the attempt's ordinal for each retry.
    #[cfg_attr(
        not(feature = "tracing"),
        allow(unused_variables, clippy::missing_const_for_fn)
    )]
    pub fn open(url: &str, resend_count: usize) -> Self {
        #[cfg(feature = "tracing")]
        {
            let (host, port) = target(url);
            let span = if super::untraced() {
                tracing::Span::none()
            } else {
                client_span(url, &host, port, resend_count)
            };
            Self {
                span,
                #[cfg(feature = "otel")]
                target: (host, port),
            }
        }
        #[cfg(not(feature = "tracing"))]
        {
            Self {}
        }
    }

    /// Writes this span's context onto the request as `traceparent` and,
    /// when it has one, `tracestate`; nothing when the span is not recorded.
    #[cfg_attr(
        not(feature = "otel"),
        allow(
            unused_variables,
            clippy::unused_self,
            clippy::missing_const_for_fn,
            clippy::needless_pass_by_ref_mut
        )
    )]
    pub fn inject(&self, headers: &mut hyper::HeaderMap) {
        #[cfg(feature = "otel")]
        if let Some((parent, state)) = super::otel::trace_headers(&self.span) {
            use a2a_protocol_types::trace_context::{TRACEPARENT_HEADER, TRACESTATE_HEADER};
            if let Ok(value) = hyper::header::HeaderValue::from_str(&parent) {
                headers.insert(TRACEPARENT_HEADER, value);
            }
            if let Some(value) = (!state.is_empty())
                .then(|| hyper::header::HeaderValue::from_str(&state).ok())
                .flatten()
            {
                headers.insert(TRACESTATE_HEADER, value);
            }
        }
    }

    /// Runs the attempt — the request under its timeout — inside the span,
    /// and records how it ended.
    pub async fn run<B, E, F>(self, attempt: F) -> F::Output
    where
        F: Future<Output = Result<Result<hyper::Response<B>, E>, tokio::time::error::Elapsed>>,
    {
        #[cfg(feature = "tracing")]
        {
            #[cfg(feature = "otel")]
            let started = std::time::Instant::now();
            let out = tracing::Instrument::instrument(attempt, self.span.clone()).await;
            let status = match &out {
                Ok(Ok(resp)) => Some(resp.status().as_u16()),
                _ => None,
            };
            let failure = match (&out, status) {
                (Ok(Ok(_)), Some(status)) => (status >= 400).then(|| status.to_string()),
                (Ok(Err(_)), _) => Some("transport".to_owned()),
                _ => Some("timeout".to_owned()),
            };
            if let Some(status) = status {
                self.span
                    .record("http.response.status_code", i64::from(status));
            }
            if let Some(kind) = failure.as_deref() {
                self.span.record("error.type", kind);
                self.span.record("otel.status_code", "ERROR");
            }
            #[cfg(feature = "otel")]
            record_duration(&self.target, started.elapsed(), status, failure.as_deref());
            out
        }
        #[cfg(not(feature = "tracing"))]
        {
            attempt.await
        }
    }
}

/// `server.address` and `server.port` for `url`: the host without an IPv6
/// literal's brackets, and the port, or the scheme's default.
#[cfg(feature = "tracing")]
fn target(url: &str) -> (String, u16) {
    let uri = url.parse::<hyper::Uri>().ok();
    // `Uri::host` keeps an IPv6 literal's brackets; `server.address` is the
    // address itself.
    let host = uri
        .as_ref()
        .and_then(hyper::Uri::host)
        .map(|h| h.trim_start_matches('[').trim_end_matches(']'))
        .unwrap_or_default();
    let port = uri.as_ref().map_or(0, |u| {
        u.port_u16().unwrap_or_else(|| {
            if u.scheme_str() == Some("https") {
                443
            } else {
                80
            }
        })
    });
    (host.to_owned(), port)
}

/// The HTTP conventions' advised buckets for `http.client.request.duration`.
#[cfg(feature = "otel")]
const DURATION_BUCKETS: [f64; 14] = [
    0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 7.5, 10.0,
];

/// The most webhook hosts `http.client.request.duration` names; the rest
/// are `_OTHER`.
#[cfg(feature = "otel")]
const MAX_METRIC_HOSTS: usize = 64;

/// `host` as the duration metric names it: itself for the first
/// [`MAX_METRIC_HOSTS`] hosts this process has delivered to, `_OTHER` after.
/// A push config's URL is the caller's choice, so a caller registering a
/// webhook per task on hosts of its own made a series per host, without
/// bound (audit C-03). The span keeps the host.
#[cfg(feature = "otel")]
fn bounded_host(host: &str) -> String {
    static SEEN: std::sync::Mutex<Option<std::collections::HashSet<String>>> =
        std::sync::Mutex::new(None);
    let mut guard = SEEN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let seen = guard.get_or_insert_with(std::collections::HashSet::new);
    let named =
        seen.contains(host) || (seen.len() < MAX_METRIC_HOSTS && seen.insert(host.to_owned()));
    drop(guard);
    if named {
        host.to_owned()
    } else {
        "_OTHER".to_owned()
    }
}

/// Records one attempt in `http.client.request.duration`.
#[cfg(feature = "otel")]
fn record_duration(
    (host, port): &(String, u16),
    elapsed: std::time::Duration,
    status: Option<u16>,
    failure: Option<&str>,
) {
    use opentelemetry::KeyValue;
    let histogram = opentelemetry::global::meter("a2a.server")
        .f64_histogram("http.client.request.duration")
        .with_unit("s")
        .with_description("Duration of HTTP client requests.")
        .with_boundaries(DURATION_BUCKETS.to_vec())
        .build();
    let mut attributes = vec![
        KeyValue::new("http.request.method", "POST"),
        KeyValue::new("server.address", bounded_host(host)),
        KeyValue::new("server.port", i64::from(*port)),
    ];
    if let Some(status) = status {
        attributes.push(KeyValue::new(
            "http.response.status_code",
            i64::from(status),
        ));
    }
    if let Some(failure) = failure {
        attributes.push(KeyValue::new("error.type", failure.to_owned()));
    }
    histogram.record(elapsed.as_secs_f64(), &attributes);
}

/// The span, every field declared up front.
#[cfg(feature = "tracing")]
fn client_span(url: &str, host: &str, port: u16, resend_count: usize) -> tracing::Span {
    let span = tracing::info_span!(
        target: "a2a_protocol_server::push",
        "a2a.webhook",
        otel.name = "POST",
        otel.kind = "client",
        otel.status_code = tracing::field::Empty,
        http.request.method = "POST",
        server.address = host,
        server.port = i64::from(port),
        url.full = scrubbed(url).as_str(),
        http.request.resend_count = tracing::field::Empty,
        http.response.status_code = tracing::field::Empty,
        error.type = tracing::field::Empty,
    );
    if resend_count > 0 {
        span.record(
            "http.request.resend_count",
            i64::try_from(resend_count).unwrap_or(i64::MAX),
        );
    }
    span
}

/// `url` as `url.full` records it: the origin, and `/REDACTED` for any path
/// or query, which is where a webhook keeps its secret.
#[cfg(feature = "tracing")]
fn scrubbed(url: &str) -> String {
    let origin = crate::push::webhook::origin_for_log(url);
    let bare = url
        .parse::<hyper::Uri>()
        .ok()
        .and_then(|u| u.path_and_query().map(|pq| pq.as_str() == "/"))
        .unwrap_or(true);
    if bare {
        format!("{origin}/")
    } else {
        format!("{origin}/REDACTED")
    }
}

#[cfg(test)]
#[cfg(feature = "tracing")]
mod tests {
    use super::*;

    #[test]
    fn url_full_keeps_the_origin_and_hides_path_query_and_userinfo() {
        assert_eq!(
            scrubbed("https://user:pw@hooks.example.com/t/SECRET?sig=SECRET"),
            "https://hooks.example.com/REDACTED"
        );
        assert_eq!(
            scrubbed("http://127.0.0.1:8080/?sig=SECRET"),
            "http://127.0.0.1:8080/REDACTED"
        );
        assert_eq!(scrubbed("http://127.0.0.1:8080"), "http://127.0.0.1:8080/");
        assert_eq!(scrubbed("http://h.example/"), "http://h.example/");
    }

    /// The fields the span of an attempt holds after it ran to `outcome`.
    fn fields_after(
        url: &str,
        resend: usize,
        outcome: Result<Result<hyper::Response<()>, ()>, tokio::time::error::Elapsed>,
    ) -> std::collections::BTreeMap<String, String> {
        use tracing_subscriber::layer::SubscriberExt as _;
        let fields = super::super::tests::Fields::default();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        tracing::subscriber::with_default(
            tracing_subscriber::registry().with(fields.clone()),
            || {
                let _ = runtime.block_on(WebhookSpan::open(url, resend).run(async { outcome }));
            },
        );
        let recorded = fields.0.lock().unwrap();
        recorded.clone()
    }

    fn response(status: u16) -> hyper::Response<()> {
        hyper::Response::builder().status(status).body(()).unwrap()
    }

    #[test]
    fn a_delivered_attempt_records_its_status_and_no_error() {
        let f = fields_after("http://[::1]:9000/hook/SECRET", 0, Ok(Ok(response(204))));
        let get = |k: &str| f.get(k).map(String::as_str);
        assert_eq!(get("otel.name"), Some("POST"));
        assert_eq!(get("otel.kind"), Some("client"));
        assert_eq!(get("http.request.method"), Some("POST"));
        assert_eq!(get("server.address"), Some("::1"));
        assert_eq!(get("server.port"), Some("9000"));
        assert_eq!(get("url.full"), Some("http://[::1]:9000/REDACTED"));
        assert_eq!(get("http.response.status_code"), Some("204"));
        assert_eq!(get("http.request.resend_count"), None);
        assert_eq!(get("error.type"), None);
        assert_eq!(get("otel.status_code"), None);
    }

    #[test]
    fn a_refused_retry_records_its_status_as_the_error() {
        let f = fields_after("https://hooks.example.com/x", 2, Ok(Ok(response(404))));
        let get = |k: &str| f.get(k).map(String::as_str);
        assert_eq!(get("server.port"), Some("443"));
        assert_eq!(get("http.request.resend_count"), Some("2"));
        assert_eq!(get("http.response.status_code"), Some("404"));
        assert_eq!(get("error.type"), Some("404"));
        assert_eq!(get("otel.status_code"), Some("ERROR"));
    }

    #[tokio::test]
    async fn an_attempt_without_a_response_is_a_transport_error_or_a_timeout() {
        let elapsed = tokio::time::timeout(std::time::Duration::ZERO, std::future::pending::<()>())
            .await
            .unwrap_err();
        let timed_out =
            tokio::task::spawn_blocking(move || fields_after("http://h.example/", 0, Err(elapsed)))
                .await
                .unwrap();
        assert_eq!(
            timed_out.get("error.type").map(String::as_str),
            Some("timeout")
        );
        assert!(!timed_out.contains_key("http.response.status_code"));
        let unreachable =
            tokio::task::spawn_blocking(|| fields_after("http://h.example/", 0, Ok(Err(()))))
                .await
                .unwrap();
        assert_eq!(
            unreachable.get("error.type").map(String::as_str),
            Some("transport")
        );
        assert_eq!(
            unreachable.get("otel.status_code").map(String::as_str),
            Some("ERROR")
        );
    }

    /// A POST carries its own span's context as `traceparent`, and nothing
    /// when no OpenTelemetry layer records the span. The mutation gate found
    /// `trace_headers` untested: answering with any made-up pair survived.
    #[cfg(feature = "otel")]
    #[test]
    fn a_post_carries_its_spans_context_only_when_it_is_recorded() {
        use a2a_protocol_types::trace_context::TraceContext;
        use opentelemetry::trace::TracerProvider as _;
        use tracing_subscriber::layer::SubscriberExt as _;
        let injected = || {
            let mut headers = hyper::HeaderMap::new();
            WebhookSpan::open("http://h.example/hook", 0).inject(&mut headers);
            headers
        };
        let unrecorded = tracing::subscriber::with_default(
            tracing_subscriber::registry().with(super::super::tests::Fields::default()),
            injected,
        );
        assert!(unrecorded.is_empty(), "{unrecorded:?}");
        let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder().build();
        let recorded = tracing::subscriber::with_default(
            tracing_subscriber::registry()
                .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("webhook"))),
            injected,
        );
        let parent = recorded["traceparent"].to_str().unwrap();
        let context = TraceContext::parse(parent).expect("a valid traceparent");
        assert!(context.is_sampled(), "{parent}");
        assert_ne!(context.trace_id(), "0".repeat(32));
        assert!(!recorded.contains_key("tracestate"), "{recorded:?}");
    }
}

#[cfg(all(test, feature = "otel"))]
mod bounded_host_tests {
    use super::{MAX_METRIC_HOSTS, bounded_host};

    /// However many hosts a caller's webhooks name, the metric names at most
    /// `MAX_METRIC_HOSTS` of them (audit C-03). The set is per process, so
    /// other tests may have filled some of it: the bound is what is checked.
    #[test]
    fn hosts_past_the_bound_are_one_series() {
        let run = uuid::Uuid::new_v4();
        let named = (0..MAX_METRIC_HOSTS * 2)
            .filter(|i| bounded_host(&format!("h{i}.{run}.example")) != "_OTHER")
            .count();
        assert!(named <= MAX_METRIC_HOSTS, "{named} hosts named");
        assert_eq!(
            bounded_host(&format!("h{}.{run}.example", MAX_METRIC_HOSTS * 2)),
            "_OTHER"
        );
    }
}
