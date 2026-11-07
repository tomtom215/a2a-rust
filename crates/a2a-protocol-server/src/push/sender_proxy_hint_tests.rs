// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The hint that names `HttpPushSender::with_proxy_resolved_names` belongs to
//! one refusal only: a name this process looked up and got no address for.

use std::net::SocketAddr;

use super::*;

const TEN: std::time::Duration = std::time::Duration::from_secs(10);

fn answers(addrs: &'static [&'static str]) -> std::future::Ready<std::io::Result<Vec<SocketAddr>>> {
    std::future::ready(Ok(addrs.iter().map(|a| a.parse().unwrap()).collect()))
}

/// Behind a proxy, a refusal that is not a failed lookup is reported as it
/// is: a private address the lookup did answer with, and a URL the form check
/// refuses before any lookup. Neither is a name the proxy could resolve for
/// this process, so neither is told to enable the option, with it set or not.
#[tokio::test]
async fn the_hint_is_not_given_for_a_refusal_that_is_not_a_failed_lookup() {
    for proxy_resolved_names in [false, true] {
        let private = preflight_with(
            "http://hooks.example.com/h",
            TEN,
            true,
            proxy_resolved_names,
            |_| answers(&["10.0.0.7:80"]),
        )
        .await
        .expect_err("a private answer");
        assert!(private.to_string().contains("private"), "{private}");
        assert!(
            !private.to_string().contains("with_proxy_resolved_names"),
            "{private}"
        );

        let internal = preflight_with(
            "http://metadata.internal/h",
            TEN,
            true,
            proxy_resolved_names,
            |_| answers(&[]),
        )
        .await
        .expect_err("an internal name");
        assert!(internal.to_string().contains("internal"), "{internal}");
        assert!(
            !internal.to_string().contains("with_proxy_resolved_names"),
            "{internal}"
        );
    }
}
