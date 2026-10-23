// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a call's span records of the method as sent and, on JSON-RPC, of the
//! request: `docs/rpc/json-rpc.md`'s `jsonrpc.protocol.version`,
//! `rpc.method_original` and `jsonrpc.request.id`.

#![cfg(feature = "tracing")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::tests::{peer_with, recorded};
use super::*;

/// A method outside the A2A service is `_OTHER`, as on the server: the span is
/// named for its system and keeps the method as sent.
#[cfg(feature = "tracing")]
#[test]
fn an_unknown_method_is_named_for_its_system() {
    let fields = recorded(|| {
        let call = CallSpan::open("message/send", &peer_with("", ""), &serde_json::json!({}));
        call.succeeded(None);
    });
    let field = |k: &str| fields.get(k).map(String::as_str);
    assert_eq!(field("otel.name"), Some("jsonrpc"));
    assert_eq!(field("rpc.method"), Some("_OTHER"));
    assert_eq!(field("rpc.method_original"), Some("message/send"));
    // A known method over gRPC is sent as `rpc.method` itself, so it has no
    // original (over JSON-RPC it does: `jsonrpc_tests.rs`).
    let grpc = Peer::new("GRPC", "http://agent.example:9000", None);
    let known = recorded(|| {
        CallSpan::open("GetTask", &grpc, &serde_json::json!({})).succeeded(None);
    });
    assert_eq!(
        known.get("otel.name").map(String::as_str),
        Some("lf.a2a.v1.A2AService/GetTask")
    );
    assert!(!known.contains_key("rpc.method_original"));
}

/// On JSON-RPC a call records the protocol version, which differs from the
/// conventions' default `1.0` (Conditionally Required), and the method as
/// sent, which differs from `rpc.method` (Conditionally Required "if and
/// only if"). The other bindings record neither.
#[test]
fn a_jsonrpc_call_records_its_version_and_the_method_as_sent() {
    let jsonrpc = recorded(|| {
        let peer = Peer::new("JSONRPC", "http://agent.example:9000", None);
        CallSpan::open("SendMessage", &peer, &serde_json::json!({})).succeeded(None);
    });
    let field = |k: &str| jsonrpc.get(k).map(String::as_str);
    assert_eq!(field("jsonrpc.protocol.version"), Some("2.0"));
    assert_eq!(field("rpc.method_original"), Some("SendMessage"));
    assert_eq!(
        field("rpc.method"),
        Some("lf.a2a.v1.A2AService/SendMessage")
    );
    for binding in ["HTTP+JSON", "GRPC"] {
        let other = recorded(|| {
            let peer = Peer::new(binding, "http://agent.example:9000", None);
            CallSpan::open("SendMessage", &peer, &serde_json::json!({})).succeeded(None);
        });
        assert!(!other.contains_key("jsonrpc.protocol.version"), "{binding}");
        assert!(!other.contains_key("rpc.method_original"), "{binding}");
    }
}

/// Counts how often `jsonrpc.request.id` is recorded, and keeps the last.
#[derive(Clone, Default)]
struct IdRecords(Arc<(AtomicUsize, std::sync::Mutex<Option<String>>)>);

struct Id<'a>(&'a IdRecords);

impl tracing::field::Visit for Id<'_> {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "jsonrpc.request.id" {
            (self.0).0.0.fetch_add(1, Ordering::SeqCst);
            *(self.0).0.1.lock().unwrap() = Some(value.to_owned());
        }
    }
    fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for IdRecords {
    fn on_record(
        &self,
        _id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        values.record(&mut Id(self));
    }
}

/// A call whose transport sent two requests — a retry — records the id of
/// the last, the one its response answers, and records it once: a field
/// recorded twice is exported as two attributes with one key.
#[test]
fn the_request_id_is_the_last_one_sent_recorded_once() {
    use tracing_subscriber::layer::SubscriberExt as _;
    let records = IdRecords::default();
    crate::test_tracing::install();
    tracing::subscriber::with_default(tracing_subscriber::registry().with(records.clone()), || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let peer = Peer::new("JSONRPC", "http://agent.example:9000", None);
        let call = CallSpan::open("GetTask", &peer, &serde_json::json!({}));
        let send = async {
            note_request_id(&serde_json::json!("first"));
            note_request_id(&serde_json::json!(7));
        };
        let ((), id) = runtime.block_on(noting_request_id(send));
        call.request_id(id.as_deref());
        call.succeeded(None);
    });
    assert_eq!(records.0.0.load(Ordering::SeqCst), 1);
    assert_eq!(records.0.1.lock().unwrap().as_deref(), Some("7"));
}

/// Outside a call there is no slot, so a transport used directly notes
/// nothing; a `null` id, which the conventions say not to capture, is not
/// noted either.
#[test]
fn no_id_is_noted_outside_a_call_or_for_null() {
    note_request_id(&serde_json::json!("orphan"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let ((), id) = runtime.block_on(noting_request_id(async {
        note_request_id(&serde_json::Value::Null);
    }));
    assert_eq!(id, None);
}
