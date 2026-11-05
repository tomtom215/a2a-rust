// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! W3C Baggage parsing, encoding and bounds (W3C Baggage §3), and
//! `OTEL_PROPAGATORS` (OpenTelemetry environment specification).

use super::*;

fn member(key: &str, value: &str, properties: Option<&str>) -> Member {
    Member {
        key: key.to_owned(),
        value: value.to_owned(),
        properties: properties.map(str::to_owned),
    }
}

#[test]
fn nothing_crosses_by_default() {
    assert_eq!(BaggagePolicy::default(), BaggagePolicy::Off);
    assert_eq!(
        parse("tenant=acme", &BaggagePolicy::Off),
        Vec::<Member>::new()
    );
    assert_eq!(
        encode([("tenant", "acme", None)], &BaggagePolicy::Off),
        None
    );
}

#[test]
fn an_allow_list_lets_only_its_keys_through_case_sensitively() {
    let policy = BaggagePolicy::keys(["tenant", "deploy"]);
    assert_eq!(
        parse(
            " tenant = acme , user.id=42,Deploy=x, deploy=blue ",
            &policy
        ),
        [
            member("tenant", "acme", None),
            member("deploy", "blue", None)
        ]
    );
    assert_eq!(
        encode(
            [
                ("user.id", "42", None),
                ("tenant", "acme", None),
                ("Tenant", "x", None)
            ],
            &policy
        ),
        Some("tenant=acme".to_owned())
    );
}

#[test]
fn values_are_percent_decoded_and_encoded_and_properties_kept() {
    let all = BaggagePolicy::All;
    assert_eq!(
        parse("k=a%20b%2C%3Bc%25;ttl=30", &all),
        [member("k", "a b,;c%", Some("ttl=30"))]
    );
    assert_eq!(
        encode([("k", "a b,;c% é\"\\", Some("ttl=30"))], &all),
        Some("k=a%20b%2C%3Bc%25%20%C3%A9%22%5C;ttl=30".to_owned())
    );
    // What is encoded parses back to the same member.
    let header = encode([("k", "a b,;c% é", Some("p"))], &all).unwrap();
    assert_eq!(parse(&header, &all), [member("k", "a b,;c% é", Some("p"))]);
}

#[test]
fn a_malformed_member_is_skipped_not_the_header() {
    let got = parse(
        "ok=1,novalue,bad key=2,=3,bad=%ZZ,notutf8=%FF,also=2",
        &BaggagePolicy::All,
    );
    assert_eq!(got, [member("ok", "1", None), member("also", "2", None)]);
    assert_eq!(
        encode(
            [("bad key", "1", None), ("ok", "1", None)],
            &BaggagePolicy::All
        ),
        Some("ok=1".to_owned())
    );
}

/// W3C Baggage §3.3.2: at least 64 members and 8192 bytes, never a partial
/// member.
#[test]
fn the_bounds_are_64_members_and_8192_bytes_of_whole_members() {
    let all = BaggagePolicy::All;
    let many: String = (0..100)
        .map(|i| format!("k{i}=v"))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(parse(&many, &all).len(), MAX_MEMBERS);
    let keys: Vec<String> = (0..100).map(|i| format!("k{i}")).collect();
    let encoded = encode(keys.iter().map(|k| (k.as_str(), "v", None)), &all).unwrap();
    assert_eq!(encoded.split(',').count(), MAX_MEMBERS);
    // A header over 8192 bytes is dropped whole.
    let long = format!("a=1,b={}", "x".repeat(MAX_BYTES));
    assert_eq!(parse(&long, &all), Vec::<Member>::new());
    let at_limit = format!("b={}", "x".repeat(MAX_BYTES - 2));
    assert_eq!(at_limit.len(), MAX_BYTES);
    assert_eq!(parse(&at_limit, &all).len(), 1);
    // Encoding keeps whole members within 8192 bytes, skipping one that
    // does not fit and keeping later ones that do.
    let big = "x".repeat(MAX_BYTES);
    let got = encode(
        [
            ("a", "1", None),
            ("b", big.as_str(), None),
            ("c", "3", None),
        ],
        &all,
    );
    assert_eq!(got, Some("a=1,c=3".to_owned()));
    let exact = "x".repeat(MAX_BYTES - "a=1,b=".len());
    let got = encode([("a", "1", None), ("b", exact.as_str(), None)], &all).unwrap();
    assert_eq!(got.len(), MAX_BYTES);
}

#[test]
fn the_propagators_default_to_tracecontext_and_baggage() {
    for unset in [None, Some(""), Some("  ")] {
        assert_eq!(Propagators::from_env_value(unset), Propagators::default());
    }
    let d = Propagators::default();
    assert!(d.tracecontext && d.baggage && d.unknown.is_empty());
}

#[test]
fn the_propagators_named_are_the_ones_carried() {
    let p = Propagators::from_env_value(Some(" TraceContext "));
    assert!(p.tracecontext && !p.baggage, "{p:?}");
    let p = Propagators::from_env_value(Some("baggage,b3"));
    assert!(!p.tracecontext && p.baggage, "{p:?}");
    assert_eq!(p.unknown, ["b3"]);
    let p = Propagators::from_env_value(Some("none"));
    assert!(!p.tracecontext && !p.baggage, "{p:?}");
    let p = Propagators::from_env_value(Some("tracecontext,none"));
    assert!(!p.tracecontext && !p.baggage, "none wins: {p:?}");
}

/// F3: a list naming only propagators this crate does not carry falls back
/// to the default, and names them to warn about.
#[test]
fn a_list_of_only_unknown_propagators_falls_back_to_the_default() {
    let p = Propagators::from_env_value(Some("b3, jaeger"));
    assert!(p.tracecontext && p.baggage, "{p:?}");
    assert_eq!(p.unknown, ["b3", "jaeger"]);
}
