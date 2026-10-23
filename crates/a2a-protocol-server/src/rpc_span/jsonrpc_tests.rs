// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a JSON-RPC call's span records beyond every call's:
//! `docs/rpc/json-rpc.md`'s `jsonrpc.protocol.version`, `jsonrpc.request.id`
//! and `rpc.method_original` ([`ServerSpan::open_jsonrpc`]).

#![cfg(feature = "tracing")]

use super::ServerSpan;
use super::tests::{Fields, Seen, handler};

/// The fields of the span a JSON-RPC request opens.
fn jsonrpc_fields(request: &str) -> std::collections::BTreeMap<String, String> {
    use tracing_subscriber::layer::SubscriberExt as _;
    let request: a2a_protocol_types::jsonrpc::JsonRpcRequest =
        serde_json::from_str(request).unwrap();
    let fields = Fields::default();
    let handler = handler(&Seen::default());
    tracing::subscriber::with_default(tracing_subscriber::registry().with(fields.clone()), || {
        drop(ServerSpan::open_jsonrpc(&handler, &request, None));
    });
    let recorded = fields.0.lock().unwrap();
    recorded.clone()
}

/// A JSON-RPC span carries the protocol version, the request's id, and the
/// method as sent — once, also for `_OTHER` — as `docs/rpc/json-rpc.md` asks;
/// a notification or a `null` id has no `jsonrpc.request.id`.
#[test]
fn a_jsonrpc_span_has_the_version_the_id_and_the_method_as_sent() {
    let numbered = jsonrpc_fields(r#"{"jsonrpc":"2.0","id":7,"method":"GetTask"}"#);
    let get = |f: &std::collections::BTreeMap<String, String>, k: &str| f.get(k).cloned();
    assert_eq!(
        get(&numbered, "jsonrpc.protocol.version").as_deref(),
        Some("2.0")
    );
    assert_eq!(get(&numbered, "jsonrpc.request.id").as_deref(), Some("7"));
    assert_eq!(
        get(&numbered, "rpc.method_original").as_deref(),
        Some("GetTask")
    );
    assert_eq!(
        get(&numbered, "rpc.method").as_deref(),
        Some("lf.a2a.v1.A2AService/GetTask")
    );
    let named = jsonrpc_fields(r#"{"jsonrpc":"2.0","id":"req-1","method":"tasks/get"}"#);
    assert_eq!(get(&named, "jsonrpc.request.id").as_deref(), Some("req-1"));
    assert_eq!(
        get(&named, "rpc.method_original").as_deref(),
        Some("tasks/get")
    );
    let long = format!(
        r#"{{"jsonrpc":"2.0","id":"{}","method":"GetTask"}}"#,
        "x".repeat(300)
    );
    assert_eq!(
        get(&jsonrpc_fields(&long), "jsonrpc.request.id").map(|id| id.len()),
        Some(128)
    );
    for idless in [
        r#"{"jsonrpc":"2.0","id":null,"method":"GetTask"}"#,
        r#"{"jsonrpc":"2.0","method":"GetTask"}"#,
    ] {
        let fields = jsonrpc_fields(idless);
        assert!(
            !fields.contains_key("jsonrpc.request.id"),
            "{idless}: {fields:?}"
        );
        assert_eq!(
            get(&fields, "jsonrpc.protocol.version").as_deref(),
            Some("2.0")
        );
    }
}
