// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! One webhook delivery's request, minus the address it goes to, and how an
//! attempt's outcome is judged. Moved out of `sender.rs`, which is over the
//! 500-line limit, when the delivery id was added.

use std::time::Duration;

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use bytes::Bytes;
use http_body_util::Full;

use super::sender::{authorization_header, validate_header_value};
use super::webhook::{
    DELIVERY_ID_HEADER, NOTIFICATION_TOKEN_HEADER, NOTIFICATION_TOKEN_HEADER_UNPREFIXED,
};

/// What every attempt of one delivery sends: the checked credentials, the
/// serialised event and the id that tells a receiver two attempts are one
/// delivery.
pub(super) struct Prepared {
    authorization: Option<String>,
    token: Option<String>,
    body: Bytes,
    delivery_id: String,
}

impl Prepared {
    /// Checks the config's credentials and serialises `event`.
    ///
    /// # Errors
    ///
    /// A credential with CR/LF in it, an authentication scheme this sender
    /// cannot send, or an event that does not serialise.
    pub(super) fn new(
        config: &TaskPushNotificationConfig,
        event: &StreamResponse,
    ) -> A2aResult<Self> {
        // Header injection protection: validate credentials.
        if let Some(ref auth) = config.authentication {
            if let Some(ref credentials) = auth.credentials {
                validate_header_value(credentials, "authentication credentials")?;
            }
            validate_header_value(&auth.scheme, "authentication scheme")?;
        }
        if let Some(ref token) = config.token {
            validate_header_value(token, "notification token")?;
        }
        // Spec §13.2: agents MUST include the credentials
        // `config.authentication` specifies. One this sender cannot send
        // refuses the delivery; until 2026-10-23 it delivered without
        // any `Authorization` header.
        let authorization = authorization_header(config)?;

        let body = serde_json::to_vec(event)
            .map(Bytes::from)
            .map_err(|e| A2aError::internal(format!("push serialization: {e}")))?;

        Ok(Self {
            authorization,
            token: config.token.clone(),
            body,
            // One per delivery, the same on every attempt of it (audit D-05).
            delivery_id: uuid::Uuid::new_v4().to_string(),
        })
    }

    /// The id sent as `A2A-Delivery-Id`.
    #[cfg(test)]
    pub(super) fn delivery_id(&self) -> &str {
        &self.delivery_id
    }

    /// The request for one attempt: to `uri` when the caller has rewritten
    /// the address (with `host` as the `Host` header that goes with it), and
    /// to `url` as it stands otherwise.
    pub(super) fn request(
        &self,
        url: &str,
        uri: Option<hyper::Uri>,
        host: Option<&str>,
    ) -> A2aResult<hyper::Request<Full<Bytes>>> {
        // §4.3.3 fixes the webhook request's Content-Type as
        // `application/a2a+json`, and unlike §9's JSON-RPC binding —
        // which specifies `application/json` — this one is the A2A
        // media type. It read `application/json` until 2026-08-30,
        // from the same stale vendored specification that put six
        // §5.4 error mappings wrong.
        //
        // Named through the constant rather than spelled again: the
        // REST dispatcher already accepts this media type by the same
        // name, and a webhook receiver written against §4.3.3 matches
        // on it.
        let mut builder = hyper::Request::builder()
            .method(hyper::Method::POST)
            .header("content-type", a2a_protocol_types::A2A_CONTENT_TYPE)
            .header(DELIVERY_ID_HEADER, self.delivery_id.as_str());

        if let Some(uri) = uri {
            builder = builder.uri(uri);
            if let Some(host) = host {
                builder = builder.header("host", host);
            }
        } else {
            builder = builder.uri(url);
        }

        if let Some(value) = self.authorization.as_deref() {
            builder = builder.header("authorization", value);
        }

        // Set the notification token header if present — under both
        // spellings, the same value in each.
        //
        // The specification names no token header: §4.3.3's request
        // example carries only `Authorization` and `Content-Type`. The
        // reference SDKs disagree. a2a-sdk (Python) 1.1.5 sends
        // `headers = {'X-A2A-Notification-Token': push_info.token}`
        // (`base_push_notification_sender.py`); a2a-go v2.5.0 sends
        // `http.CanonicalHeaderKey("A2A-Notification-Token")`
        // (`a2asrv/push/sender.go`). This comment used to say the X-
        // name is what "official-SDK webhook receivers look for",
        // which is true of Python and false of Go: a Go webhook read
        // no token from this sender. 0.8 had dropped the unprefixed
        // name as this SDK's own invention; it is a2a-go's, so it is
        // back. Sending both costs one header line, and a receiver
        // reading either finds the token (`webhook::notification_token`
        // reads both).
        if let Some(ref token) = self.token {
            builder = builder
                .header(NOTIFICATION_TOKEN_HEADER, token.as_str())
                .header(NOTIFICATION_TOKEN_HEADER_UNPREFIXED, token.as_str());
        }

        builder
            .body(Full::new(self.body.clone()))
            .map_err(|e| A2aError::internal(format!("push request build: {e}")))
    }
}

/// What one attempt came to.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Attempt {
    /// The webhook answered `2xx`.
    Delivered,
    /// The webhook answered in a way no retry will change: the message to
    /// report and the status it answered with.
    Rejected { why: String, status: u16 },
    /// The attempt failed in a way worth another: the message to report if
    /// none succeeds.
    Retry(String),
}

/// Judges an attempt's `result` (a response, a transport error, or the
/// request timeout), logging a failure against `target`.
pub(super) fn judge<B>(
    target: &str,
    attempt: usize,
    request_timeout: Duration,
    result: Result<Result<hyper::Response<B>, impl std::error::Error + 'static>, impl Sized>,
) -> Attempt {
    let _ = (target, attempt);
    match result {
        Ok(Ok(resp)) if resp.status().is_success() => {
            trace_debug!(target = %target, "push notification delivered");
            Attempt::Delivered
        }
        Ok(Ok(resp)) => {
            let status = resp.status();
            // A non-retryable client error (400/401/403/404/…) will
            // fail identically on every attempt — retrying it only
            // hammers the webhook and delays the failure signal.
            // Retry is reserved for transient statuses: 408
            // (request timeout), 429 (rate limited), and 5xx.
            let retryable = status.is_server_error()
                || status == hyper::StatusCode::REQUEST_TIMEOUT
                || status == hyper::StatusCode::TOO_MANY_REQUESTS;
            if !retryable {
                trace_warn!(target = %target, attempt, status = %status, "push delivery rejected; not retrying");
                return Attempt::Rejected {
                    why: format!("push notification got non-retryable HTTP {status}"),
                    status: status.as_u16(),
                };
            }
            trace_warn!(target = %target, attempt, status = %status, "push delivery failed");
            Attempt::Retry(format!("push notification got HTTP {status}"))
        }
        Ok(Err(e)) => {
            trace_warn!(target = %target, attempt, error = %e, "push delivery error");
            Attempt::Retry(format!(
                "push notification failed: {}",
                crate::causes::with_causes(&e)
            ))
        }
        Err(_) => {
            trace_warn!(target = %target, attempt, "push delivery timed out");
            Attempt::Retry(format!(
                "push notification timed out after {}s",
                request_timeout.as_secs()
            ))
        }
    }
}

#[cfg(test)]
mod tests;
