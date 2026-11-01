// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The shared outbound client. Its behaviour (a proxy tunnelled, a private CA
//! trusted, the proxy's login sent) is tested end to end in
//! `tests/outbound_proxy_and_roots.rs`; this is what a unit test can see.

use super::*;

/// `HttpsClient` sits inside `HttpPushSender`, which derives `Debug` and is
/// printed in logs: the client says what it is and nothing of its pool or its
/// proxy rules (which may carry a proxy login).
#[test]
fn debug_names_the_type_and_nothing_else() {
    let client = HttpsClient::new(&crate::net::roots::default_tls_config());
    assert_eq!(format!("{client:?}"), "HttpsClient { .. }");
}
