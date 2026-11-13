// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! An explicit binding override uses the same version-compatible interface
//! selection `from_card` does (adversarial review P4, 2026-11-12).
//!
//! `from_card` filters a card's interfaces to this SDK's protocol major, but
//! kept the whole list, and `with_protocol_binding` searched that list by
//! binding alone: a card that listed JSON-RPC `0.3` at `/legacy` before
//! JSON-RPC `1.0` at `/v1` selected `/v1` from `from_card` and `/legacy` from
//! `with_protocol_binding("JSONRPC")`, while the transport went on sending
//! version 1 requests to it.

use a2a_protocol_types::{AgentCapabilities, AgentCard, AgentInterface};

use super::ClientBuilder;
use crate::config::{BINDING_GRPC, BINDING_HTTP_JSON, BINDING_JSONRPC, BINDING_REST};
use crate::error::ClientError;

fn card(interfaces: Vec<AgentInterface>) -> AgentCard {
    let mut built = AgentCard::new("override", "1.0", AgentInterface::jsonrpc(String::new()));
    built.url = None;
    built.description = "Binding override fixture".into();
    built.supported_interfaces = interfaces;
    built.provider = None;
    built.icon_url = None;
    built.documentation_url = None;
    built.capabilities = AgentCapabilities::none();
    built.security_schemes = None;
    built.security_requirements = None;
    built.default_input_modes = vec![];
    built.default_output_modes = vec![];
    built.skills = vec![];
    built.signatures = None;
    built
}

fn iface(binding: &str, version: &str, url: &str, tenant: Option<&str>) -> AgentInterface {
    let mut built = AgentInterface::new(url, binding).with_protocol_version(version);
    built.tenant = tenant.map(Into::into);
    built
}

/// JSON-RPC 0.3 at `/legacy` (tenant `old`) listed before JSON-RPC 1.0 at `/v1`
/// (tenant `new`).
fn legacy_first() -> AgentCard {
    card(vec![
        iface(
            BINDING_JSONRPC,
            "0.3",
            "http://agent.example/legacy",
            Some("old"),
        ),
        iface(
            BINDING_JSONRPC,
            "1.0",
            "http://agent.example/v1",
            Some("new"),
        ),
    ])
}

#[test]
fn from_card_skips_the_legacy_interface() {
    let builder = ClientBuilder::from_card(&legacy_first()).expect("from_card");
    assert_eq!(builder.endpoint, "http://agent.example/v1");
    assert_eq!(builder.config.tenant.as_deref(), Some("new"));
}

#[test]
fn an_explicit_override_of_the_same_binding_does_not_select_the_legacy_interface() {
    let builder = ClientBuilder::from_card(&legacy_first())
        .expect("from_card")
        .with_protocol_binding(BINDING_JSONRPC);
    assert_eq!(
        builder.endpoint, "http://agent.example/v1",
        "the override moved to an interface for protocol 0.3 and kept sending 1.0 requests to it"
    );
    assert_eq!(builder.config.tenant.as_deref(), Some("new"));
}

#[test]
fn an_override_in_another_case_resolves_the_same_compatible_interface() {
    let builder = ClientBuilder::from_card(&legacy_first())
        .expect("from_card")
        .with_protocol_binding("jsonrpc");
    assert_eq!(builder.endpoint, "http://agent.example/v1");
}

/// `REST` is the legacy spelling of `HTTP+JSON`: an override in either spelling
/// finds an interface advertised in the other.
#[test]
fn the_rest_alias_finds_an_http_json_interface_and_the_reverse() {
    let advertised_canonical = card(vec![
        iface(BINDING_JSONRPC, "1.0", "http://agent.example/rpc", None),
        iface(BINDING_HTTP_JSON, "1.0", "http://agent.example/rest", None),
    ]);
    let builder = ClientBuilder::from_card(&advertised_canonical)
        .expect("from_card")
        .with_protocol_binding(BINDING_REST);
    assert_eq!(builder.endpoint, "http://agent.example/rest");

    let advertised_alias = card(vec![
        iface(BINDING_JSONRPC, "1.0", "http://agent.example/rpc", None),
        iface(BINDING_REST, "1.0", "http://agent.example/rest", None),
    ]);
    let builder = ClientBuilder::from_card(&advertised_alias)
        .expect("from_card")
        .with_protocol_binding(BINDING_HTTP_JSON);
    assert_eq!(builder.endpoint, "http://agent.example/rest");
}

/// The card offers the binding only for a protocol this SDK does not speak:
/// building is refused, naming what the card offers, instead of pairing the
/// new binding with the old binding's endpoint (or the legacy interface's).
#[test]
fn a_binding_the_card_offers_only_for_another_major_is_a_build_error() {
    let only_legacy_rest = card(vec![
        iface(BINDING_JSONRPC, "1.0", "http://agent.example/rpc", None),
        iface(
            BINDING_HTTP_JSON,
            "0.3",
            "http://agent.example/legacy-rest",
            None,
        ),
    ]);
    let built = ClientBuilder::from_card(&only_legacy_rest)
        .expect("from_card")
        .with_protocol_binding(BINDING_HTTP_JSON)
        .build();
    let Err(ClientError::InvalidEndpoint(message)) = built else {
        panic!("expected an InvalidEndpoint error, got {built:?}");
    };
    assert!(
        message.contains("0.3") && message.contains(BINDING_HTTP_JSON),
        "the error names the binding and what the card offers for it: {message}"
    );
}

/// A transport the builder only holds: `build()` constructs a client and never
/// calls it.
struct NeverCalled;

impl crate::transport::Transport for NeverCalled {
    fn send_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _headers: &'a std::collections::HashMap<String, String>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = crate::error::ClientResult<serde_json::Value>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async { Err(ClientError::Transport("unused".into())) })
    }

    fn send_streaming_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _headers: &'a std::collections::HashMap<String, String>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = crate::error::ClientResult<crate::streaming::EventStream>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async { Err(ClientError::Transport("unused".into())) })
    }
}

/// A caller who supplies their own transport is not held to the card offering
/// the binding they named: the card is not asked for an address. The same card
/// and override without the transport is refused (the test above), so this is
/// the transport's doing and not a leniency of the check.
#[test]
fn a_custom_transport_is_not_held_to_the_card_offering_the_binding() {
    let only_legacy_rest = card(vec![
        iface(BINDING_JSONRPC, "1.0", "http://agent.example/rpc", None),
        iface(
            BINDING_HTTP_JSON,
            "0.3",
            "http://agent.example/legacy-rest",
            None,
        ),
    ]);
    let builder = ClientBuilder::from_card(&only_legacy_rest)
        .expect("from_card")
        .with_protocol_binding(BINDING_HTTP_JSON);
    assert!(
        builder.binding_conflict.is_some(),
        "the fixture must conflict, or this test proves nothing"
    );
    let built = builder.with_custom_transport(NeverCalled).build();
    assert!(built.is_ok(), "{built:?}");
}

/// A later override that the card does resolve clears an earlier conflict.
#[test]
fn a_later_resolvable_override_clears_the_conflict() {
    let only_legacy_rest = card(vec![
        iface(BINDING_JSONRPC, "1.0", "http://agent.example/rpc", None),
        iface(
            BINDING_HTTP_JSON,
            "0.3",
            "http://agent.example/legacy-rest",
            None,
        ),
    ]);
    let built = ClientBuilder::from_card(&only_legacy_rest)
        .expect("from_card")
        .with_protocol_binding(BINDING_HTTP_JSON)
        .with_protocol_binding(BINDING_JSONRPC)
        .build();
    assert!(built.is_ok(), "{built:?}");
}

/// A builder that did not come from a card has nothing to resolve against, and
/// the caller's endpoint stands (unchanged behaviour).
#[test]
fn a_builder_without_a_card_keeps_its_endpoint() {
    let builder =
        ClientBuilder::new("http://agent.example/anything").with_protocol_binding(BINDING_GRPC);
    assert_eq!(builder.endpoint, "http://agent.example/anything");
}
