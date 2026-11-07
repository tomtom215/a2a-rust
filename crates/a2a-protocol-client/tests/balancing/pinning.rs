// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `DnsEndpoints` pins each address of a name. The pins under TLS, and to
//! names that resolve to nothing, are `tests/pinned_endpoints.rs`, which
//! clears the proxy variables first (a non-loopback name would otherwise go
//! to the environment's proxy, which resolves it itself).

use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_client::balance::{BalancePolicy, DnsEndpoints, EndpointSource};
use tokio::net::TcpListener;

use super::harness::{Replica, get};

/// One listener on each address `localhost` resolves to, all on one port:
/// the addresses `DnsEndpoints` pins. A CI runner's `localhost` is `::1` and
/// `127.0.0.1`; a replica on one of them alone refuses the pin to the other,
/// which a call reaches in its turn.
async fn listeners_on_every_address_of_localhost() -> Vec<TcpListener> {
    let ips: Vec<IpAddr> = tokio::net::lookup_host(("localhost", 0))
        .await
        .expect("localhost resolves")
        .map(|addr| addr.ip())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    // A port free on one address may be taken on another: try again.
    for _ in 0..20 {
        let first = TcpListener::bind((ips[0], 0)).await.expect("bind");
        let port = first.local_addr().expect("addr").port();
        let mut listeners = vec![first];
        for ip in &ips[1..] {
            match TcpListener::bind((*ip, port)).await {
                Ok(listener) => listeners.push(listener),
                Err(_) => break,
            }
        }
        if listeners.len() == ips.len() {
            return listeners;
        }
    }
    panic!("no port was free on every address of localhost ({ips:?})");
}

/// `DnsEndpoints` on `localhost` pins every address the name resolves to,
/// each of which reaches the replica listening on it, under the name's
/// `Host`. Calls one after another go round the set (ADR 0050 decision 2),
/// so one call per address reaches each of them once.
#[tokio::test]
async fn dns_endpoints_reach_the_name_s_addresses() {
    let listeners = listeners_on_every_address_of_localhost().await;
    let port = listeners[0].local_addr().expect("addr").port();
    let replicas: Vec<_> = listeners
        .into_iter()
        .map(|listener| Replica::serve("a", listener))
        .collect();
    let url = format!("http://localhost:{port}");

    let pinned: BTreeSet<SocketAddr> = DnsEndpoints::new(url.clone())
        .unwrap()
        .endpoints()
        .await
        .expect("the name resolves")
        .iter()
        .map(|endpoint| endpoint.addr().expect("pinned"))
        .collect();
    let listening: BTreeSet<SocketAddr> = replicas.iter().map(|r| r.addr).collect();
    assert_eq!(pinned, listening, "one pin for each address of the name");

    let client = ClientBuilder::new(url.clone())
        .with_balancing(DnsEndpoints::new(url).unwrap(), BalancePolicy::new())
        .build()
        .unwrap();
    for _ in &replicas {
        assert_eq!(get(&client).await.expect("answered"), "a");
    }
    for replica in &replicas {
        assert_eq!(replica.requests(), 1, "{}", replica.addr);
        assert!(replica.hosts().iter().all(|h| h.starts_with("localhost")));
    }
}
