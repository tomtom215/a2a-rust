// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The name a delivery's checked addresses are pinned under.

use super::*;

/// The bare host: no port, no brackets for an address, nothing of the path.
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
