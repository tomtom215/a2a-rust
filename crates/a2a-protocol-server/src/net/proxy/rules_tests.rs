// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Loopback and the `NO_PROXY` dialect (audit NEW-b-2, B-23); the client crate
//! has the same tests of its copy.
//!
//! With `HTTP_PROXY` set and `NO_PROXY` unset, a request for
//! `http://127.0.0.1:3000/` used to be sent to the proxy, and a loopback JWKS
//! or model endpoint's credential left the machine with it. Loopback now goes
//! direct unless `A2A_PROXY_LOOPBACK` asks otherwise.

use std::collections::HashMap;

use super::*;

fn env(pairs: &[(&str, &str)]) -> Box<Env> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    Box::new(move |k| map.get(k).cloned())
}

/// The proxy that carries `dst` under `pairs`, as text.
fn via(pairs: &[(&str, &str)], dst: &str) -> Option<String> {
    let rules = matcher(&*env(pairs), &ProxySetting::Environment);
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
        "http://[::1]:8080/",
        "https://localhost/",
        "https://127.0.0.1:8443/",
    ] {
        assert_eq!(via(&PROXIED, dst), None, "{dst} is this machine");
    }
    for dst in [
        "http://agent.example:3000/",
        "https://agent.example/",
        "http://128.0.0.1/",
        "http://localhost.example/",
    ] {
        assert!(via(&PROXIED, dst).is_some(), "{dst} goes through the proxy");
    }
}

#[test]
fn the_environment_can_send_loopback_through_the_proxy_again() {
    for value in ["1", "true", "YES", "on"] {
        let pairs = [PROXIED[0], PROXIED[1], ("A2A_PROXY_LOOPBACK", value)];
        assert!(via(&pairs, "http://127.0.0.1:3000/").is_some(), "{value:?}");
    }
    for value in ["0", "false", "off", "maybe"] {
        let pairs = [PROXIED[0], PROXIED[1], ("A2A_PROXY_LOOPBACK", value)];
        assert_eq!(via(&pairs, "http://127.0.0.1:3000/"), None, "{value:?}");
    }
}

/// An explicit proxy chosen in code is the caller's; the environment's
/// loopback rule and `NO_PROXY` do not apply to it.
#[test]
fn a_proxy_chosen_in_code_still_carries_loopback() {
    let rules = matcher(&*env(&[]), &ProxySetting::Url("http://proxy:1".into()));
    assert!(intercept(&rules, &"http://127.0.0.1:4318/".parse().unwrap()).is_some());
}

#[test]
fn the_callers_entries_apply_in_go_s_dialect_beside_loopback() {
    let pairs = [
        PROXIED[0],
        PROXIED[1],
        ("NO_PROXY", "internal.example, *.corp.example,10.0.0.0/8"),
    ];
    for dst in [
        "http://internal.example/",
        "http://a.corp.example/",
        "http://10.1.2.3/",
        "http://127.0.0.1/",
    ] {
        assert_eq!(via(&pairs, dst), None, "{dst}");
    }
    assert!(via(&pairs, "http://agent.example/").is_some());
}

#[test]
fn the_rules_are_loopback_then_the_callers_entries_in_the_matchers_form() {
    assert_eq!(no_proxy_rules("", false), "localhost,127.0.0.0/8,::1");
    assert_eq!(no_proxy_rules("", true), "");
    assert_eq!(
        no_proxy_rules("a.example, *.b.example ,[fd00::]/8,,", false),
        "localhost,127.0.0.0/8,::1,a.example,.b.example,fd00::/8"
    );
}

#[test]
fn an_entry_with_a_port_is_dropped_and_a_bare_ipv6_address_is_not() {
    for entry in ["host:8080", "10.0.0.1:80", "[::1]:9", "[fd00::1]:443"] {
        assert!(names_a_port(entry), "{entry}");
        assert_eq!(no_proxy_rules(entry, true), "", "{entry}");
    }
    for entry in [
        "host",
        "::1",
        "fd00::1",
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
