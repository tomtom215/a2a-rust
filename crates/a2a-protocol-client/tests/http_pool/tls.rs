// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A pool has the one TLS configuration it was built with, which every
//! client given it uses (ADR 0032, decision 1).

#[path = "../support_tls/mod.rs"]
mod support_tls;

use a2a_protocol_client::tls::tls_config_from_pem;
use a2a_protocol_client::{ClientBuilder, HttpPool};

use support_tls::{ping, pki, serve_tls};

#[tokio::test]
async fn a_pool_trusts_what_its_tls_configuration_trusts() {
    let pki = pki();
    let port = serve_tls(&pki, false).await;
    let tls = tls_config_from_pem(Some(pki.ca_pem.as_bytes()), None).expect("tls");
    let pool = HttpPool::builder()
        .with_tls_config(tls)
        .build()
        .expect("pool");

    // Two clients, neither of which sets TLS itself: the pool's applies.
    for _ in 0..2 {
        let client = ClientBuilder::new(format!("https://127.0.0.1:{port}"))
            .with_pool(&pool)
            .build()
            .expect("client");
        client
            .send_message(ping())
            .await
            .expect("the private CA is trusted through the pool");
    }
}

#[tokio::test]
async fn a_pool_with_the_default_roots_does_not_trust_a_private_ca() {
    let pki = pki();
    let port = serve_tls(&pki, false).await;
    let pool = HttpPool::builder().build().expect("pool");
    let client = ClientBuilder::new(format!("https://127.0.0.1:{port}"))
        .with_pool(&pool)
        .build()
        .expect("client");

    client
        .send_message(ping())
        .await
        .expect_err("a certificate the default roots do not vouch for is refused");
}

#[tokio::test]
async fn a_pool_presents_the_client_certificate_in_its_tls_configuration() {
    let pki = pki();
    let port = serve_tls(&pki, true).await;
    let with_identity = tls_config_from_pem(
        Some(pki.ca_pem.as_bytes()),
        Some((
            pki.client_cert_pem.as_bytes(),
            pki.client_key_pem.as_bytes(),
        )),
    )
    .expect("tls");
    let without = tls_config_from_pem(Some(pki.ca_pem.as_bytes()), None).expect("tls");
    let url = format!("https://127.0.0.1:{port}");

    let mutual = HttpPool::builder()
        .with_tls_config(with_identity)
        .build()
        .expect("pool");
    ClientBuilder::new(&url)
        .with_pool(&mutual)
        .build()
        .expect("client")
        .send_message(ping())
        .await
        .expect("the server asked for a certificate and the pool gave one");

    let anonymous = HttpPool::builder()
        .with_tls_config(without)
        .build()
        .expect("pool");
    ClientBuilder::new(&url)
        .with_pool(&anonymous)
        .build()
        .expect("client")
        .send_message(ping())
        .await
        .expect_err("a pool without the certificate is refused by a server that wants one");
}
