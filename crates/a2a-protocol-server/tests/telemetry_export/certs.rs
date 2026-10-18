// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The private CA the TLS collectors present certificates from, and the
//! client certificate the mutual-TLS ones require.

/// A private CA, a `localhost` certificate it signed, and a client
/// certificate it signed.
pub struct TestCerts {
    pub ca_pem: String,
    pub ca_cert: rustls_pki_types::CertificateDer<'static>,
    pub client_cert_pem: String,
    pub client_key_pem: String,
    pub server_cert_pem: String,
    pub server_key_pem: String,
    pub server_cert: rustls_pki_types::CertificateDer<'static>,
    pub server_key: rustls_pki_types::PrivateKeyDer<'static>,
}

pub fn test_certs() -> TestCerts {
    let mut ca_params = rcgen::CertificateParams::new(vec![]).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Telemetry Test CA");
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let issuer = rcgen::Issuer::new(ca_params, ca_key);
    let mut server_params = rcgen::CertificateParams::new(vec!["localhost".into()]).unwrap();
    server_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "localhost");
    let server_key = rcgen::KeyPair::generate().unwrap();
    let server_cert = server_params.signed_by(&server_key, &issuer).unwrap();
    let mut client_params = rcgen::CertificateParams::new(vec![]).unwrap();
    client_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "telemetry-test-client");
    client_params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ClientAuth];
    let client_key = rcgen::KeyPair::generate().unwrap();
    let client_cert = client_params.signed_by(&client_key, &issuer).unwrap();
    TestCerts {
        ca_pem: ca_cert.pem(),
        ca_cert: ca_cert.der().clone(),
        client_cert_pem: client_cert.pem(),
        client_key_pem: client_key.serialize_pem(),
        server_cert_pem: server_cert.pem(),
        server_key_pem: server_key.serialize_pem(),
        server_cert: server_cert.der().clone(),
        server_key: rustls_pki_types::PrivateKeyDer::try_from(server_key.serialize_der()).unwrap(),
    }
}
