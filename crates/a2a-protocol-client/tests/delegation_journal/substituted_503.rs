// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! N26-01: a bare `503` that arrives after the agent made the child. A relay
//! passes the start to the server, waits until the server streams the
//! child's first event (so the child exists and ran), and answers the client
//! with a bare `503` in its place. The start fails, but the grant stays
//! reserved and the record open, for a restarted supervisor to find; before
//! the fix the grant was refunded and the record settled `not_started`.

use std::net::SocketAddr;
use std::sync::Arc;

use a2a_protocol_client::delegation::{Delegation, DelegationOptions};
use a2a_protocol_client::error::ClientError;
use a2a_protocol_types::budget::{Budget, BudgetHandle};
use a2a_protocol_types::delegation_journal::{DelegationJournal, InMemoryDelegationJournal};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::harness::{RUNS, client_for, job, serve};

const BARE_503: &[u8] =
    b"HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";

/// A relay to `upstream` that answers every request with [`BARE_503`] once
/// the server has sent the first event of its stream.
async fn substituting_503(upstream: SocketAddr) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        while let Ok((client, _)) = listener.accept().await {
            let server = TcpStream::connect(upstream).await.expect("upstream");
            tokio::spawn(substitute(client, server));
        }
    });
    addr
}

async fn substitute(client: TcpStream, server: TcpStream) {
    let (mut cr, mut cw) = client.into_split();
    let (mut sr, mut sw) = server.into_split();
    let up = tokio::spawn(async move { tokio::io::copy(&mut cr, &mut sw).await });
    // The executor counts its run before it emits `Working`, so the child
    // ran once the first `data:` line has come back.
    let mut seen = Vec::new();
    let mut buf = [0u8; 4096];
    while !seen.windows(5).any(|w| w == b"data:") {
        match sr.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => seen.extend_from_slice(&buf[..n]),
        }
    }
    let _ = cw.write_all(BARE_503).await;
    let _ = cw.shutdown().await;
    up.abort();
}

#[tokio::test]
async fn a_bare_503_after_the_child_ran_keeps_the_grant_and_the_record() {
    let client = client_for(substituting_503(serve().await).await);
    let journal: Arc<dyn DelegationJournal> = Arc::new(InMemoryDelegationJournal::new());
    let parent = BudgetHandle::start(&Budget::new().with_meter("tokens", 100).unwrap());
    let grant = parent
        .grant(&Budget::new().with_meter("tokens", 40).unwrap())
        .unwrap();
    let text = format!("count:{}", uuid::Uuid::new_v4());
    let err = Delegation::start_with(
        client,
        job(&text),
        DelegationOptions::new().with_budget(grant).with_journal(
            Arc::clone(&journal),
            "sup",
            "worker",
        ),
    )
    .await
    .expect_err("the relay answered 503");
    assert!(
        matches!(err, ClientError::UnexpectedStatus { status: 503, .. }),
        "{err:?}"
    );
    assert_eq!(
        RUNS.lock().unwrap().get(&text),
        Some(&1),
        "the child ran, once: a bare 503 is not sent again (D-1)"
    );
    assert_eq!(
        parent.remaining("tokens"),
        Some(60),
        "the grant stays reserved"
    );
    let open = journal.open("sup").await.expect("open");
    assert_eq!(open.len(), 1, "the record stays open: {open:?}");
    assert!(open[0].outcome.is_none(), "{:?}", open[0].outcome);
}
