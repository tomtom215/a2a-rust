// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `DnsEndpoints` pins each address of a name. The pins under TLS, and to
//! names that resolve to nothing, are `tests/pinned_endpoints.rs`, which
//! clears the proxy variables first (a non-loopback name would otherwise go
//! to the environment's proxy, which resolves it itself).

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_client::balance::{BalancePolicy, DnsEndpoints};

use super::harness::{Replica, get};

/// `DnsEndpoints` on `localhost` pins its loopback address and reaches the
/// replica through it.
#[tokio::test]
async fn dns_endpoints_reach_the_name_s_addresses() {
    let replica = Replica::start("a").await;
    let url = format!("http://localhost:{}", replica.addr.port());
    let client = ClientBuilder::new(url.clone())
        .with_balancing(DnsEndpoints::new(url).unwrap(), BalancePolicy::new())
        .build()
        .unwrap();
    assert_eq!(get(&client).await.expect("answered"), "a");
    assert!(replica.hosts().iter().all(|h| h.starts_with("localhost")));
}
