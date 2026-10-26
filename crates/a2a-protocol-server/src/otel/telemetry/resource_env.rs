// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Percent-decoding in `OTEL_RESOURCE_ATTRIBUTES`.
//!
//! The specification (resource/sdk.md, "Specifying resource information via
//! an environment variable", read 2026-10-26) says the `,` and `=` characters
//! in keys and values MUST be percent-encoded, which a reader must undo:
//! `deployment.environment=prod%2Cwest` is the value `prod,west`.
//! `opentelemetry_sdk` 0.33.1's `EnvResourceDetector` splits on `,` and `=`
//! and keeps the text as written (`resource/env.rs`, `construct_otel_resources`),
//! so the value arrived as the literal `prod%2Cwest`.
//!
//! The SDK's resource is built as before and this module then replaces the
//! attributes whose text holds a `%`. An entry that is not valid percent-encoding
//! (a `%` not followed by two hex digits, or bytes that are not UTF-8) is kept as
//! written and reported: the specification says the whole variable SHOULD be
//! discarded, which would also drop a `service.name` set beside it.

use std::collections::HashSet;

use opentelemetry::KeyValue;
use opentelemetry_sdk::Resource;

use super::config::Env;

/// The attributes `OTEL_RESOURCE_ATTRIBUTES` names whose text needed decoding,
/// each with the key as the SDK stored it, and what could not be decoded.
#[derive(Debug, Default)]
pub(super) struct Decoded {
    pub(super) attributes: Vec<(String, KeyValue)>,
    pub(super) notices: Vec<String>,
}

const fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// `text` with each `%HH` replaced by the byte it names; `None` when a `%` is
/// not followed by two hex digits or the result is not UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let high = hex(*bytes.get(i + 1)?)?;
            let low = hex(*bytes.get(i + 2)?)?;
            out.push(high << 4 | low);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// What `OTEL_RESOURCE_ATTRIBUTES` says, read as the SDK reads it (pairs split
/// on `,` and `=`, text trimmed) and decoded.
pub(super) fn decode(env: &Env) -> Decoded {
    let mut decoded = Decoded::default();
    let Some(raw) = env("OTEL_RESOURCE_ATTRIBUTES") else {
        return decoded;
    };
    for entry in raw.split_terminator(',') {
        let Some((key, value)) = entry.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if !key.contains('%') && !value.contains('%') {
            continue;
        }
        match (percent_decode(key), percent_decode(value)) {
            (Some(k), Some(v)) => decoded
                .attributes
                .push((key.to_owned(), KeyValue::new(k, v))),
            _ => decoded.notices.push(format!(
                "OTEL_RESOURCE_ATTRIBUTES: `{key}={value}` is not valid percent-encoding (a `%` \
                 needs two hex digits after it, and the bytes must be UTF-8), so it is used as \
                 written; encode a literal `%` as `%25`"
            )),
        }
    }
    decoded
}

/// `resource` with the decoded attributes in place of the text the SDK kept.
pub(super) fn apply(resource: Resource, decoded: Vec<(String, KeyValue)>) -> Resource {
    if decoded.is_empty() {
        return resource;
    }
    let stored: HashSet<&str> = decoded.iter().map(|(raw, _)| raw.as_str()).collect();
    let kept: Vec<KeyValue> = resource
        .iter()
        .filter(|(key, _)| !stored.contains(key.as_str()))
        .map(|(key, value)| KeyValue::new(key.clone(), value.clone()))
        .collect();
    Resource::builder_empty()
        .with_attributes(kept)
        .with_attributes(decoded.into_iter().map(|(_, kv)| kv))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(value: &str) -> Box<Env> {
        let map: HashMap<String, String> =
            [("OTEL_RESOURCE_ATTRIBUTES".to_owned(), value.to_owned())].into();
        Box::new(move |k| map.get(k).cloned())
    }

    #[test]
    fn the_characters_the_specification_requires_encoded_are_decoded() {
        let d = decode(&*env(
            "deployment.environment=prod%2Cwest,k8s.pod.label=app%3Dweb",
        ));
        let got: Vec<(String, String)> = d
            .attributes
            .iter()
            .map(|(_, kv)| (kv.key.to_string(), kv.value.to_string()))
            .collect();
        assert_eq!(
            got,
            [
                ("deployment.environment".to_owned(), "prod,west".to_owned()),
                ("k8s.pod.label".to_owned(), "app=web".to_owned()),
            ]
        );
        assert_eq!(d.notices, Vec::<String>::new());
    }

    #[test]
    fn a_key_is_decoded_and_the_raw_key_is_remembered_for_replacement() {
        let d = decode(&*env("a%2Eb=1"));
        assert_eq!(d.attributes.len(), 1);
        assert_eq!(d.attributes[0].0, "a%2Eb");
        assert_eq!(d.attributes[0].1.key.as_str(), "a.b");
    }

    #[test]
    fn multi_byte_utf8_and_either_case_of_hex_decode() {
        let d = decode(&*env("name=caf%C3%A9,other=%e2%9c%93"));
        let values: Vec<String> = d
            .attributes
            .iter()
            .map(|(_, kv)| kv.value.to_string())
            .collect();
        assert_eq!(values, ["café", "✓"]);
    }

    #[test]
    fn text_with_no_percent_is_left_to_the_sdk() {
        let d = decode(&*env("service.name=a,deployment.environment=prod"));
        assert!(d.attributes.is_empty() && d.notices.is_empty());
        assert_eq!(decode(&|_: &str| None).attributes, []);
    }

    #[test]
    fn an_entry_that_is_not_valid_percent_encoding_is_reported_and_the_rest_decoded() {
        for bad in ["cpu=50%", "x=%zz", "x=%4", "x=%ff%fe"] {
            let d = decode(&*env(&format!("ok=a%2Cb,{bad}")));
            assert_eq!(
                d.attributes.len(),
                1,
                "{bad}: the valid entry is still decoded"
            );
            assert_eq!(d.notices.len(), 1, "{bad}");
            assert!(d.notices[0].contains("used as written"), "{}", d.notices[0]);
            assert!(d.notices[0].contains(bad), "{}", d.notices[0]);
        }
    }

    #[test]
    fn applying_replaces_the_text_the_sdk_kept_and_leaves_the_rest() {
        let sdk = Resource::builder_empty()
            .with_attributes([
                KeyValue::new("service.name", "svc"),
                KeyValue::new("deployment.environment", "prod%2Cwest"),
            ])
            .build();
        let d = decode(&*env("deployment.environment=prod%2Cwest"));
        let out = apply(sdk, d.attributes);
        assert_eq!(out.get(&"service.name".into()).unwrap().to_string(), "svc");
        assert_eq!(
            out.get(&"deployment.environment".into())
                .unwrap()
                .to_string(),
            "prod,west"
        );
        assert_eq!(out.len(), 2, "no raw copy is left beside the decoded one");
    }
}
