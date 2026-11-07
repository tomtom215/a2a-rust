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

/// `is_off` is true for `Off` and for nothing else: `Keys` and `All` let
/// entries cross.
#[test]
fn only_off_is_off() {
    assert!(BaggagePolicy::Off.is_off());
    assert!(!BaggagePolicy::All.is_off());
    assert!(!BaggagePolicy::keys(["tenant"]).is_off());
    assert!(!BaggagePolicy::keys(Vec::<String>::new()).is_off());
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

/// The separator counts toward the 8192 bytes: a second member that fits only
/// if the comma were free is skipped, and a first member over the limit is
/// skipped (so a header is never over [`MAX_BYTES`], and never partial).
#[test]
fn the_comma_and_the_first_member_count_toward_the_byte_limit() {
    let all = BaggagePolicy::All;
    // "a=1" + "," + "b=" + value is one byte over when the value is one longer
    // than the exact fit above.
    let one_over = "x".repeat(MAX_BYTES - "a=1,b=".len() + 1);
    let got = encode([("a", "1", None), ("b", one_over.as_str(), None)], &all);
    assert_eq!(got, Some("a=1".to_owned()), "b would make 8193 bytes");
    // Nothing before it: a lone member over the limit is dropped, not kept.
    let over = "x".repeat(MAX_BYTES);
    assert_eq!(encode([("b", over.as_str(), None)], &all), None);
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

const CHILD: &str = "A2A_TEST_BAGGAGE_CHILD";
const FROM_ENV_TEST: &str =
    "trace_context::baggage::tests::from_env_reads_otel_propagators_from_the_process_environment";

/// `from_env` reads `OTEL_PROPAGATORS` from the real process environment. The
/// 2024 edition makes `set_var` unsafe, so the test runs itself again with the
/// variable set, as the server crate's environment tests do.
#[test]
fn from_env_reads_otel_propagators_from_the_process_environment() {
    if std::env::var_os(CHILD).is_some() {
        let p = Propagators::from_env();
        assert!(!p.tracecontext && p.baggage, "{p:?}");
        assert_eq!(p.unknown, ["b3"]);
        return;
    }
    let out = std::process::Command::new(std::env::current_exe().expect("exe"))
        .args([FROM_ENV_TEST, "--exact", "--nocapture"])
        .env(CHILD, "1")
        .env("OTEL_PROPAGATORS", "baggage,b3")
        .output()
        .expect("child runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "child failed: {stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// An empty name between commas (`a,,b`, or a trailing comma) names nothing: it
/// is not an unknown propagator to warn about.
#[test]
fn an_empty_name_in_the_list_is_not_an_unknown_propagator() {
    let p = Propagators::from_env_value(Some("tracecontext,,baggage,"));
    assert!(p.tracecontext && p.baggage, "{p:?}");
    assert!(p.unknown.is_empty(), "{p:?}");
    let p = Propagators::from_env_value(Some("b3,,"));
    assert_eq!(p.unknown, ["b3"]);
}

/// F3: a list naming only propagators this crate does not carry falls back
/// to the default, and names them to warn about.
#[test]
fn a_list_of_only_unknown_propagators_falls_back_to_the_default() {
    let p = Propagators::from_env_value(Some("b3, jaeger"));
    assert!(p.tracecontext && p.baggage, "{p:?}");
    assert_eq!(p.unknown, ["b3", "jaeger"]);
}
