// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! TLS and mTLS integration tests.
//!
//! Uses `rcgen` to generate self-signed CA + server certificates at test time,
//! `tokio-rustls` to run a real TLS server, and the a2a-client with custom
//! root certificates to verify end-to-end TLS connectivity.
//!
//! # Feature gate
//!
//! These tests require `tls-rustls` to be enabled on `a2a-protocol-client`.

#![cfg(feature = "tls-rustls")]

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use a2a_protocol_types::jsonrpc::{
    JsonRpcError, JsonRpcErrorResponse, JsonRpcRequest, JsonRpcSuccessResponse, JsonRpcVersion,
};

use a2a_protocol_client::tls;

// ── Certificate generation ──────────────────────────────────────────────────

struct TestCerts {
    ca_cert_der: rustls_pki_types::CertificateDer<'static>,
    server_cert_der: rustls_pki_types::CertificateDer<'static>,
    server_key_der: rustls_pki_types::PrivateKeyDer<'static>,
}

fn generate_test_certs(san: &str) -> TestCerts {
    // Generate CA
    let mut ca_params = rcgen::CertificateParams::new(vec![]).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Test CA");
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();

    // Generate server cert signed by CA
    let mut server_params = rcgen::CertificateParams::new(vec![san.into()]).unwrap();
    server_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, san);
    let server_key = rcgen::KeyPair::generate().unwrap();
    let server_cert = server_params
        .signed_by(&server_key, &ca_cert, &ca_key)
        .unwrap();

    TestCerts {
        ca_cert_der: ca_cert.der().clone(),
        server_cert_der: server_cert.der().clone(),
        server_key_der: rustls_pki_types::PrivateKeyDer::Pkcs8(
            rustls_pki_types::PrivatePkcs8KeyDer::from(server_key.serialize_der()),
        ),
    }
}

// ── TLS server helper ───────────────────────────────────────────────────────

async fn start_tls_server(certs: &TestCerts) -> SocketAddr {
    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![certs.server_cert_der.clone()],
            certs.server_key_der.clone_key(),
        )
        .expect("build server TLS config");

    let acceptor = TlsAcceptor::from(Arc::new(server_config));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(tls_stream) = acceptor.accept(stream).await else {
                    return;
                };
                let io = TokioIo::new(tls_stream);
                let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                    .serve_connection(io, service_fn(echo_handler))
                    .await;
            });
        }
    });

    addr
}

/// Simple JSON-RPC handler that echoes back the method name.
async fn echo_handler(
    req: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let body_bytes = req.collect().await.unwrap().to_bytes();
    let rpc_req: Result<JsonRpcRequest, _> = serde_json::from_slice(&body_bytes);

    let resp_body = match rpc_req {
        Ok(rpc) => {
            let success = JsonRpcSuccessResponse {
                jsonrpc: JsonRpcVersion,
                id: rpc.id,
                result: serde_json::json!({
                    "method": rpc.method,
                    "echo": true,
                }),
            };
            serde_json::to_vec(&success).unwrap()
        }
        Err(e) => {
            let err_resp = JsonRpcErrorResponse::new(
                None,
                JsonRpcError::new(-32700, format!("parse error: {e}")),
            );
            serde_json::to_vec(&err_resp).unwrap()
        }
    };

    Ok(Response::builder()
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(resp_body)))
        .unwrap())
}

// ── TLS Tests ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn tls_client_connects_to_tls_server_with_custom_ca() {
    let certs = generate_test_certs("localhost");
    let addr = start_tls_server(&certs).await;

    // Build client with the test CA in its root store.
    let tls_config = tls::tls_config_with_extra_roots(vec![certs.ca_cert_der.clone()]);
    let client = tls::build_https_client_with_config(tls_config);

    let rpc_req = a2a_protocol_types::JsonRpcRequest::with_params(
        serde_json::json!("tls-1"),
        "TestMethod",
        serde_json::json!({}),
    );
    let body_bytes = serde_json::to_vec(&rpc_req).unwrap();

    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(format!("https://localhost:{}", addr.port()))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body_bytes)))
        .unwrap();

    let resp = client
        .request(req)
        .await
        .expect("TLS request should succeed");
    assert!(resp.status().is_success());

    let body = resp.collect().await.unwrap().to_bytes();
    let success: JsonRpcSuccessResponse<serde_json::Value> =
        serde_json::from_slice(&body).expect("valid JSON-RPC response");
    assert_eq!(success.id, Some(serde_json::json!("tls-1")));
    assert_eq!(success.result["method"], "TestMethod");
    assert_eq!(success.result["echo"], true);
}

#[tokio::test]
async fn tls_client_rejects_unknown_ca() {
    let certs = generate_test_certs("localhost");
    let _addr = start_tls_server(&certs).await;

    // Build client with default Mozilla roots only — our self-signed CA is not trusted.
    let tls_config = tls::default_tls_config();
    let client = tls::build_https_client_with_config(tls_config);

    let rpc_req = a2a_protocol_types::JsonRpcRequest::with_params(
        serde_json::json!("tls-2"),
        "TestMethod",
        serde_json::json!({}),
    );
    let body_bytes = serde_json::to_vec(&rpc_req).unwrap();

    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(format!("https://localhost:{}", _addr.port()))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body_bytes)))
        .unwrap();

    // Should fail because the self-signed CA is not in the default root store.
    let result = client.request(req).await;
    assert!(result.is_err(), "should reject connection with unknown CA");
}

#[tokio::test]
async fn tls_sni_with_matching_hostname() {
    // Server cert has SAN = "localhost"
    let certs = generate_test_certs("localhost");
    let addr = start_tls_server(&certs).await;

    let tls_config = tls::tls_config_with_extra_roots(vec![certs.ca_cert_der.clone()]);
    let client = tls::build_https_client_with_config(tls_config);

    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(format!("https://localhost:{}/", addr.port()))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(b"{}" as &[u8])))
        .unwrap();

    // Should succeed because SNI matches the cert SAN.
    let result = client.request(req).await;
    assert!(result.is_ok(), "SNI matching should succeed: {result:?}");
}

#[tokio::test]
async fn tls_sni_with_mismatched_hostname() {
    // Server cert has SAN = "not-localhost"
    let certs = generate_test_certs("not-localhost.example.com");
    let addr = start_tls_server(&certs).await;

    let tls_config = tls::tls_config_with_extra_roots(vec![certs.ca_cert_der.clone()]);
    let client = tls::build_https_client_with_config(tls_config);

    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        // Connect to localhost but cert says "not-localhost.example.com"
        .uri(format!("https://localhost:{}/", addr.port()))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(b"{}" as &[u8])))
        .unwrap();

    // Should fail because hostname doesn't match the cert SAN.
    let result = client.request(req).await;
    assert!(
        result.is_err(),
        "should reject connection with mismatched hostname"
    );
}

// ── mTLS Tests ──────────────────────────────────────────────────────────────

struct MtlsCerts {
    ca_cert_der: rustls_pki_types::CertificateDer<'static>,
    server_cert_der: rustls_pki_types::CertificateDer<'static>,
    server_key_der: rustls_pki_types::PrivateKeyDer<'static>,
    client_cert_der: rustls_pki_types::CertificateDer<'static>,
    client_key_der: rustls_pki_types::PrivateKeyDer<'static>,
}

fn generate_mtls_certs() -> MtlsCerts {
    // Generate CA
    let mut ca_params = rcgen::CertificateParams::new(vec![]).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "mTLS Test CA");
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();

    // Server cert
    let mut server_params = rcgen::CertificateParams::new(vec!["localhost".into()]).unwrap();
    server_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "localhost");
    let server_key = rcgen::KeyPair::generate().unwrap();
    let server_cert = server_params
        .signed_by(&server_key, &ca_cert, &ca_key)
        .unwrap();

    // Client cert
    let mut client_params = rcgen::CertificateParams::new(vec![]).unwrap();
    client_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "test-client");
    let client_key = rcgen::KeyPair::generate().unwrap();
    let client_cert = client_params
        .signed_by(&client_key, &ca_cert, &ca_key)
        .unwrap();

    MtlsCerts {
        ca_cert_der: ca_cert.der().clone(),
        server_cert_der: server_cert.der().clone(),
        server_key_der: rustls_pki_types::PrivateKeyDer::Pkcs8(
            rustls_pki_types::PrivatePkcs8KeyDer::from(server_key.serialize_der()),
        ),
        client_cert_der: client_cert.der().clone(),
        client_key_der: rustls_pki_types::PrivateKeyDer::Pkcs8(
            rustls_pki_types::PrivatePkcs8KeyDer::from(client_key.serialize_der()),
        ),
    }
}

async fn start_mtls_server(certs: &MtlsCerts) -> SocketAddr {
    let mut root_store = rustls::RootCertStore::empty();
    root_store.add(certs.ca_cert_der.clone()).unwrap();
    let client_auth = rustls::server::WebPkiClientVerifier::builder(Arc::new(root_store))
        .build()
        .unwrap();

    let server_config = rustls::ServerConfig::builder()
        .with_client_cert_verifier(client_auth)
        .with_single_cert(
            vec![certs.server_cert_der.clone()],
            certs.server_key_der.clone_key(),
        )
        .expect("build mTLS server config");

    let acceptor = TlsAcceptor::from(Arc::new(server_config));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(tls_stream) = acceptor.accept(stream).await else {
                    return;
                };
                let io = TokioIo::new(tls_stream);
                let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                    .serve_connection(io, service_fn(echo_handler))
                    .await;
            });
        }
    });

    addr
}

#[tokio::test]
async fn mtls_client_with_valid_cert_succeeds() {
    let certs = generate_mtls_certs();
    let addr = start_mtls_server(&certs).await;

    // Build client with CA trust + client certificate.
    let mut root_store = rustls::RootCertStore::empty();
    root_store.add(certs.ca_cert_der.clone()).unwrap();

    let client_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_client_auth_cert(
            vec![certs.client_cert_der.clone()],
            certs.client_key_der.clone_key(),
        )
        .expect("build mTLS client config");

    let client = tls::build_https_client_with_config(client_config);

    let rpc_req = a2a_protocol_types::JsonRpcRequest::with_params(
        serde_json::json!("mtls-1"),
        "SecureMethod",
        serde_json::json!({}),
    );
    let body_bytes = serde_json::to_vec(&rpc_req).unwrap();

    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(format!("https://localhost:{}/", addr.port()))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body_bytes)))
        .unwrap();

    let resp = client
        .request(req)
        .await
        .expect("mTLS request should succeed");
    assert!(resp.status().is_success());
}

#[tokio::test]
async fn mtls_client_without_cert_is_rejected() {
    let certs = generate_mtls_certs();
    let addr = start_mtls_server(&certs).await;

    // Build client with CA trust but NO client certificate.
    let tls_config = tls::tls_config_with_extra_roots(vec![certs.ca_cert_der.clone()]);
    let client = tls::build_https_client_with_config(tls_config);

    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(format!("https://localhost:{}/", addr.port()))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(b"{}" as &[u8])))
        .unwrap();

    // Should fail because server requires client cert and we didn't provide one.
    let result = client.request(req).await;
    assert!(
        result.is_err(),
        "should reject connection without client certificate"
    );
}

#[tokio::test]
async fn mtls_client_with_wrong_ca_cert_is_rejected() {
    let certs = generate_mtls_certs();
    let addr = start_mtls_server(&certs).await;

    // Generate a rogue client cert signed by a different CA.
    let mut rogue_ca_params = rcgen::CertificateParams::new(vec![]).unwrap();
    rogue_ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    rogue_ca_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Rogue CA");
    let rogue_ca_key = rcgen::KeyPair::generate().unwrap();
    let rogue_ca_cert = rogue_ca_params.self_signed(&rogue_ca_key).unwrap();

    let mut rogue_client_params = rcgen::CertificateParams::new(vec![]).unwrap();
    rogue_client_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "rogue-client");
    let rogue_client_key = rcgen::KeyPair::generate().unwrap();
    let rogue_client_cert = rogue_client_params
        .signed_by(&rogue_client_key, &rogue_ca_cert, &rogue_ca_key)
        .unwrap();

    // Build client with correct server CA trust but rogue client cert.
    let mut root_store = rustls::RootCertStore::empty();
    root_store.add(certs.ca_cert_der.clone()).unwrap();

    let client_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_client_auth_cert(
            vec![rogue_client_cert.der().clone()],
            rustls_pki_types::PrivateKeyDer::Pkcs8(rustls_pki_types::PrivatePkcs8KeyDer::from(
                rogue_client_key.serialize_der(),
            )),
        )
        .expect("build rogue client config");

    let client = tls::build_https_client_with_config(client_config);

    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(format!("https://localhost:{}/", addr.port()))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(b"{}" as &[u8])))
        .unwrap();

    // Should fail because the client cert isn't signed by the server's trusted CA.
    let result = client.request(req).await;
    assert!(
        result.is_err(),
        "should reject connection with client cert from wrong CA"
    );
}
