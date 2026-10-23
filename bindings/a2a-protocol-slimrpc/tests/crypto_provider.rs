// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! This crate links two rustls providers, SLIM's `aws-lc-rs` and the SDK's
//! `ring`, so rustls picks neither by itself and SLIM's first TLS handshake
//! would panic. `SlimRpcServerBuilder::build` installs SLIM's provider when
//! the process has none. Its own test binary, because the default is
//! process-global and the check needs a process where nothing installed one.

mod common;

use a2a_protocol_slimrpc::{SlimName, SlimRpcServer};
use rustls::crypto::CryptoProvider;

#[tokio::test]
async fn building_a_server_installs_a_rustls_provider() {
    assert!(
        CryptoProvider::get_default().is_none(),
        "this process started with a provider installed, so the test proves nothing"
    );
    let name = SlimName::new("org", "crypto", "agent");
    let _server = SlimRpcServer::builder(common::handler_for(&name), name)
        .with_shared_secret("agent", common::SECRET)
        .expect("secret")
        .build()
        .expect("server");
    assert!(
        CryptoProvider::get_default().is_some(),
        "SLIM's TLS would find no rustls provider"
    );
}
