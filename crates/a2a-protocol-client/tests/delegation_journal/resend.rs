// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! COR-06: a start whose response was lost is recovered from its journal
//! record by sending the message again under the record's key, and the agent
//! runs one task.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use a2a_protocol_client::delegation::{Delegation, DelegationOptions, Outcome};
use a2a_protocol_types::delegation_journal::{DelegationJournal, InMemoryDelegationJournal};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::harness::{GUARD, RUNS, client_for, job, serve};

/// A relay to `upstream` that loses the response to the first connection:
/// it passes the request on, waits for the server's first bytes (so the
/// server has the request), and closes both ends. Later connections pass.
async fn losing_the_first_response(upstream: SocketAddr) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let lost = Arc::new(AtomicBool::new(false));
    tokio::spawn(async move {
        while let Ok((client, _)) = listener.accept().await {
            let server = TcpStream::connect(upstream).await.expect("upstream");
            let first = !lost.swap(true, Ordering::SeqCst);
            tokio::spawn(relay(client, server, first));
        }
    });
    addr
}

async fn relay(client: TcpStream, server: TcpStream, lose: bool) {
    let (mut cr, mut cw) = client.into_split();
    let (mut sr, mut sw) = server.into_split();
    let up = tokio::spawn(async move { tokio::io::copy(&mut cr, &mut sw).await });
    if lose {
        let mut byte = [0u8; 1];
        let _ = sr.read(&mut byte).await;
        up.abort();
        return;
    }
    let mut buf = vec![0u8; 16 * 1024];
    while let Ok(n) = sr.read(&mut buf).await {
        if n == 0 || cw.write_all(&buf[..n]).await.is_err() {
            break;
        }
    }
    up.abort();
}

/// Before COR-06 a client not known to honour keys sent no key, so its
/// record had none and a resend started a second task.
#[tokio::test]
async fn a_lost_start_is_resent_under_its_records_key_and_runs_once() {
    let server = serve().await;
    let client = client_for(losing_the_first_response(server).await);
    let journal: Arc<dyn DelegationJournal> = Arc::new(InMemoryDelegationJournal::new());
    let text = format!("count:{}", uuid::Uuid::new_v4());
    // The same message both times: a key is held by one message id.
    let message = job(&text);
    let first = Delegation::start_with(
        Arc::clone(&client),
        message.clone(),
        DelegationOptions::new().with_journal(Arc::clone(&journal), "sup", "worker"),
    )
    .await;
    assert!(first.is_err(), "the first response was lost");

    let open = journal.open("sup").await.expect("open");
    assert_eq!(open.len(), 1);
    let record = open[0].clone();
    assert!(record.task_id.is_none(), "the child was never named");
    assert!(record.idempotency_key.is_some(), "the record has the key");

    let resent = Delegation::start_with(
        Arc::clone(&client),
        message,
        DelegationOptions::new().resending(Arc::clone(&journal), record.clone()),
    )
    .await
    .expect("the resend reaches the child");
    let done = tokio::time::timeout(GUARD, resent.wait(std::future::pending()))
        .await
        .expect("settles");
    assert!(
        matches!(done.outcome, Outcome::Completed(_)),
        "{:?}",
        done.outcome
    );
    assert_eq!(
        RUNS.lock().unwrap().get(&text),
        Some(&1),
        "the agent ran one task"
    );
    assert!(
        journal.open("sup").await.expect("open").is_empty(),
        "the record is settled"
    );
}
