// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The request a delivery sends, and how an attempt's outcome is judged.

use super::*;
use a2a_protocol_types::events::TaskStatusUpdateEvent;
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

fn event() -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: TaskId::new("t"),
        context_id: ContextId::new("c"),
        status: TaskStatus::with_timestamp(TaskState::Working),
        metadata: None,
    })
}

fn config(token: Option<&str>) -> TaskPushNotificationConfig {
    TaskPushNotificationConfig {
        tenant: None,
        id: Some("cfg".to_owned()),
        task_id: Some("t".to_owned()),
        url: "https://hooks.example/x".to_owned(),
        token: token.map(str::to_owned),
        authentication: None,
    }
}

/// Every attempt of one delivery carries the id, and it is the same on each:
/// that is what lets a receiver tell a retry from a second event.
#[test]
fn every_attempt_of_a_delivery_carries_the_same_delivery_id() {
    let prepared = Prepared::new(&config(None), &event()).unwrap();
    let first = prepared
        .request("https://hooks.example/x", None, None)
        .unwrap();
    let second = prepared
        .request("https://hooks.example/x", None, None)
        .unwrap();
    let id = first.headers().get(DELIVERY_ID_HEADER).expect("sent");
    assert_eq!(id, second.headers().get(DELIVERY_ID_HEADER).expect("sent"));
    assert_eq!(id.to_str().unwrap(), prepared.delivery_id());
}

/// Two deliveries get two ids, and an id is a UUID: a receiver keying on it
/// must not see two events share one.
#[test]
fn two_deliveries_have_two_ids() {
    let a = Prepared::new(&config(None), &event()).unwrap();
    let b = Prepared::new(&config(None), &event()).unwrap();
    assert_ne!(a.delivery_id(), b.delivery_id());
    assert!(uuid::Uuid::parse_str(a.delivery_id()).is_ok());
}

/// The headers a webhook has always received are still sent: the A2A media
/// type, and the token under both spellings.
#[test]
fn the_content_type_and_the_token_under_both_spellings_are_sent() {
    let prepared = Prepared::new(&config(Some("tok")), &event()).unwrap();
    let req = prepared
        .request("https://hooks.example/x", None, None)
        .unwrap();
    let h = req.headers();
    assert_eq!(h["content-type"], a2a_protocol_types::A2A_CONTENT_TYPE);
    assert_eq!(h[NOTIFICATION_TOKEN_HEADER], "tok");
    assert_eq!(h[NOTIFICATION_TOKEN_HEADER_UNPREFIXED], "tok");
    assert_eq!(req.uri(), "https://hooks.example/x");
    assert!(h.get("host").is_none(), "no rewritten address, no Host");
    assert!(h.get("authorization").is_none());

    let none = Prepared::new(&config(None), &event()).unwrap();
    let req = none.request("https://hooks.example/x", None, None).unwrap();
    assert!(req.headers().get(NOTIFICATION_TOKEN_HEADER).is_none());
}

/// A rewritten address goes in the request line and the original host in
/// `Host`; without a host the header is not invented.
#[test]
fn a_rewritten_address_is_sent_with_the_original_host() {
    let prepared = Prepared::new(&config(None), &event()).unwrap();
    let uri: hyper::Uri = "http://203.0.113.9:80/x".parse().unwrap();
    let req = prepared
        .request(
            "http://hooks.example/x",
            Some(uri.clone()),
            Some("hooks.example"),
        )
        .unwrap();
    assert_eq!(req.uri(), &uri);
    assert_eq!(req.headers()["host"], "hooks.example");
    let req = prepared
        .request("http://hooks.example/x", Some(uri), None)
        .unwrap();
    assert!(req.headers().get("host").is_none());
}

/// CR/LF in a credential is refused before any request exists.
#[test]
fn a_credential_with_a_line_break_is_refused() {
    let err = Prepared::new(&config(Some("a\r\nX-Evil: 1")), &event())
        .err()
        .expect("refused");
    assert!(
        err.message.contains("notification token"),
        "{}",
        err.message
    );
}

fn judged_status(status: u16) -> Attempt {
    let resp = hyper::Response::builder().status(status).body(()).unwrap();
    judge(
        "https://hooks.example",
        0,
        Duration::from_secs(10),
        Ok::<_, ()>(Ok::<_, std::io::Error>(resp)),
    )
}

/// 2xx is delivered; 408, 429 and 5xx are worth another attempt; every
/// other status is the webhook's final answer.
#[test]
fn statuses_are_delivered_retried_or_final() {
    assert_eq!(judged_status(200), Attempt::Delivered);
    assert_eq!(judged_status(204), Attempt::Delivered);
    for retry in [408, 429, 500, 502, 503, 504] {
        assert_eq!(
            judged_status(retry),
            Attempt::Retry(format!(
                "push notification got HTTP {}",
                hyper::StatusCode::from_u16(retry).unwrap()
            )),
            "{retry}"
        );
    }
    for final_ in [400, 401, 403, 404, 410, 422] {
        assert!(
            matches!(judged_status(final_), Attempt::Rejected(m) if m.contains("non-retryable")),
            "{final_}"
        );
    }
}

/// A transport error names itself and a timeout names the bound.
#[test]
fn an_error_and_a_timeout_are_retried_with_their_reasons() {
    let io = std::io::Error::other("connection refused");
    let failed = judge::<()>(
        "https://hooks.example",
        0,
        Duration::from_secs(10),
        Ok::<Result<hyper::Response<()>, _>, ()>(Err(io)),
    );
    assert_eq!(
        failed,
        Attempt::Retry("push notification failed: connection refused".to_owned())
    );
    let timed_out = judge::<()>(
        "https://hooks.example",
        0,
        Duration::from_secs(10),
        Err::<Result<hyper::Response<()>, std::io::Error>, ()>(()),
    );
    assert_eq!(
        timed_out,
        Attempt::Retry("push notification timed out after 10s".to_owned())
    );
}
