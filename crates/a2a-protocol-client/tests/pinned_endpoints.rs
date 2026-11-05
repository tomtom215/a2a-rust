// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Endpoints pinned to an address keep their host name (ADR 0050): the
//! `Host` header, the TLS server name and the certificate check use the
//! URL's host, and only the TCP connection goes to the address. The names
//! here resolve to nothing, so a call that succeeds went where the pin said.
//!
//! A name that is not loopback goes to the environment's proxy, which
//! resolves it itself and makes the pin moot (documented in
//! `book/src/client/load-balancers.md`). So this binary clears the proxy
//! variables first, and holds one test, because it writes the process
//! environment.

#![cfg(feature = "tls-rustls")]

mod support_tls;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use a2a_protocol_client::balance::{BalancePolicy, Endpoint, StaticEndpoints};
use a2a_protocol_client::{A2aClient, ClientBuilder, HttpPool};
use a2a_protocol_types::{Task, TaskQueryParams, TaskState, TaskStatus};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;

/// The `Host` header of each request a replica answered, and how many
/// connections it accepted.
type Seen = (Arc<Mutex<Vec<String>>>, Arc<AtomicUsize>);

/// A JSON-RPC agent on `ip` answering `GetTask` with a task named `name`.
async fn replica(name: &'static str, ip: &str) -> (SocketAddr, Seen) {
    let listener = tokio::net::TcpListener::bind((ip, 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hosts: Arc<Mutex<Vec<String>>> = Arc::default();
    let accepted = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&hosts);
    let count = Arc::clone(&accepted);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            let seen = Arc::clone(&seen);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req: hyper::Request<_>| {
                    let seen = Arc::clone(&seen);
                    async move {
                        let host = req.headers().get(hyper::header::HOST).cloned();
                        seen.lock().unwrap().push(
                            host.and_then(|h| h.to_str().ok().map(str::to_owned))
                                .unwrap_or_default(),
                        );
                        let body: hyper::body::Incoming = req.into_body();
                        let rpc: serde_json::Value =
                            serde_json::from_slice(&body.collect().await.unwrap().to_bytes())
                                .unwrap();
                        let task = Task::new(name, "ctx", TaskStatus::new(TaskState::Completed));
                        let reply = serde_json::json!({
                            "jsonrpc": "2.0", "id": rpc["id"], "result": task
                        });
                        Ok::<_, std::convert::Infallible>(
                            hyper::Response::builder()
                                .header("content-type", "application/json")
                                .body(Full::new(Bytes::from(reply.to_string())))
                                .unwrap(),
                        )
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    (addr, (hosts, accepted))
}

async fn get(client: &A2aClient) -> String {
    client
        .get_task(TaskQueryParams::new("t"))
        .await
        .expect("answered")
        .id
        .to_string()
}

/// A CA and a server certificate for `name` only.
fn pki_for(name: &str) -> support_tls::Pki {
    let mut ca_params = rcgen::CertificateParams::new(vec![]).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let issuer = rcgen::Issuer::new(ca_params, ca_key);
    let server_key = rcgen::KeyPair::generate().unwrap();
    let server_cert = rcgen::CertificateParams::new(vec![name.into()])
        .unwrap()
        .signed_by(&server_key, &issuer)
        .unwrap();
    support_tls::Pki {
        ca_pem: ca_cert.pem(),
        server_cert_pem: server_cert.pem(),
        server_key_pem: server_key.serialize_pem(),
        client_cert_pem: String::new(),
        client_key_pem: String::new(),
        ca_der: ca_cert.der().clone(),
    }
}

/// Two addresses behind one name over plain HTTP, then TLS to a pinned
/// address with the name's certificate, through a pool and without one, and
/// a name the certificate does not carry refused.
#[test]
fn pinned_endpoints_keep_their_name() {
    // SAFETY: this binary holds this one test, and nothing reads or writes
    // the environment while it clears these: no runtime has started yet.
    unsafe {
        for name in [
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            std::env::remove_var(name);
        }
    }
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        // Two addresses: 127.0.0.1 on two ports. Not 127.0.0.2: Linux and
        // Windows answer for all of 127.0.0.0/8, but macOS configures only
        // 127.0.0.1 and refuses to bind another (`AddrNotAvailable`). No port
        // in the URL: each pinned address's own is used.
        let (a, (a_hosts, a_accepted)) = replica("a", "127.0.0.1").await;
        let (b, (b_hosts, _)) = replica("b", "127.0.0.1").await;
        let url = "http://replica.invalid";
        let client = ClientBuilder::new(url)
            .with_balancing(
                StaticEndpoints::from_endpoints(vec![
                    Endpoint::pinned(url, a),
                    Endpoint::pinned(url, b),
                ]),
                BalancePolicy::new(),
            )
            .build()
            .unwrap();
        let mut order = Vec::new();
        for _ in 0..4 {
            order.push(get(&client).await);
        }
        assert_eq!(order, ["a", "b", "a", "b"]);
        for hosts in [a_hosts, b_hosts] {
            assert_eq!(
                *hosts.lock().unwrap(),
                ["replica.invalid", "replica.invalid"]
            );
        }

        // Two clients of one pool pinned to one address share its
        // connections: one more connection to `a`, not two.
        let pool = HttpPool::builder().build().unwrap();
        let before = a_accepted.load(Ordering::SeqCst);
        for _ in 0..2 {
            let client = ClientBuilder::new(url)
                .with_pool(&pool)
                .with_balancing(
                    StaticEndpoints::from_endpoints(vec![Endpoint::pinned(url, a)]),
                    BalancePolicy::new(),
                )
                .build()
                .unwrap();
            assert_eq!(get(&client).await, "a");
        }
        assert_eq!(a_accepted.load(Ordering::SeqCst), before + 1);

        // One-label names: the agent answers loopback only for those (its
        // DNS-rebinding guard); they resolve to nothing here either.
        let pki = pki_for("replica");
        let port = support_tls::serve_tls(&pki, false).await;
        let tls = || {
            a2a_protocol_client::tls::tls_config_from_pem(Some(pki.ca_pem.as_bytes()), None)
                .unwrap()
        };
        let pool = HttpPool::builder().with_tls_config(tls()).build().unwrap();
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        let pinned = |url: &str| StaticEndpoints::from_endpoints(vec![Endpoint::pinned(url, addr)]);
        let url = format!("https://replica:{port}");
        let pooled = ClientBuilder::new(url.clone())
            .with_pool(&pool)
            .with_balancing(pinned(&url), BalancePolicy::new())
            .build()
            .unwrap();
        let own = ClientBuilder::new(url.clone())
            .with_tls_config(tls())
            .with_balancing(pinned(&url), BalancePolicy::new())
            .build()
            .unwrap();
        for client in [pooled, own] {
            client
                .send_message(support_tls::ping())
                .await
                .expect("pinned, and the certificate's name checked");
        }
        let wrong = format!("https://other:{port}");
        let client = ClientBuilder::new(wrong.clone())
            .with_pool(&pool)
            .with_balancing(pinned(&wrong), BalancePolicy::new())
            .build()
            .unwrap();
        let err = client
            .send_message(support_tls::ping())
            .await
            .expect_err("the certificate does not name other");
        assert!(
            err.to_string()
                .contains("certificate not valid for name \"other\""),
            "{err:?}"
        );
    });
}
