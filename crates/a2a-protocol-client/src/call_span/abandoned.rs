// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A call whose future the caller dropped before it finished.
//!
//! A `tokio::time::timeout` or a `select!` drops the future of a call that is
//! still waiting for its agent. Nothing after the `.await` runs then, so the
//! span ended with no status and `rpc.client.call.duration` held no point:
//! the calls a caller gave up on, which are the slow ones, were the calls the
//! telemetry could not see. The server records the same event, a call its
//! peer abandoned, as `error.type` `cancelled` (`rpc_span`), and so does this.
//!
//! The interceptors' error hook does not run for such a call: it is async and
//! a destructor cannot await.

use std::time::Instant;

use super::Status;
use crate::A2aClient;

/// The `error.type` of a call dropped before it finished.
const CANCELLED: &str = "cancelled";

impl Status {
    /// A call whose caller dropped it unfinished: no status came back.
    pub(crate) fn cancelled() -> Self {
        Self {
            code: None,
            error: Some(CANCELLED.to_owned()),
        }
    }
}

/// Held across a call's `.await`: dropped before [`finish`](Self::finish), it
/// records the call as cancelled, on its span and in its duration.
///
/// Without `otel` only the span is recorded to; the client, method and clock
/// are for the duration.
#[cfg_attr(not(feature = "otel"), allow(dead_code))]
pub struct Abandoned<'a> {
    client: &'a A2aClient,
    method: &'a str,
    /// A clone, so the span is still open when this records, whichever of
    /// the two the future's drop reaches first.
    span: tracing::Span,
    started: Instant,
    armed: bool,
}

impl<'a> Abandoned<'a> {
    /// Watches the call of `method` on `span`, begun at `started`.
    pub fn new(
        client: &'a A2aClient,
        method: &'a str,
        span: &tracing::Span,
        started: Instant,
    ) -> Self {
        Self {
            client,
            method,
            span: span.clone(),
            started,
            armed: true,
        }
    }

    /// The call came back: nothing was abandoned.
    pub fn finish(mut self) {
        self.armed = false;
    }
}

impl Drop for Abandoned<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let status = Status::cancelled();
        if !self.span.is_disabled() {
            super::ended(&self.span, &status);
        }
        #[cfg(feature = "otel")]
        self.client.metric.record(
            &self.client.peer,
            super::qualified(self.method),
            self.started,
            &status,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ClientBuilder;
    use crate::call_span::{CallSpan, tests::recorded};

    /// What the call's span records when its guard is dropped, after
    /// `finished` or not.
    fn recorded_by(finished: bool) -> std::collections::BTreeMap<String, String> {
        let client = ClientBuilder::new("http://agent.example:9000")
            .build()
            .expect("client");
        recorded(|| {
            let call = CallSpan::open("SendMessage", &client.peer, &serde_json::json!({}));
            let guard = Abandoned::new(&client, "SendMessage", &call.span, Instant::now());
            if finished {
                guard.finish();
            }
        })
    }

    #[test]
    fn a_call_dropped_unfinished_ends_its_span_cancelled() {
        let fields = recorded_by(false);
        assert_eq!(
            fields.get("error.type").map(String::as_str),
            Some("cancelled"),
            "{fields:?}"
        );
        assert_eq!(
            fields.get("otel.status_code").map(String::as_str),
            Some("ERROR")
        );
        assert!(!fields.contains_key("rpc.status_code"), "{fields:?}");
    }

    #[test]
    fn a_call_that_came_back_is_not_recorded_as_cancelled() {
        let fields = recorded_by(true);
        assert!(!fields.contains_key("error.type"), "{fields:?}");
        assert!(!fields.contains_key("otel.status_code"), "{fields:?}");
    }
}
