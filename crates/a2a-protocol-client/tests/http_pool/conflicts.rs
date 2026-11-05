// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A client that is given a pool and also sets what the pool owns fails at
//! `build()` and names the setting, in whichever order it was called
//! (ADR 0032, decision 2).

use std::time::Duration;

use a2a_protocol_client::tls::default_tls_config;
use a2a_protocol_client::{ClientBuilder, ClientError, HttpPool};

const URL: &str = "http://127.0.0.1:1";

fn pool() -> HttpPool {
    HttpPool::builder().build().expect("pool")
}

fn refusal(builder: ClientBuilder) -> String {
    match builder.build() {
        Err(ClientError::Config(why)) => why,
        Err(other) => panic!("expected a configuration error, got {other}"),
        Ok(_) => panic!("expected a configuration error, but the client built"),
    }
}

#[test]
fn a_tls_configuration_and_a_pool_conflict_in_either_order() {
    let pool = pool();
    for builder in [
        ClientBuilder::new(URL)
            .with_tls_config(default_tls_config())
            .with_pool(&pool),
        ClientBuilder::new(URL)
            .with_pool(&pool)
            .with_tls_config(default_tls_config()),
    ] {
        let why = refusal(builder);
        assert!(
            why.contains("ClientBuilder::with_tls_config and ClientBuilder::with_pool"),
            "{why}"
        );
        assert!(
            why.contains("HttpPool::builder().with_tls_config(..)"),
            "it says where the setting goes: {why}"
        );
    }
}

#[test]
fn disabling_tls_and_a_pool_conflict_in_either_order() {
    let pool = pool();
    for builder in [
        ClientBuilder::new(URL).without_tls().with_pool(&pool),
        ClientBuilder::new(URL).with_pool(&pool).without_tls(),
    ] {
        let why = refusal(builder);
        assert!(
            why.contains("ClientBuilder::without_tls and ClientBuilder::with_pool"),
            "{why}"
        );
    }
}

#[test]
fn a_connect_timeout_and_a_pool_conflict_even_when_it_is_the_default_value() {
    let pool = pool();
    for builder in [
        ClientBuilder::new(URL)
            .with_connection_timeout(Duration::from_secs(10))
            .with_pool(&pool),
        ClientBuilder::new(URL)
            .with_pool(&pool)
            .with_connection_timeout(Duration::from_secs(3)),
    ] {
        let why = refusal(builder);
        assert!(
            why.contains("ClientBuilder::with_connection_timeout and ClientBuilder::with_pool"),
            "{why}"
        );
        assert!(
            why.contains("HttpPool::builder().with_connect_timeout(..)"),
            "{why}"
        );
    }
}

#[test]
fn the_last_tls_setter_is_the_one_named() {
    let pool = pool();
    let why = refusal(
        ClientBuilder::new(URL)
            .without_tls()
            .with_tls_config(default_tls_config())
            .with_pool(&pool),
    );
    assert!(why.contains("ClientBuilder::with_tls_config"), "{why}");
}

#[test]
fn a_pool_alone_and_a_pool_with_the_settings_it_does_not_own_build() {
    let pool = pool();
    ClientBuilder::new(URL)
        .with_pool(&pool)
        .build()
        .expect("a pool alone");
    ClientBuilder::new(URL)
        .with_pool(&pool)
        .with_timeout(Duration::from_secs(5))
        .with_max_in_flight(4)
        .build()
        .expect("per-client settings stay the client's");
}

#[test]
fn without_a_pool_the_same_settings_still_build() {
    ClientBuilder::new(URL)
        .with_tls_config(default_tls_config())
        .with_connection_timeout(Duration::from_secs(3))
        .build()
        .expect("nothing changed for a client that is not given a pool");
}
