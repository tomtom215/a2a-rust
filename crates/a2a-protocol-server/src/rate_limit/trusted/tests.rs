// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Which addresses are in a range, at and either side of its edges.

use super::*;

fn range(s: &str) -> TrustedProxy {
    s.parse().unwrap_or_else(|e| panic!("{s}: {e}"))
}

fn ip(s: &str) -> IpAddr {
    s.parse().expect("an address")
}

#[test]
fn a_range_holds_exactly_the_addresses_sharing_its_prefix() {
    let ten = range("10.0.0.0/8");
    assert!(ten.contains(ip("10.0.0.0")));
    assert!(ten.contains(ip("10.255.255.255")));
    assert!(!ten.contains(ip("11.0.0.0")), "one past the end");
    assert!(!ten.contains(ip("9.255.255.255")), "one before the start");

    // A prefix that is not a multiple of eight, where an off-by-one in the
    // mask would show: .0 and .1 only.
    let pair = range("192.0.2.0/31");
    assert!(pair.contains(ip("192.0.2.0")) && pair.contains(ip("192.0.2.1")));
    assert!(!pair.contains(ip("192.0.2.2")) && !pair.contains(ip("192.0.1.255")));

    let host = range("203.0.113.7/32");
    assert!(host.contains(ip("203.0.113.7")));
    assert!(!host.contains(ip("203.0.113.6")) && !host.contains(ip("203.0.113.8")));

    let v6 = range("2001:db8::/32");
    assert!(v6.contains(ip("2001:db8:ffff:ffff:ffff:ffff:ffff:ffff")));
    assert!(v6.contains(ip("2001:db8::")));
    assert!(!v6.contains(ip("2001:db9::")), "one past the end");
    assert!(!v6.contains(ip("2001:db7:ffff::")), "one before the start");
}

#[test]
fn the_zero_prefix_is_every_address_of_its_family_and_no_other() {
    assert!(range("0.0.0.0/0").contains(ip("198.51.100.1")));
    assert!(!range("0.0.0.0/0").contains(ip("2001:db8::1")));
    assert!(range("::/0").contains(ip("2001:db8::1")));
    assert!(!range("::/0").contains(ip("198.51.100.1")));
    let loopback = range("::1/128");
    assert!(loopback.contains(ip("::1")) && !loopback.contains(ip("::2")));
}

#[test]
fn a_single_address_is_a_host_range_and_host_bits_are_ignored() {
    assert_eq!(range("10.1.2.3"), range("10.1.2.3/32"));
    assert_eq!(range("2001:db8::7"), range("2001:db8::7/128"));
    assert_eq!(range("10.1.2.3/8"), range("10.0.0.0/8"));
    assert_eq!(range("10.1.2.3/8").to_string(), "10.0.0.0/8");
    assert_eq!(range("2001:db8:1::5/32").to_string(), "2001:db8::/32");
}

#[test]
fn an_ipv4_mapped_address_is_its_ipv4_form_in_the_range_and_in_the_question() {
    let ten = range("10.0.0.0/8");
    assert!(ten.contains(ip("::ffff:10.1.2.3")), "a dual-stack peer");
    assert!(!ten.contains(ip("::ffff:11.1.2.3")));
    assert_eq!(range("::ffff:10.0.0.0/104"), ten, "a mapped range");
    // A real IPv6 address that merely looks similar is not an IPv4 one.
    assert!(!ten.contains(ip("::a00:1")));
}

#[test]
fn what_is_not_a_range_is_refused_with_the_reason() {
    for bad in [
        "",
        "/8",
        "nope",
        "10.0.0.0/",
        "10.0.0.0/33",
        "10.0.0.0/-1",
        "10.0.0.0/8/9",
        "2001:db8::/129",
        "10.0.0.0/256",
        "10.0.0.256",
        " 10.0.0.0/8",
    ] {
        let err = bad.parse::<TrustedProxy>().expect_err(bad);
        assert!(
            err.to_string().contains("not a trusted proxy range"),
            "{err}"
        );
        assert!(err.to_string().contains(&format!("{bad:?}")), "{err}");
    }
    assert!(TrustedProxy::new(ip("10.0.0.0"), 33).is_none());
    assert!(TrustedProxy::new(ip("10.0.0.0"), 32).is_some());
    assert!(TrustedProxy::new(ip("2001:db8::"), 129).is_none());
    assert!(TrustedProxy::new(ip("2001:db8::"), 128).is_some());
}
