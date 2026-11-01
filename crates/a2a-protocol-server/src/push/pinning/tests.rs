// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Which of a host's validated addresses a delivery attempt uses.

use super::*;

fn addrs(n: u8) -> Vec<SocketAddr> {
    (1..=n)
        .map(|i| SocketAddr::from(([203, 0, 113, i], 443)))
        .collect()
}

/// Attempt k uses address k, and the schedule wraps: with two addresses and
/// three attempts the third goes back to the first. An address that is down is
/// followed by the next on the next attempt (audit B-13).
#[test]
fn each_attempt_takes_the_next_address_and_the_schedule_wraps() {
    let all = addrs(3);
    assert_eq!(pick_addr(&all, 0), Some(all[0]));
    assert_eq!(pick_addr(&all, 1), Some(all[1]));
    assert_eq!(pick_addr(&all, 2), Some(all[2]));
    assert_eq!(pick_addr(&all, 3), Some(all[0]), "wraps");
    assert_eq!(pick_addr(&all, 7), Some(all[1]));
}

/// One address is every attempt's address; none is no pin.
#[test]
fn one_address_serves_every_attempt_and_none_gives_none() {
    let one = addrs(1);
    for attempt in 0..5 {
        assert_eq!(pick_addr(&one, attempt), Some(one[0]));
    }
    assert_eq!(pick_addr(&[], 0), None);
    assert_eq!(pick_addr(&[], 9), None);
}

/// The bare host: no port, no brackets for an address, nothing of the path.
#[cfg(feature = "tls-rustls")]
#[test]
fn the_pin_is_keyed_by_the_bare_host() {
    assert_eq!(
        host_from_url("https://hooks.example:8443/a/b?c=d").unwrap(),
        "hooks.example"
    );
    assert_eq!(
        host_from_url("https://hooks.example/x").unwrap(),
        "hooks.example"
    );
    assert!(host_from_url("not a url").is_err());
    assert!(host_from_url("https:///nohost").is_err());
}
