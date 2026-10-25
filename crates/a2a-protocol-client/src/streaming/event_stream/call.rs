// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The call a stream belongs to, which lasts as long as the stream: its
//! `CLIENT` span, kept open while the stream is, and its
//! `rpc.client.call.duration`, recorded when the stream is dropped — with
//! the error it ended on, if it did (`call_span.rs`, `call_metric.rs`).

use crate::call_span::Status;
use crate::error::ClientError;

/// A streaming call in progress.
#[derive(Debug)]
pub struct StreamCall {
    pub(super) span: tracing::Span,
    system: Option<&'static str>,
    /// How the stream failed, once it has.
    failed: Option<Status>,
    #[cfg(feature = "otel")]
    metric: Option<Measured>,
}

/// What `rpc.client.call.duration` needs to record the call.
#[cfg(feature = "otel")]
#[derive(Debug)]
struct Measured {
    metric: crate::call_metric::CallMetric,
    peer: crate::call_span::Peer,
    method: &'static str,
    started: std::time::Instant,
}

impl StreamCall {
    /// The call whose span is `span`, on `system`.
    pub(crate) const fn new(span: tracing::Span, system: Option<&'static str>) -> Self {
        Self {
            span,
            system,
            failed: None,
            #[cfg(feature = "otel")]
            metric: None,
        }
    }

    /// Records the call's duration in `metric` when the stream is dropped.
    #[cfg(feature = "otel")]
    #[must_use]
    pub(crate) fn measured(
        mut self,
        metric: crate::call_metric::CallMetric,
        peer: crate::call_span::Peer,
        method: &str,
        started: std::time::Instant,
    ) -> Self {
        self.metric = Some(Measured {
            metric,
            peer,
            method: crate::call_span::qualified(method),
            started,
        });
        self
    }

    /// The stream ended on `error`: the call failed with it.
    pub(super) fn fail(&mut self, error: &ClientError) {
        if self.failed.is_none() {
            self.failed = Some(Status::failed(self.system, error));
        }
    }

    /// The caller dropped the stream before it ended: the call was cut short
    /// (`cancelled`, as the server records a peer that goes away), unless it
    /// had already failed.
    pub(super) fn abandon(&mut self) {
        if self.failed.is_none() {
            self.failed = Some(Status::cancelled());
        }
    }
}

impl Drop for StreamCall {
    fn drop(&mut self) {
        let status = self
            .failed
            .take()
            .unwrap_or_else(|| Status::ok(self.system));
        if !self.span.is_disabled() {
            crate::call_span::ended(&self.span, &status);
        }
        #[cfg(feature = "otel")]
        if let Some(m) = &self.metric {
            m.metric.record(&m.peer, m.method, m.started, &status);
        }
    }
}

impl super::EventStream {
    /// The call failed after the stream opened — an interceptor's `after`
    /// hook refused it — so it ends with `error`, not as abandoned.
    pub(crate) fn fail_call(&mut self, error: &ClientError) {
        if let Some(call) = self.call.as_mut() {
            call.0.fail(error);
        }
    }

    /// Keeps `call` — its span open, its duration running — until the
    /// stream is dropped.
    #[must_use]
    pub(crate) fn with_call(mut self, call: StreamCall) -> Self {
        self.call = Some(std::panic::AssertUnwindSafe(Box::new(call)));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call_span::{CallSpan, Peer};

    /// What the stream's span records when the stream is dropped, after
    /// `during` acts on it.
    fn ended_with(
        during: impl FnOnce(&mut StreamCall),
    ) -> std::collections::BTreeMap<String, String> {
        crate::call_span::tests::recorded(|| {
            let peer = Peer::new("GRPC", "http://a:1", None);
            let call = CallSpan::open("SendStreamingMessage", &peer, &serde_json::json!({}));
            let mut stream = StreamCall::new(call.span.clone(), call.system);
            during(&mut stream);
            drop(stream);
        })
    }

    #[test]
    fn a_stream_that_failed_ends_its_span_with_the_error() {
        let fields = ended_with(|s| s.fail(&ClientError::Timeout("slow".into())));
        assert_eq!(
            fields.get("error.type").map(String::as_str),
            Some("timeout")
        );
        assert_eq!(
            fields.get("otel.status_code").map(String::as_str),
            Some("ERROR")
        );
    }

    #[test]
    fn a_stream_that_ended_well_ends_its_span_ok() {
        let fields = ended_with(|_| {});
        assert_eq!(
            fields.get("rpc.status_code").map(String::as_str),
            Some("OK")
        );
        assert!(!fields.contains_key("error.type"), "{fields:?}");
    }
}
