// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The proxy rules and connector, through `proxy`'s own functions.

use super::*;

fn env(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |name| {
        vars.iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| (*v).to_owned())
    }
}

fn uri(s: &str) -> http::Uri {
    s.parse().unwrap()
}

#[test]
fn https_proxy_applies_to_https_and_no_proxy_exempts() {
    let rules = matcher_from(&env(&[
        ("HTTPS_PROXY", "http://proxy:3128"),
        ("no_proxy", "internal.example,10.0.0.0/8,[::1]"),
    ]));
    let p = intercept(&rules, &uri("https://agent.example.com/")).expect("proxied");
    assert_eq!(p.uri().host(), Some("proxy"));
    assert!(intercept(&rules, &uri("https://a.internal.example/")).is_none());
    assert!(intercept(&rules, &uri("https://10.1.2.3/")).is_none());
    assert!(intercept(&rules, &uri("https://[::1]:8443/")).is_none());
    assert!(
        intercept(&rules, &uri("http://agent.example.com/")).is_none(),
        "HTTP_PROXY unset"
    );
}

#[test]
fn lower_case_names_are_read_and_empty_values_ignored() {
    let rules = matcher_from(&env(&[("HTTP_PROXY", ""), ("http_proxy", "http://p:8080")]));
    assert!(intercept(&rules, &uri("http://agent/")).is_some());
}

#[test]
fn cgi_ignores_proxy_variables() {
    let rules = matcher_from(&env(&[
        ("REQUEST_METHOD", "GET"),
        ("HTTP_PROXY", "http://attacker:80"),
    ]));
    assert!(intercept(&rules, &uri("http://agent/")).is_none());
}

#[cfg(feature = "websocket")]
#[test]
fn a_websocket_url_is_matched_as_its_http_equivalent() {
    assert_eq!(
        http_equivalent(&uri("ws://a.example:81/x")),
        uri("http://a.example:81/x")
    );
    assert_eq!(
        http_equivalent(&uri("wss://a.example/")),
        uri("https://a.example/")
    );
    assert_eq!(
        http_equivalent(&uri("https://a.example/")),
        uri("https://a.example/")
    );
    let rules = matcher_from(&env(&[
        ("HTTPS_PROXY", "http://p:3128"),
        ("NO_PROXY", "b.example"),
    ]));
    assert!(intercept(&rules, &http_equivalent(&uri("wss://a.example/"))).is_some());
    assert!(intercept(&rules, &http_equivalent(&uri("wss://b.example/"))).is_none());
    assert!(
        intercept(&rules, &http_equivalent(&uri("ws://a.example/"))).is_none(),
        "only HTTPS_PROXY set: a plaintext ws:// goes direct"
    );
}

#[test]
fn a_socks_proxy_is_not_spoken_but_noticed() {
    let rules = matcher_from(&env(&[("ALL_PROXY", "socks5://p:1080")]));
    let dst = uri("https://agent/");
    assert!(intercept(&rules, &dst).is_none());
    assert_eq!(unsupported(&rules, &dst).as_deref(), Some("socks5"));
}
