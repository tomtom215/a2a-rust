// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for the interfaces an agent card advertises.

use super::{interfaces, Endpoints};
use crate::Binding;

fn endpoints() -> Endpoints {
    Endpoints {
        jsonrpc: "http://127.0.0.1:1/jsonrpc".to_owned(),
        rest: "http://127.0.0.1:2/rest".to_owned(),
        grpc: "127.0.0.1:3".to_owned(),
        websocket: "ws://127.0.0.1:4/ws".to_owned(),
    }
}

/// One interface per binding, in the matrix's own order — so the card and the
/// coverage report describe the same agent in the same sequence.
#[test]
fn there_is_one_interface_per_binding() {
    assert_eq!(interfaces(&endpoints()).len(), Binding::ALL.len());
}

/// The card's `protocolBinding` values and the coverage matrix's column
/// labels are two spellings of the same four names, written in two places. If
/// either is renamed the card and the report start describing different
/// agents, and nothing else would notice — the sweep would report a binding
/// the card never advertised.
#[test]
fn every_binding_label_matches_the_interface_it_advertises() {
    for (b, iface) in Binding::ALL.iter().zip(interfaces(&endpoints())) {
        assert_eq!(
            iface.protocol_binding,
            b.label(),
            "the card advertises {b:?} as {:?}, the matrix calls it {:?}",
            iface.protocol_binding,
            b.label()
        );
    }
}

/// The three the specification names, spelled as §5.3 spells them. Pinned as
/// literals rather than derived, because deriving them from the same constant
/// the code uses would assert nothing.
#[test]
fn the_spec_named_bindings_use_the_specs_spelling() {
    let ifaces = interfaces(&endpoints());
    let binding_at = |u: &str| {
        ifaces
            .iter()
            .find(|i| i.url == u)
            .expect("an interface for this url")
            .protocol_binding
            .clone()
    };
    assert_eq!(binding_at("http://127.0.0.1:1/jsonrpc"), "JSONRPC");
    assert_eq!(binding_at("http://127.0.0.1:2/rest"), "HTTP+JSON");
    assert_eq!(binding_at("127.0.0.1:3"), "GRPC");
    assert_eq!(binding_at("ws://127.0.0.1:4/ws"), "WEBSOCKET");
}

/// Each binding must advertise *its own* address. Two bindings sharing a URL
/// sends every client of one to the other, and the sweep would still report
/// both columns green, because both would answer.
#[test]
fn each_binding_advertises_its_own_url() {
    let ep = endpoints();
    for (b, iface) in Binding::ALL.iter().zip(interfaces(&ep)) {
        assert_eq!(iface.url, ep.url_for(*b), "wrong url for {b:?}");
    }
    let urls: std::collections::BTreeSet<_> = interfaces(&ep).into_iter().map(|i| i.url).collect();
    assert_eq!(urls.len(), Binding::ALL.len(), "two bindings share a url");
}

/// Every interface carries the protocol version the types crate declares, and
/// no tenant — the surface examples are single-tenant.
#[test]
fn interfaces_carry_the_protocol_version_and_no_tenant() {
    for iface in interfaces(&endpoints()) {
        assert_eq!(iface.protocol_version, a2a_protocol_types::A2A_VERSION);
        assert!(iface.tenant.is_none());
    }
}
