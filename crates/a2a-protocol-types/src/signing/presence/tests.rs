// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use super::*;
use crate::signing::canonicalize;

/// The canonical schema this table must agree with.
const PROTO: &str = include_str!("../../../proto/a2a_v1/a2a.proto");

/// protobuf's `ToJsonName`: drop underscores, upper-case the next character.
fn json_name(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut upper = false;
    for c in field.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// `message Name { ... }` bodies, with comments stripped. Nested messages do
/// not occur in the part of the schema reachable from `AgentCard`.
fn message_bodies(src: &str) -> BTreeMap<String, String> {
    let stripped: String = src
        .lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = BTreeMap::new();
    let mut rest = stripped.as_str();
    while let Some(at) = rest.find("message ") {
        rest = &rest[at + "message ".len()..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        let Some(open) = rest.find('{') else { break };
        let mut depth = 0usize;
        let mut end = open;
        for (i, c) in rest[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.insert(name, rest[open + 1..end].to_owned());
        rest = &rest[end..];
    }
    out
}

/// What the proto says about one field, in the table's terms.
fn proto_fields(body: &str, messages: &BTreeMap<String, String>) -> Vec<(String, Presence, Shape)> {
    let flat = body.replace(['{', '}'], ";");
    let mut out = Vec::new();
    for decl in flat.split(';') {
        let decl = decl.trim();
        let Some((lhs, rhs)) = decl.split_once('=') else {
            continue;
        };
        if !rhs.trim_start().starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        let mut words: Vec<&str> = lhs.split_whitespace().collect();
        if words.first() == Some(&"oneof") {
            words.drain(..2);
        }
        let (label, ty, name) = match words.as_slice() {
            [label @ ("optional" | "repeated"), ty, name] => (*label, *ty, *name),
            [ty, name] => ("", *ty, *name),
            _ if lhs.contains("map<") => {
                let ty = &lhs[lhs.find("map<").unwrap_or(0)..=lhs.find('>').unwrap_or(0)];
                (
                    "map",
                    ty,
                    lhs.rsplit_once('>').map_or("", |(_, n)| n.trim()),
                )
            }
            _ => continue,
        };
        let presence = if rhs.contains("REQUIRED") {
            Presence::Required
        } else if label == "optional" {
            Presence::Optional
        } else {
            Presence::Implicit
        };
        let value_ty = if label == "map" {
            ty.trim_end_matches('>')
                .rsplit(',')
                .next()
                .unwrap_or("")
                .trim()
        } else {
            ty
        };
        let leaked = |t: &str| -> &'static str { Box::leak(t.to_owned().into_boxed_str()) };
        let shape = if value_ty == "google.protobuf.Struct" {
            Shape::Struct
        } else if messages.contains_key(value_ty) {
            match label {
                "repeated" => Shape::MessageList(leaked(value_ty)),
                "map" => Shape::MessageMap(leaked(value_ty)),
                _ => Shape::Message(leaked(value_ty)),
            }
        } else {
            Shape::Scalar
        };
        out.push((json_name(name), presence, shape));
    }
    out
}

/// The table is the proto, not someone's memory of it: every message
/// reachable from `AgentCard`, every field, its presence and its shape.
#[test]
fn schema_matches_the_vendored_proto() {
    let messages = message_bodies(PROTO);
    let mut reachable = BTreeSet::new();
    let mut queue = vec!["AgentCard".to_owned()];
    let mut from_proto = BTreeMap::new();
    while let Some(name) = queue.pop() {
        if !reachable.insert(name.clone()) {
            continue;
        }
        let body = messages
            .get(&name)
            .unwrap_or_else(|| panic!("message {name} is not in the vendored proto"));
        let fields = proto_fields(body, &messages);
        for (_, _, shape) in &fields {
            if let Shape::Message(c) | Shape::MessageList(c) | Shape::MessageMap(c) = shape {
                queue.push((*c).to_owned());
            }
        }
        from_proto.insert(name, fields);
    }
    let from_table: BTreeMap<String, Vec<(String, Presence, Shape)>> = SCHEMA
        .iter()
        .map(|(m, fields)| {
            (
                (*m).to_owned(),
                fields
                    .iter()
                    .map(|(n, p, s)| ((*n).to_owned(), *p, *s))
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        from_table.keys().collect::<Vec<_>>(),
        from_proto.keys().collect::<Vec<_>>(),
        "SCHEMA must list exactly the messages reachable from AgentCard"
    );
    for (message, fields) in &from_proto {
        assert_eq!(&from_table[message], fields, "fields of {message}");
    }
}

fn pruned(mut value: Value) -> String {
    prune(value.as_object_mut().expect("object"), "AgentCard");
    String::from_utf8(canonicalize(&value).expect("canonical")).expect("utf-8")
}

/// §8.4.1's own worked example, byte for byte.
#[test]
fn the_specification_example_canonicalizes_as_published() {
    let fragment = json!({
        "name": "Example Agent",
        "description": "",
        "capabilities": {
            "streaming": false,
            "pushNotifications": false,
            "extensions": []
        },
        "skills": []
    });
    assert_eq!(
        pruned(fragment),
        r#"{"capabilities":{"pushNotifications":false,"streaming":false},"description":"","name":"Example Agent","skills":[]}"#
    );
}

#[test]
fn implicit_defaults_are_dropped_at_every_depth() {
    let card = json!({
        "name": "a",
        "supportedInterfaces": [{"url": "u", "protocolBinding": "JSONRPC", "tenant": "", "protocolVersion": "1.0"}],
        "capabilities": {"extensions": [{"uri": "x", "description": "", "required": false}]},
        "securitySchemes": {
            "k": {"apiKeySecurityScheme": {"description": "", "location": "header", "name": "X"}}
        },
        "securityRequirements": [],
        "skills": [{"id": "s", "name": "n", "description": "", "tags": [], "examples": [], "inputModes": []}],
    });
    assert_eq!(
        pruned(card),
        concat!(
            r#"{"capabilities":{"extensions":[{"uri":"x"}]},"name":"a","#,
            r#""securitySchemes":{"k":{"apiKeySecurityScheme":{"location":"header","name":"X"}}},"#,
            r#""skills":[{"description":"","id":"s","name":"n","tags":[]}],"#,
            r#""supportedInterfaces":[{"protocolBinding":"JSONRPC","protocolVersion":"1.0","url":"u"}]}"#
        )
    );
}

#[test]
fn optional_fields_keep_a_set_default_and_drop_null() {
    let card = json!({
        "capabilities": {"streaming": false, "extendedAgentCard": null},
        "documentationUrl": "",
        "iconUrl": null,
    });
    assert_eq!(
        pruned(card),
        r#"{"capabilities":{"streaming":false},"documentationUrl":""}"#
    );
}

/// A set message is present however empty it is, and a `Struct`'s contents
/// are free-form JSON with no protobuf defaults to remove.
#[test]
fn set_messages_and_struct_contents_survive() {
    let card = json!({
        "provider": {"url": "", "organization": ""},
        "capabilities": {
            "extensions": [{"uri": "x", "params": {"empty": "", "zero": 0, "list": []}}]
        },
        "securitySchemes": {"m": {"mtlsSecurityScheme": {}}},
    });
    assert_eq!(
        pruned(card),
        concat!(
            r#"{"capabilities":{"extensions":[{"params":{"empty":"","list":[],"zero":0},"uri":"x"}]},"#,
            r#""provider":{"organization":"","url":""},"securitySchemes":{"m":{"mtlsSecurityScheme":{}}}}"#
        )
    );
}

#[test]
fn unknown_keys_and_messages_are_left_alone() {
    let mut value = json!({"futureField": "", "name": ""});
    prune(value.as_object_mut().expect("object"), "AgentCard");
    assert_eq!(value, json!({"futureField": "", "name": ""}));
    let mut other = json!({"anything": ""});
    prune(other.as_object_mut().expect("object"), "NotAMessage");
    assert_eq!(other, json!({"anything": ""}));
}
