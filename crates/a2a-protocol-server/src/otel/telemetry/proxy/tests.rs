// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Which destinations go through which proxy, with the environment injected.
//! The wire behaviour (a forward request, a `CONNECT` tunnel) is tested
//! against a real forward proxy in `tests/telemetry_export/proxy.rs`.

use std::collections::HashMap;

use super::*;

fn env(pairs: &[(&str, &str)]) -> Box<Env> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    Box::new(move |k| map.get(k).cloned().filter(|v| !v.trim().is_empty()))
}

fn uri(s: &str) -> http::Uri {
    s.parse().expect("a URI")
}

/// The proxy URI `setting` and `pairs` choose for `dst`, as text.
fn via(pairs: &[(&str, &str)], setting: &ProxySetting, dst: &str) -> Option<String> {
    let rules = matcher(&*env(pairs), setting);
    intercept(&rules, &uri(dst)).map(|i| i.uri().to_string())
}

#[test]
fn nothing_is_proxied_when_nothing_is_set() {
    assert_eq!(
        via(&[], &ProxySetting::Environment, "http://collector:4318"),
        None
    );
}

#[test]
fn the_scheme_of_the_destination_picks_the_variable() {
    let e = [
        ("HTTP_PROXY", "http://plain.proxy:3128"),
        ("HTTPS_PROXY", "http://secure.proxy:3129"),
    ];
    assert_eq!(
        via(&e, &ProxySetting::Environment, "http://collector:4318").as_deref(),
        Some("http://plain.proxy:3128/")
    );
    assert_eq!(
        via(&e, &ProxySetting::Environment, "https://collector:4318").as_deref(),
        Some("http://secure.proxy:3129/")
    );
}

#[test]
fn all_proxy_covers_both_and_the_specific_variable_wins() {
    let e = [
        ("ALL_PROXY", "http://all.proxy:1"),
        ("HTTPS_PROXY", "http://secure.proxy:2"),
    ];
    assert_eq!(
        via(&e, &ProxySetting::Environment, "http://c:1").as_deref(),
        Some("http://all.proxy:1/")
    );
    assert_eq!(
        via(&e, &ProxySetting::Environment, "https://c:1").as_deref(),
        Some("http://secure.proxy:2/")
    );
}

#[test]
fn lowercase_variables_are_read_and_uppercase_wins_when_both_are_set() {
    assert_eq!(
        via(
            &[("https_proxy", "http://lower:1")],
            &ProxySetting::Environment,
            "https://c:1"
        )
        .as_deref(),
        Some("http://lower:1/")
    );
    assert_eq!(
        via(
            &[
                ("HTTPS_PROXY", "http://upper:1"),
                ("https_proxy", "http://lower:1")
            ],
            &ProxySetting::Environment,
            "https://c:1"
        )
        .as_deref(),
        Some("http://upper:1/")
    );
}

#[test]
fn no_proxy_exempts_hosts_suffixes_and_ranges() {
    let e = [
        ("HTTP_PROXY", "http://proxy:3128"),
        ("NO_PROXY", "localhost, .internal.example,10.0.0.0/8"),
    ];
    for exempt in [
        "http://localhost:4318",
        "http://collector.internal.example:4318",
        "http://10.1.2.3:4318",
    ] {
        assert_eq!(
            via(&e, &ProxySetting::Environment, exempt),
            None,
            "{exempt}"
        );
    }
    for proxied in ["http://collector.example:4318", "http://11.1.2.3:4318"] {
        assert!(
            via(&e, &ProxySetting::Environment, proxied).is_some(),
            "{proxied}"
        );
    }
}

#[test]
fn star_in_no_proxy_exempts_everything() {
    let e = [("HTTP_PROXY", "http://proxy:3128"), ("no_proxy", "*")];
    assert_eq!(via(&e, &ProxySetting::Environment, "http://c:1"), None);
}

#[test]
fn off_ignores_the_environment_and_a_url_in_code_ignores_it_entirely() {
    let e = [
        ("HTTP_PROXY", "http://env.proxy:1"),
        ("NO_PROXY", "exempt.example"),
    ];
    assert_eq!(via(&e, &ProxySetting::Off, "http://c:1"), None);
    let code = ProxySetting::Url("http://code.proxy:9".to_owned());
    assert_eq!(
        via(&e, &code, "https://c:1").as_deref(),
        Some("http://code.proxy:9/")
    );
    // `NO_PROXY` belongs to the proxies the environment names, not to one
    // chosen in code.
    assert_eq!(
        via(&e, &code, "http://exempt.example:1").as_deref(),
        Some("http://code.proxy:9/")
    );
}

#[test]
fn a_cgi_process_uses_no_proxy_whatever_the_variables_say() {
    let e = [
        ("REQUEST_METHOD", "GET"),
        ("HTTP_PROXY", "http://attacker.example:1"),
    ];
    assert_eq!(via(&e, &ProxySetting::Environment, "http://c:1"), None);
    assert_eq!(
        via(
            &e,
            &ProxySetting::Url("http://p:1".to_owned()),
            "http://c:1"
        ),
        None
    );
}

#[test]
fn only_http_proxies_are_used_and_the_others_are_named() {
    for (proxy, scheme) in [("socks5://p:1080", "socks5"), ("https://p:443", "https")] {
        let rules = matcher(&*env(&[("ALL_PROXY", proxy)]), &ProxySetting::Environment);
        let dst = uri("http://c:4318");
        assert!(intercept(&rules, &dst).is_none(), "{proxy}");
        assert_eq!(unsupported(&rules, &dst).as_deref(), Some(scheme));
    }
    let rules = matcher(
        &*env(&[("ALL_PROXY", "http://p:1")]),
        &ProxySetting::Environment,
    );
    assert_eq!(unsupported(&rules, &uri("http://c:4318")), None);
}

#[test]
fn userinfo_in_the_proxy_url_becomes_basic_proxy_authorization() {
    let rules = matcher(
        &*env(&[("ALL_PROXY", "http://user:p%40ss@p:3128")]),
        &ProxySetting::Environment,
    );
    let found = intercept(&rules, &uri("http://c:1")).expect("proxied");
    assert_eq!(found.uri().to_string(), "http://p:3128/");
    // base64("user:p@ss"): the password was percent-decoded first.
    assert_eq!(
        found.basic_auth().expect("auth").to_str().expect("ascii"),
        "Basic dXNlcjpwQHNz"
    );
}
