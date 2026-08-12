// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Delivering push notifications over HTTPS to a private CA.

use super::Check;

const LABEL: &str = "HTTPS push (private CA trusted, untrusted cert rejected)";

/// Delivers a push notification over TLS to a certificate the sender was told
/// to trust, and requires the *same* endpoint to be rejected by the default
/// trust store.
///
/// Both halves are the check. Delivering successfully to a webhook proves TLS
/// is wired; it does not prove the certificate was verified, and a sender that
/// accepted any certificate would pass that half while being exactly the thing
/// [`HttpPushSender::with_tls_config`] exists to avoid. The second delivery —
/// same URL, same server, only the trust store swapped for Mozilla's roots —
/// must fail, because a self-signed certificate no public CA issued is
/// precisely what a real deployment needs rejected.
///
/// [`HttpPushSender::with_tls_config`]: a2a_protocol_server::push::HttpPushSender::with_tls_config
#[cfg(feature = "tls-rustls")]
pub(super) async fn https_push() -> Check {
    use std::time::Duration;

    use a2a_protocol_server::push::{HttpPushSender, PushRetryPolicy, PushSender};
    use a2a_protocol_types::events::StreamResponse;
    use a2a_protocol_types::push::TaskPushNotificationConfig;
    use a2a_protocol_types::task::{TaskId, TaskState, TaskStatus};

    // rustls needs a process-wide crypto provider before any config is built.
    // Installing it is idempotent-by-ignoring: a second call returns Err, which
    // only means someone got there first.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let issued = match rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]) {
        Ok(issued) => issued,
        Err(e) => return Check::fail(LABEL, format!("minting the certificate: {e}")),
    };
    let cert_der = issued.cert.der().clone();
    let key_der = rustls_pki_types::PrivateKeyDer::Pkcs8(issued.signing_key.serialize_der().into());

    let server_config = match rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der.clone()], key_der)
    {
        Ok(config) => config,
        Err(e) => return Check::fail(LABEL, format!("building the server TLS config: {e}")),
    };
    let webhook = serve_https(server_config).await;

    // The sender is handed a trust store containing exactly this certificate —
    // the "internal CA" case the API documents.
    let mut roots = rustls::RootCertStore::empty();
    if let Err(e) = roots.add(cert_der) {
        return Check::fail(LABEL, format!("trusting the certificate: {e}"));
    }
    let client_config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    // One attempt each: a retry loop would only make the failing leg slow, and
    // the property under test is the verdict, not the retry behaviour.
    let once = PushRetryPolicy::default().with_max_attempts(1);
    let event = StreamResponse::StatusUpdate(a2a_protocol_types::events::TaskStatusUpdateEvent {
        task_id: TaskId::new("harden-push"),
        context_id: a2a_protocol_types::task::ContextId::new("harden-ctx"),
        status: TaskStatus::new(TaskState::Completed),
        metadata: None,
    });
    let config = TaskPushNotificationConfig::new("harden-push", webhook.clone());

    let trusting = HttpPushSender::with_tls_config(client_config)
        .with_retry_policy(once.clone())
        .allow_private_urls();
    let deadline = Duration::from_secs(15);
    match tokio::time::timeout(deadline, trusting.send(&webhook, &event, &config)).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            return Check::fail(
                LABEL,
                format!("delivery to a certificate the sender was told to trust failed: {e}"),
            )
        }
        Err(_) => return Check::fail(LABEL, format!("trusted delivery hung past {deadline:?}")),
    }

    // Same endpoint, default Mozilla roots. Must be rejected.
    let default_roots = HttpPushSender::new()
        .with_retry_policy(once)
        .allow_private_urls();
    match tokio::time::timeout(deadline, default_roots.send(&webhook, &event, &config)).await {
        Ok(Err(_)) => Check::pass(
            LABEL,
            "delivered over TLS to a trusted private cert; the same cert was rejected \
             by the default root store",
        ),
        Ok(Ok(())) => Check::fail(
            LABEL,
            "a self-signed certificate was accepted by the DEFAULT root store — \
             the sender is not verifying certificates",
        ),
        Err(_) => Check::fail(LABEL, format!("untrusted delivery hung past {deadline:?}")),
    }
}

/// Serves HTTPS on a loopback port, answering everything with `200 ok`.
///
/// Returns an `https://localhost:<port>/webhook` URL: `localhost`, not
/// `127.0.0.1`, because that is the name in the certificate's SAN and the whole
/// point is that the name is checked.
#[cfg(feature = "tls-rustls")]
async fn serve_https(config: rustls::ServerConfig) -> String {
    use std::sync::Arc;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("binding a loopback port");
    let port = listener.local_addr().expect("local_addr").port();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(stream).await else {
                    return;
                };
                let io = hyper_util::rt::TokioIo::new(tls);
                let service = hyper::service::service_fn(|_req| async {
                    Ok::<_, std::convert::Infallible>(hyper::Response::new(
                        http_body_util::Full::new(bytes::Bytes::from_static(b"ok")),
                    ))
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await;
            });
        }
    });
    format!("https://localhost:{port}/webhook")
}

#[cfg(not(feature = "tls-rustls"))]
pub(super) async fn https_push() -> Check {
    Check::skipped(LABEL, "tls-rustls")
}
