// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Loopback and the `NO_PROXY` dialect (audit NEW-b-2, B-23).
//!
//! With `HTTP_PROXY` set and `NO_PROXY` unset, a request for
//! `http://127.0.0.1:3000/` used to be sent to the proxy: the error never
//! mentioned one, and a loopback token endpoint's client secret left the machine
//! in cleartext (reproduced live, `docs/audits/2026-11-01-triage/live-results.md`
//! LR-2). Loopback now goes direct unless `A2A_PROXY_LOOPBACK` asks otherwise.

use std::collections::HashMap;

use super::*;

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    move |k| map.get(k).cloned()
}

/// The proxy that carries `dst` under `pairs`, as text.
fn via(pairs: &[(&str, &str)], dst: &str) -> Option<String> {
    let rules = matcher_from(&env(pairs));
    intercept(&rules, &dst.parse().expect("a URI")).map(|i| i.uri().to_string())
}

const PROXIED: [(&str, &str); 2] = [
    ("HTTP_PROXY", "http://proxy:3128"),
    ("HTTPS_PROXY", "http://proxy:3129"),
];

#[test]
fn loopback_goes_direct_and_everything_else_through_the_proxy() {
    for dst in [
        "http://127.0.0.1:3000/",
        "http://127.9.9.9/",
        "http://localhost:1/",
        "http://LOCALHOST:5/",
        "http://[::1]:8080/",
        "https://localhost/",
        "https://127.0.0.1:8443/",
    ] {
        assert_eq!(via(&PROXIED, dst), None, "{dst} is this machine");
    }
    for dst in [
        "http://agent.example:3000/",
        "https://agent.example/",
        // Not loopback: one past 127/8, and a name that only starts with it.
        "http://128.0.0.1/",
        "http://localhost.example/",
    ] {
        assert!(via(&PROXIED, dst).is_some(), "{dst} goes through the proxy");
    }
}

#[test]
fn the_environment_can_send_loopback_through_the_proxy_again() {
    for value in ["1", "true", "TRUE", "yes", "on", " 1 "] {
        let pairs = [PROXIED[0], PROXIED[1], ("A2A_PROXY_LOOPBACK", value)];
        assert!(
            via(&pairs, "http://127.0.0.1:3000/").is_some(),
            "A2A_PROXY_LOOPBACK={value:?}"
        );
    }
    for value in ["", "0", "false", "no", "off", "maybe"] {
        let pairs = [PROXIED[0], PROXIED[1], ("A2A_PROXY_LOOPBACK", value)];
        assert_eq!(
            via(&pairs, "http://127.0.0.1:3000/"),
            None,
            "A2A_PROXY_LOOPBACK={value:?} is not a yes"
        );
    }
}

#[test]
fn the_callers_no_proxy_applies_beside_loopback() {
    let pairs = [
        PROXIED[0],
        PROXIED[1],
        ("NO_PROXY", "internal.example, 10.0.0.0/8"),
    ];
    assert_eq!(via(&pairs, "http://internal.example/"), None);
    assert_eq!(via(&pairs, "http://db.internal.example/"), None);
    assert_eq!(via(&pairs, "http://10.1.2.3/"), None);
    assert_eq!(via(&pairs, "http://127.0.0.1/"), None);
    assert!(via(&pairs, "http://agent.example/").is_some());
}

/// Go's `*.domain` entry, which the matcher alone reads as a literal name.
#[test]
fn a_wildcard_domain_entry_exempts_its_subdomains() {
    let pairs = [PROXIED[0], PROXIED[1], ("NO_PROXY", "*.corp.example")];
    assert_eq!(via(&pairs, "http://a.corp.example/"), None);
    assert_eq!(via(&pairs, "http://a.b.corp.example/"), None);
    assert!(via(&pairs, "http://corp.example.org/").is_some());
    assert!(via(&pairs, "http://other.example/").is_some());
}

/// As Go reads the environment: the upper-case name first.
#[test]
fn the_upper_case_variable_wins() {
    let pairs = [
        PROXIED[0],
        PROXIED[1],
        ("NO_PROXY", "a.example"),
        ("no_proxy", "b.example"),
    ];
    assert_eq!(via(&pairs, "http://a.example/"), None);
    assert!(via(&pairs, "http://b.example/").is_some());
}

#[test]
fn the_rules_are_loopback_then_the_callers_entries_in_the_matchers_form() {
    assert_eq!(no_proxy_rules("", false), "localhost,127.0.0.0/8,::1");
    assert_eq!(no_proxy_rules("", true), "");
    assert_eq!(
        no_proxy_rules("a.example, *.b.example ,[fd00::]/8,,", false),
        "localhost,127.0.0.0/8,::1,a.example,.b.example,fd00::/8"
    );
    assert_eq!(no_proxy_rules(" ,", true), "");
}

#[test]
fn an_entry_with_a_port_is_dropped_and_a_bare_ipv6_address_is_not() {
    for entry in [
        "host:8080",
        "10.0.0.1:80",
        "[::1]:9",
        "[fd00::1]:443",
        ".corp.example:8080",
    ] {
        assert!(names_a_port(entry), "{entry}");
        assert_eq!(no_proxy_rules(entry, true), "", "{entry} exempts nothing");
    }
    for entry in [
        "host",
        "::1",
        "fd00::1",
        "2001:db8::",
        "[::1]",
        "fd00::/8",
        "10.0.0.0/8",
        // A trailing colon names no port, and a port is digits.
        "host:",
        "[::1]:",
        "host:http",
        "host:80a",
    ] {
        assert!(!names_a_port(entry), "{entry}");
    }
    assert_eq!(
        no_proxy_rules("fd00::1,host:80,a.example", true),
        "fd00::1,a.example"
    );
}

/// The warning is for the person who wrote the entry: one per entry.
#[cfg(feature = "tracing")]
#[test]
fn an_entry_with_a_port_is_warned_of_once_each() {
    let warned = crate::test_tracing::warnings_in(|| {
        let _ = no_proxy_rules("host:8080,[::1]:9,plain.example,::1", true);
    });
    assert_eq!(warned, 2);
}

#[test]
fn only_the_listed_words_are_yes() {
    for yes in ["1", "true", "True", "YES", "on", "  on\n"] {
        assert!(truthy(yes), "{yes:?}");
    }
    for no in ["", "0", "false", "off", "no", "2", "y", "t"] {
        assert!(!truthy(no), "{no:?}");
    }
}
