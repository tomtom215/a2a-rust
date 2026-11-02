// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The append lock of a chain exists while someone holds or awaits it, and not
//! after (audit D-19). The map grew by one entry per distinct chain name and
//! never shrank, and the name is the tenant, which a caller chooses.

use std::sync::Arc;
use std::time::Duration;

use super::AuditLog;
use super::record::{AuditRecord, kind};
use crate::audit::store::InMemoryAuditStore;

fn log() -> Arc<AuditLog> {
    Arc::new(AuditLog::new(Arc::new(InMemoryAuditStore::new())))
}

fn rec(chain: &str) -> AuditRecord {
    AuditRecord::new(chain, kind::CALL)
}

/// The scenario: a caller who can pick the tenant sends a request for each of
/// many names. Nothing should be left in the map afterwards.
#[tokio::test]
async fn appending_to_many_chains_leaves_no_lock_behind() {
    let log = log();
    for i in 0..200 {
        log.append(rec(&format!("tenant-{i}"))).await.unwrap();
    }
    assert_eq!(log.tracked_chains(), 0);
}

#[tokio::test]
async fn a_chain_in_use_is_tracked_and_only_until_its_last_user_lets_go() {
    let log = log();
    let first = log.lock_chain("busy").await;
    assert_eq!(log.tracked_chains(), 1);

    // A second caller waits for it.
    let waiter = {
        let log = Arc::clone(&log);
        tokio::spawn(async move {
            let _guard = log.lock_chain("busy").await;
            tokio::time::sleep(Duration::from_millis(20)).await;
        })
    };
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(
        !waiter.is_finished(),
        "it waits while the first holds the lock"
    );

    drop(first);
    assert_eq!(
        log.tracked_chains(),
        1,
        "the waiter still needs the entry, so it stays"
    );
    waiter.await.unwrap();
    assert_eq!(log.tracked_chains(), 0, "the last one out removes it");
}

/// A caller dropped while it waits must not leave an entry behind either.
#[tokio::test]
async fn a_caller_cancelled_while_it_waits_leaves_nothing_behind() {
    let log = log();
    let holder = log.lock_chain("busy").await;
    let timed_out = tokio::time::timeout(Duration::from_millis(50), log.lock_chain("busy"))
        .await
        .is_err();
    assert!(timed_out, "the lock is held, so the wait timed out");
    assert_eq!(log.tracked_chains(), 1, "only the holder's entry");

    drop(holder);
    assert_eq!(log.tracked_chains(), 0);
}

/// The lock still serialises: two callers on one chain never hold it at once.
#[tokio::test]
async fn the_lock_still_excludes() {
    let log = log();
    let guard = log.lock_chain("c").await;
    let waited = tokio::time::timeout(Duration::from_millis(50), log.lock_chain("c"))
        .await
        .is_err();
    assert!(waited, "the second caller waits");
    drop(guard);
    let got = tokio::time::timeout(Duration::from_secs(5), log.lock_chain("c"))
        .await
        .is_ok();
    assert!(got, "and gets it once the first lets go");
}

/// Another chain is not held up by the first.
#[tokio::test]
async fn chains_do_not_block_each_other() {
    let log = log();
    let _held = log.lock_chain("one").await;
    let got = tokio::time::timeout(Duration::from_secs(5), log.lock_chain("two"))
        .await
        .is_ok();
    assert!(got);
}
