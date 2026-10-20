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
            let span = if super::untraced() {
                tracing::Span::none()
            } else {
                client_span(url, resend_count)
            };
            Self { span }
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
            let out = tracing::Instrument::instrument(attempt, self.span.clone()).await;
            let failure = match &out {
                Ok(Ok(resp)) => {
                    let status = resp.status().as_u16();
                    self.span
                        .record("http.response.status_code", i64::from(status));
                    (status >= 400).then(|| status.to_string())
                }
                Ok(Err(_)) => Some("transport".to_owned()),
                Err(_) => Some("timeout".to_owned()),
            };
            if let Some(kind) = failure {
                self.span.record("error.type", kind.as_str());
                self.span.record("otel.status_code", "ERROR");
            }
            out
        }
        #[cfg(not(feature = "tracing"))]
        {
            attempt.await
        }
    }
}

/// The span, every field declared up front.
#[cfg(feature = "tracing")]
fn client_span(url: &str, resend_count: usize) -> tracing::Span {
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
}
