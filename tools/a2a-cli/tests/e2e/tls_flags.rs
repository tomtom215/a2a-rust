// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `--cacert`, `--client-cert` and `--client-key` (audit B-15).
//!
//! The tool had no way to trust a private CA or present a client certificate
//! except `SSL_CERT_FILE`, which covers the first only. These run the binary
//! against an agent over TLS whose CA is made here and is in no store.

use std::sync::Arc;

use a2a_protocol_server::dispatch::CombinedDispatcher;
use a2a_protocol_server::serve::Server;
use rustls_pki_types::pem::PemObject;

use super::*;

struct Pki {
    ca_pem: String,
    ca_der: rustls_pki_types::CertificateDer<'static>,
    server_cert_pem: String,
    server_key_pem: String,
    client_cert_pem: String,
    client_key_pem: String,
}

/// A CA, a `localhost` server certificate and a client certificate it signed.
fn pki() -> Pki {
    let mut ca_params = rcgen::CertificateParams::new(vec![]).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "CLI Test CA");
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let ca = ca_params.self_signed(&ca_key).unwrap();
    let issuer = rcgen::Issuer::new(ca_params, ca_key);
    let server_key = rcgen::KeyPair::generate().unwrap();
    let server = rcgen::CertificateParams::new(vec!["localhost".into()])
        .unwrap()
        .signed_by(&server_key, &issuer)
        .unwrap();
    let mut client_params = rcgen::CertificateParams::new(vec!["caller".into()]).unwrap();
    client_params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ClientAuth];
    let client_key = rcgen::KeyPair::generate().unwrap();
    let client = client_params.signed_by(&client_key, &issuer).unwrap();
    Pki {
        ca_pem: ca.pem(),
        ca_der: ca.der().clone(),
        server_cert_pem: server.pem(),
        server_key_pem: server_key.serialize_pem(),
        client_cert_pem: client.pem(),
        client_key_pem: client_key.serialize_pem(),
    }
}

/// The agent over TLS on `localhost`, asking for a client certificate the CA
/// signed when `mutual`. Returns its base URL.
async fn start_tls_agent(pki: &Pki, mutual: bool) -> String {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ServerConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .unwrap();
    let builder = if mutual {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(pki.ca_der.clone()).unwrap();
        builder.with_client_cert_verifier(
            rustls::server::WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider)
                .build()
                .unwrap(),
        )
    } else {
        builder.with_no_client_auth()
    };
    let chain = rustls_pki_types::CertificateDer::pem_slice_iter(pki.server_cert_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let key =
        rustls_pki_types::PrivateKeyDer::from_pem_slice(pki.server_key_pem.as_bytes()).unwrap();
    let mut config = builder.with_single_cert(chain, key).unwrap();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    let server = Server::bind("127.0.0.1:0").await.unwrap().with_tls(config);
    let port = server.local_addr().unwrap().port();
    let url = format!("https://localhost:{port}");
    let handler = Arc::new(
        RequestHandlerBuilder::new(EchoAgent)
            .with_agent_card(AgentCard {
                url: Some(url.clone()),
                name: "cli-tls-agent".into(),
                version: "0.0.0".into(),
                description: "echo agent over TLS".into(),
                supported_interfaces: vec![AgentInterface::jsonrpc(&url)],
                provider: None,
                icon_url: None,
                documentation_url: None,
                capabilities: AgentCapabilities::none(),
                security_schemes: None,
                security_requirements: None,
                default_input_modes: vec!["text/plain".into()],
                default_output_modes: vec!["text/plain".into()],
                skills: vec![
                    a2a_protocol_types::AgentSkill::new("test", "Test", "A test skill")
                        .with_tags(["test"]),
                ],
                signatures: None,
            })
            .build()
            .unwrap(),
    );
    tokio::spawn(
        server.serve_with_shutdown(CombinedDispatcher::new(handler), std::future::pending()),
    );
    url
}

/// The test's files, in a directory of its own.
struct Files(std::path::PathBuf);

impl Files {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("a2a-cli-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn write(&self, name: &str, contents: &str) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, contents).unwrap();
        path.to_str().unwrap().to_owned()
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A private CA is refused by default and trusted with `--cacert`, for the card
/// fetch and the call after it.
#[tokio::test(flavor = "multi_thread")]
async fn cacert_trusts_a_private_ca_for_discovery_and_the_call() {
    let pki = pki();
    let url = start_tls_agent(&pki, false).await;
    let files = Files::new("cacert");
    let ca = files.write("ca.pem", &pki.ca_pem);

    let refused = a2a(&["send", &url, "hello"]).await;
    assert_eq!(code(&refused), 1, "{}", stderr(&refused));
    assert!(
        stderr(&refused).contains("UnknownIssuer"),
        "the cause is named: {}",
        stderr(&refused)
    );

    let answered = a2a(&["--cacert", &ca, "send", &url, "hello"]).await;
    assert_eq!(code(&answered), 0, "stderr: {}", stderr(&answered));
    assert_eq!(
        json(&answered)["task"]["artifacts"][0]["parts"][0]["text"],
        "Echo: hello"
    );

    // The flag works after the subcommand too, as every global flag does, and
    // for `card` as well.
    let card = a2a(&["card", &url, "--cacert", &ca]).await;
    assert_eq!(code(&card), 0, "stderr: {}", stderr(&card));
    assert_eq!(json(&card)["name"], "cli-tls-agent");
}

/// An agent that asks for a client certificate gets the one `--client-cert` and
/// `--client-key` name, and refuses a client without it.
#[tokio::test(flavor = "multi_thread")]
async fn cert_and_key_present_a_client_certificate() {
    let pki = pki();
    let url = start_tls_agent(&pki, true).await;
    let files = Files::new("mtls");
    let ca = files.write("ca.pem", &pki.ca_pem);
    let cert = files.write("client.pem", &pki.client_cert_pem);
    let key = files.write("client.key", &pki.client_key_pem);

    let without = a2a(&["--binding", "jsonrpc", "--cacert", &ca, "send", &url, "hi"]).await;
    assert_eq!(code(&without), 1, "the agent requires a certificate");

    let mutual = a2a(&[
        "--binding",
        "jsonrpc",
        "--cacert",
        &ca,
        "--client-cert",
        &cert,
        "--client-key",
        &key,
        "send",
        &url,
        "hi",
    ])
    .await;
    assert_eq!(code(&mutual), 0, "stderr: {}", stderr(&mutual));
    assert_eq!(
        json(&mutual)["task"]["artifacts"][0]["parts"][0]["text"],
        "Echo: hi"
    );
}

/// What is wrong with the flags is a usage error (exit 2), named, before
/// anything is sent.
#[tokio::test(flavor = "multi_thread")]
async fn bad_tls_flags_are_usage_errors_before_anything_is_sent() {
    let pki = pki();
    let files = Files::new("usage");
    let ca = files.write("ca.pem", &pki.ca_pem);
    let cert = files.write("client.pem", &pki.client_cert_pem);
    let key = files.write("client.key", &pki.client_key_pem);
    let junk = files.write("junk.pem", "this is not a certificate\n");
    let other_key = {
        let other = pki_key_pem();
        files.write("other.key", &other)
    };
    // Nothing listens on port 1: a failure to connect would exit 1.
    let url = "https://localhost:1";

    for (args, wanted) in [
        (
            vec!["--cacert", "/nonexistent/ca.pem", "send", url, "x"],
            "cannot read",
        ),
        (
            vec!["--cacert", &junk, "send", url, "x"],
            "no PEM certificate",
        ),
        (
            vec!["--client-cert", &cert, "send", url, "x"],
            "--client-key",
        ),
        (
            vec!["--client-key", &key, "send", url, "x"],
            "--client-cert",
        ),
        (
            vec![
                "--cacert",
                &ca,
                "--client-cert",
                &cert,
                "--client-key",
                &other_key,
                "send",
                url,
                "x",
            ],
            "--client-key",
        ),
    ] {
        let out = a2a(&args).await;
        assert_eq!(code(&out), 2, "{args:?}: {}", stderr(&out));
        assert!(stderr(&out).contains(wanted), "{args:?}: {}", stderr(&out));
    }
}

/// A key that is not any of the test certificates'.
fn pki_key_pem() -> String {
    rcgen::KeyPair::generate().unwrap().serialize_pem()
}
