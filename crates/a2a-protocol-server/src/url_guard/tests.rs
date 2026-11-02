// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The SSRF classifier at the edge of every range it refuses.
//!
//! Audit SEC-12, B-22 and D-13 found ranges missing; N-SEC-1 found that a
//! trailing dot made an IP literal or `localhost` invisible. The ranges
//! below were each run through `check_public_url` on 2026-10-31 and accepted
//! before this change (`198.18.0.1`, `224.0.0.1`, `240.0.0.1`, `[fec0::1]`,
//! `169.254.169.254.`, `localhost.`, `a.localhost`, `metadata`).
//!
//! Every range is tested at its first and last address and one address
//! either side, because a boundary written one off is the mutation a range
//! check suffers and nothing else observes.

use super::*;

fn refused(url: &str) -> bool {
    check_public_url(url).is_err()
}

fn refused_ip(address: &str) -> bool {
    is_private_ip(address.parse().unwrap())
}

#[test]
fn every_ipv4_special_use_range_is_refused_at_both_of_its_edges() {
    for (first, last) in [
        ("0.0.0.0", "0.255.255.255"),
        ("10.0.0.0", "10.255.255.255"),
        ("100.64.0.0", "100.127.255.255"),
        ("127.0.0.0", "127.255.255.255"),
        ("169.254.0.0", "169.254.255.255"),
        ("172.16.0.0", "172.31.255.255"),
        ("192.0.0.0", "192.0.0.255"),
        ("192.168.0.0", "192.168.255.255"),
        ("198.18.0.0", "198.19.255.255"),
        ("224.0.0.0", "255.255.255.255"),
    ] {
        assert!(refused_ip(first), "{first} starts a refused range");
        assert!(refused_ip(last), "{last} ends a refused range");
        assert!(refused(&format!("http://{first}/")), "{first} as a URL");
        assert!(refused(&format!("http://{last}/")), "{last} as a URL");
    }
}

#[test]
fn the_address_beside_each_ipv4_range_is_a_public_one() {
    for public in [
        "1.0.0.0",         // after 0.0.0.0/8
        "9.255.255.255",   // before 10/8
        "11.0.0.0",        // after 10/8
        "100.63.255.255",  // before 100.64/10
        "100.128.0.0",     // after 100.64/10
        "126.255.255.255", // before 127/8
        "128.0.0.0",       // after 127/8
        "169.253.255.255", // before 169.254/16
        "169.255.0.0",     // after 169.254/16
        "172.15.255.255",  // before 172.16/12
        "172.32.0.0",      // after 172.16/12
        "191.255.255.255", // before 192.0.0.0/24
        "192.0.1.0",       // after 192.0.0.0/24
        "192.167.255.255", // before 192.168/16
        "192.169.0.0",     // after 192.168/16
        "198.17.255.255",  // before 198.18/15
        "198.20.0.0",      // after 198.18/15
        "223.255.255.255", // before 224/4
        // The documentation ranges: unrouted, and used as stand-ins for a
        // public host by tests everywhere (including this crate's).
        "192.0.2.1",
        "198.51.100.1",
        "203.0.113.1",
        // Real public addresses.
        "8.8.8.8",
        "93.184.216.34",
    ] {
        assert!(!refused_ip(public), "{public} is a public address");
        assert!(!refused(&format!("http://{public}/")), "{public} as a URL");
    }
}

#[test]
fn every_ipv6_special_use_range_is_refused() {
    for address in [
        "::1",
        "::",
        "fc00::",
        "fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
        "fe80::1",
        "febf:ffff::1",
        "fec0::1",
        "feff:ffff::1",
        "ff00::1",
        "ff02::1",
        "64:ff9b:1::1",
        "64:ff9b:1:ffff::1",
        "2001::1",
        "2001:0:ffff::1",
        "100::1",
        "100::ffff:ffff:ffff:ffff",
        // IPv4 inside IPv6.
        "::ffff:127.0.0.1",
        "::ffff:198.18.0.1",
        "64:ff9b::7f00:1",
        "2002:7f00:1::1",   // 6to4 of 127.0.0.1
        "2002:a9fe:a9fe::", // 6to4 of 169.254.169.254
        "2002:c612::1",     // 6to4 of 198.18.0.0
    ] {
        assert!(refused_ip(address), "{address} is not a public address");
        assert!(
            refused(&format!("http://[{address}]/")),
            "{address} as a URL"
        );
    }
}

#[test]
fn the_address_beside_each_ipv6_range_is_a_public_one() {
    for public in [
        "fbff:ffff::1",
        "fe00::1",
        "fe7f::1",
        "2001:1::1",        // 2001::/32 is Teredo, 2001:1::/32 is not
        "2001:db8::1",      // documentation, allowed like the IPv4 ones
        "2002:808:808::",   // 6to4 of 8.8.8.8
        "64:ff9b::808:808", // NAT64 of 8.8.8.8
        "64:ff9b:2::1",     // not the local-use /48
        "100:0:0:1::1",     // not 100::/64
        "2606:4700:4700::1111",
        "2a00:1450:4001:81b::200e",
    ] {
        assert!(!refused_ip(public), "{public} is a public address");
        assert!(
            !refused(&format!("http://[{public}]/")),
            "{public} as a URL"
        );
    }
}

#[test]
fn a_trailing_dot_does_not_hide_an_address_or_a_name() {
    for url in [
        "http://169.254.169.254./",
        "http://127.0.0.1./latest/meta-data/",
        "http://10.0.0.1./",
        "http://localhost./",
        "http://LOCALHOST./",
        "http://metadata.google.internal./",
        "http://a.local./",
    ] {
        assert!(refused(url), "{url}");
    }
    // The same dot on a public name is the root label and changes nothing.
    assert!(!refused("https://example.com./hook"));
    assert!(!refused("https://hooks.example.org/x"));
}

#[test]
fn reserved_and_internal_names_are_refused() {
    for url in [
        "http://localhost/",
        "http://app.localhost/",
        "http://deep.sub.localhost./",
        "http://printer.local/",
        "http://metadata.google.internal/",
        "http://router.home.arpa/",
        // No dot: resolved through the search domains of whatever looks it up.
        "http://metadata/",
        "http://kubernetes/",
        "http://payments-api/",
        // An empty label is no name.
        "http://a..example.com/",
        "http://example..com/",
    ] {
        assert!(refused(url), "{url}");
    }
    for url in [
        "https://example.com/",
        "https://localhost.example.com/",
        "https://mylocalhost.com/",
        "https://notinternal.example/",
        "https://xn--bcher-kva.example/",
        "https://a.b.c.d.example.org:8443/hook",
    ] {
        assert!(!refused(url), "{url}");
    }
}

#[test]
fn a_numeric_host_is_judged_by_the_address_it_denotes_not_as_a_name() {
    // 8.8.8.8 in every spelling the C resolver accepts. None has a letter
    // outside a hex prefix and most have no dot, so the name rules (no
    // single-label host) must not see them: this was refused when the rules
    // ran on every host (found by `accepts_public_numeric_ip`).
    for url in [
        "http://134744072/",
        "http://0x08080808/",
        "http://01002004010/",
        "http://8.526344/",
        "http://8.8.2056/",
        "http://0x8.0x8.0x8.0x8/",
        "http://134744072./",
    ] {
        assert!(!refused(url), "{url}");
    }
    // 127.0.0.1 in the same spellings is still refused, by the address.
    for url in [
        "http://2130706433/",
        "http://0x7f000001/",
        "http://017700000001/",
        "http://127.1/",
        "http://0x7f.1/",
    ] {
        assert!(refused(url), "{url}");
    }
    // A number too large to be an IPv4 address is no address and no name.
    let too_big = check_public_url("http://999999999999/")
        .unwrap_err()
        .to_string();
    assert!(too_big.contains("single-label"), "{too_big}");
}

#[test]
fn an_embedded_ipv4_is_classified_by_the_address_inside() {
    // Mapped, NAT64, compatible and 6to4 all carry an IPv4 address; the
    // classification is that address's.
    assert!(embedded_ipv4("::ffff:1.2.3.4".parse().unwrap()).is_some());
    assert_eq!(
        embedded_ipv4("2002:102:304::".parse().unwrap()),
        Some(Ipv4Addr::new(1, 2, 3, 4))
    );
    assert_eq!(
        embedded_ipv4("64:ff9b::102:304".parse().unwrap()),
        Some(Ipv4Addr::new(1, 2, 3, 4))
    );
    assert_eq!(embedded_ipv4("2606:4700::1".parse().unwrap()), None);
}

// ── resolve_with: every address, checked ─────────────────────────────────────

fn answer(
    addrs: &[&str],
) -> impl FnOnce(String) -> std::future::Ready<std::io::Result<Vec<SocketAddr>>> {
    let parsed: Vec<SocketAddr> = addrs.iter().map(|a| a.parse().unwrap()).collect();
    move |_| std::future::ready(Ok(parsed))
}

async fn resolved(addrs: &[&str]) -> A2aResult<Option<Vec<SocketAddr>>> {
    resolve_with(
        "https://hooks.example/x",
        "webhook URL",
        std::time::Duration::from_secs(1),
        answer(addrs),
    )
    .await
}

/// A host with several public addresses returns all of them, in the order
/// the resolver gave them and without repeats, so a caller can fail over.
/// Until 0.16.0 only the first was returned (audit B-13).
#[tokio::test]
async fn every_public_address_is_returned_in_order_without_repeats() {
    let got = resolved(&[
        "203.0.113.5:443",
        "[2001:db8::7]:443",
        "203.0.113.5:443",
        "203.0.113.6:443",
    ])
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        got,
        [
            "203.0.113.5:443".parse::<SocketAddr>().unwrap(),
            "[2001:db8::7]:443".parse().unwrap(),
            "203.0.113.6:443".parse().unwrap(),
        ]
    );
}

/// One private address among public ones refuses the name whole: the
/// connector must never be offered it.
#[tokio::test]
async fn one_private_address_among_public_ones_refuses_the_name() {
    let err = resolved(&["203.0.113.5:443", "10.0.0.7:443", "203.0.113.6:443"])
        .await
        .expect_err("refused");
    assert!(err.message.contains("10.0.0.7"), "{}", err.message);
}

/// A resolver that answers nothing is an error, not an empty list.
#[tokio::test]
async fn a_name_that_resolves_to_nothing_is_refused() {
    let err = resolved(&[]).await.expect_err("refused");
    assert!(err.message.contains("did not resolve"), "{}", err.message);
}

/// A literal address needs no lookup and has nothing to pin.
#[tokio::test]
async fn a_literal_address_returns_none() {
    let got = resolve_with(
        "https://203.0.113.9/x",
        "webhook URL",
        std::time::Duration::from_secs(1),
        |_| async { panic!("no lookup for a literal") },
    )
    .await
    .unwrap();
    assert_eq!(got, None);
}
