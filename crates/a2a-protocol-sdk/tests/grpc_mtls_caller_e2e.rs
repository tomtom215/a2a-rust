// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! OWASP ASI07 on gRPC: the client certificate of a mutual-TLS call names
//! the caller through `MutualTlsAuthInterceptor`, and a call without one is
//! refused. The listener verifies client certificates but does not require
//! one, so a client without one reaches the interceptor.

#![cfg(feature = "grpc-tls")]

use std::sync::{Arc, Mutex};

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_client::error::ClientError;
use a2a_protocol_client::transport::grpc::{
    Certificate, ClientTlsConfig, GrpcTransport, GrpcTransportConfig, Identity,
};
use a2a_protocol_server::auth::MutualTlsAuthInterceptor;
use a2a_protocol_server::auth::caller_id::{certificate_fingerprint, mtls};
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::grpc::{GrpcConfig, GrpcDispatcher, ServerTlsConfig};
use a2a_protocol_server::{CallContext, ServerInterceptor, agent_executor};
use a2a_protocol_types::error::{A2aResult, ErrorCode};
use a2a_protocol_types::params::TaskQueryParams;

struct Idle;
agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });

/// Records the caller identity each admitted call carried.
#[derive(Default, Clone)]
struct Seen(Arc<Mutex<Vec<Option<String>>>>);

impl ServerInterceptor for Seen {
    fn before<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>> {
        self.0
            .lock()
            .unwrap()
            .push(ctx.caller_identity().map(str::to_owned));
        Box::pin(async { Ok(()) })
    }
    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn the_client_certificate_names_the_caller_over_grpc() {
    let mut ca_params = rcgen::CertificateParams::new(vec![]).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let ca = ca_params.self_signed(&ca_key).unwrap();
    let issuer = rcgen::Issuer::new(ca_params, ca_key);
    let issue = |name: &str| {
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = rcgen::CertificateParams::new(vec![name.into()])
            .unwrap()
            .signed_by(&key, &issuer)
            .unwrap();
        (cert, key)
    };
    let (server_cert, server_key) = issue("localhost");
    let (client_cert, client_key) = issue("billing-agent");

    let seen = Seen::default();
    let handler = Arc::new(
        RequestHandlerBuilder::new(Idle)
            .without_agent_card()
            .with_interceptor(MutualTlsAuthInterceptor::new())
            .with_interceptor(seen.clone())
            .build()
            .unwrap(),
    );
    let tls = ServerTlsConfig::new()
        .identity(a2a_protocol_server::dispatch::grpc::Identity::from_pem(
            server_cert.pem(),
            server_key.serialize_pem(),
        ))
        .client_ca_root(a2a_protocol_server::dispatch::grpc::Certificate::from_pem(
            ca.pem(),
        ))
        .client_auth_optional(true);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = GrpcDispatcher::new(handler, GrpcConfig::default())
        .with_tls(tls)
        .serve_with_listener(listener)
        .unwrap();

    for with_cert in [true, false] {
        let mut client_tls = ClientTlsConfig::new()
            .ca_certificate(Certificate::from_pem(ca.pem()))
            .domain_name("localhost");
        if with_cert {
            // The chain with its CA: the caller is named by the leaf.
            client_tls = client_tls.identity(Identity::from_pem(
                client_cert.pem() + &ca.pem(),
                client_key.serialize_pem(),
            ));
        }
        let url = format!("https://localhost:{}", addr.port());
        let transport = GrpcTransport::connect_with_config(
            url.clone(),
            GrpcTransportConfig::default().with_tls_config(client_tls),
        )
        .await
        .unwrap();
        let client = ClientBuilder::new(url)
            .with_custom_transport(transport)
            .build()
            .unwrap();
        let result = client.get_task(TaskQueryParams::new("nope")).await;
        if with_cert {
            match result {
                Err(ClientError::Protocol(e)) => assert_eq!(e.code, ErrorCode::TaskNotFound),
                other => panic!("admitted, then not found: {other:?}"),
            }
            assert_eq!(
                seen.0.lock().unwrap().pop(),
                Some(Some(mtls(&certificate_fingerprint(client_cert.der()))))
            );
        } else {
            // gRPC UNAUTHENTICATED, which the client reports as a 401.
            assert!(
                matches!(
                    result,
                    Err(ClientError::UnexpectedStatus { status: 401, .. })
                ),
                "refused: {result:?}"
            );
            assert!(seen.0.lock().unwrap().is_empty(), "{result:?}");
        }
    }
}
