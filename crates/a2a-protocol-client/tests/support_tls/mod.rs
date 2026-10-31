// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A private CA, an agent served over TLS by it, and a client identity it
//! signs, for the private-CA and proxy tests.

#![allow(dead_code)]

use std::sync::Arc;

use a2a_protocol_server::agent_executor;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::CombinedDispatcher;
use a2a_protocol_server::serve::Server;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;

pub struct Pki {
    pub ca_pem: String,
    pub server_cert_pem: String,
    pub server_key_pem: String,
    pub client_cert_pem: String,
    pub client_key_pem: String,
    pub ca_der: rustls_pki_types::CertificateDer<'static>,
}

/// A CA, a server certificate for `localhost` and `127.0.0.1`, and a client
/// certificate, all signed by the CA.
pub fn pki() -> Pki {
    let mut ca_params = rcgen::CertificateParams::new(vec![]).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Private Test CA");
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let issuer = rcgen::Issuer::new(ca_params, ca_key);

    let server_params =
        rcgen::CertificateParams::new(vec!["localhost".into(), "127.0.0.1".into()]).unwrap();
    let server_key = rcgen::KeyPair::generate().unwrap();
    let server_cert = server_params.signed_by(&server_key, &issuer).unwrap();

    let mut client_params = rcgen::CertificateParams::new(vec!["caller".into()]).unwrap();
    client_params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ClientAuth];
    let client_key = rcgen::KeyPair::generate().unwrap();
    let client_cert = client_params.signed_by(&client_key, &issuer).unwrap();

    Pki {
        ca_pem: ca_cert.pem(),
        server_cert_pem: server_cert.pem(),
        server_key_pem: server_key.serialize_pem(),
        client_cert_pem: client_cert.pem(),
        client_key_pem: client_key.serialize_pem(),
        ca_der: ca_cert.der().clone(),
    }
}

struct Echo;
agent_executor!(Echo, |ctx, queue| async {
    let emit = a2a_protocol_server::EventEmitter::new(ctx, queue);
    emit.artifact("echo", vec![Part::text("pong")], None, Some(true))
        .await?;
    emit.status(a2a_protocol_types::task::TaskState::Completed)
        .await
});

/// Serves JSON-RPC and HTTP+JSON over TLS with `pki`'s server certificate,
/// asking for a client certificate the CA signed when `mutual`. Returns the
/// port.
pub async fn serve_tls(pki: &Pki, mutual: bool) -> u16 {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ServerConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .unwrap();
    let builder = if mutual {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(pki.ca_der.clone()).unwrap();
        let verifier =
            rustls::server::WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider)
                .build()
                .unwrap();
        builder.with_client_cert_verifier(verifier)
    } else {
        builder.with_no_client_auth()
    };
    use rustls_pki_types::pem::PemObject;
    let chain = rustls_pki_types::CertificateDer::pem_slice_iter(pki.server_cert_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let key =
        rustls_pki_types::PrivateKeyDer::from_pem_slice(pki.server_key_pem.as_bytes()).unwrap();
    let mut config = builder.with_single_cert(chain, key).unwrap();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    let handler = Arc::new(
        RequestHandlerBuilder::new(Echo)
            .without_agent_card()
            .build()
            .unwrap(),
    );
    let server = Server::bind("127.0.0.1:0").await.unwrap().with_tls(config);
    let port = server.local_addr().unwrap().port();
    tokio::spawn(
        server.serve_with_shutdown(CombinedDispatcher::new(handler), std::future::pending()),
    );
    port
}

pub fn ping() -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        message: Message {
            id: MessageId::new("m1"),
            role: MessageRole::User,
            parts: vec![Part::text("ping")],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    }
}
