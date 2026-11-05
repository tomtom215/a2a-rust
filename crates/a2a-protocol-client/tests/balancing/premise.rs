// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The measurement ADR 0032 asked for before a balancer was built: one
//! client speaking HTTP/2 through a layer-4 balancer puts every call on one
//! replica, because the balancer chooses per connection and HTTP/2 makes one
//! connection. The same calls through `with_balancing` reach every replica.

use a2a_protocol_client::balance::{BalancePolicy, StaticEndpoints};
use a2a_protocol_client::{ClientBuilder, HttpPool};

use std::net::SocketAddr;

use tokio::net::{TcpListener, TcpStream};

use super::harness::{counts, get, three, urls};

#[tokio::test]
async fn http2_through_a_layer4_balancer_lands_on_one_replica() {
    let replicas = three().await;
    let front = layer4(replicas.iter().map(|r| r.addr).collect()).await;
    let pool = HttpPool::builder()
        .with_http2_prior_knowledge()
        .build()
        .unwrap();
    let client = ClientBuilder::new(format!("http://{front}"))
        .with_pool(&pool)
        .build()
        .unwrap();
    for _ in 0..30 {
        get(&client).await.expect("answered");
    }
    let mut seen = counts(&replicas);
    seen.sort_unstable();
    assert_eq!(seen, vec![0, 0, 30], "one connection, one replica");
}

#[tokio::test]
async fn the_same_calls_balanced_reach_every_replica_evenly() {
    let replicas = three().await;
    let pool = HttpPool::builder()
        .with_http2_prior_knowledge()
        .build()
        .unwrap();
    let client = ClientBuilder::new(replicas[0].url())
        .with_pool(&pool)
        .with_balancing(StaticEndpoints::new(urls(&replicas)), BalancePolicy::new())
        .build()
        .unwrap();
    for _ in 0..30 {
        get(&client).await.expect("answered");
    }
    assert_eq!(counts(&replicas), vec![10, 10, 10]);
    for r in &replicas {
        assert_eq!(r.connections(), 1, "{}: one HTTP/2 connection", r.name);
    }
}

/// A layer-4 balancer: each new TCP connection goes to the next of
/// `upstreams`, round robin, and its bytes are copied both ways.
pub async fn layer4(upstreams: Vec<SocketAddr>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let mut next = 0;
        while let Ok((mut client, _)) = listener.accept().await {
            let upstream = upstreams[next % upstreams.len()];
            next += 1;
            tokio::spawn(async move {
                if let Ok(mut server) = TcpStream::connect(upstream).await {
                    let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
                }
            });
        }
    });
    addr
}
