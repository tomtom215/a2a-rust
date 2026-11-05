// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A renewal that asks for the lease alone (ADR 0044, audit
//! NEW-h-designs-3).

use std::collections::HashMap;
use std::time::Duration;

use a2a_protocol_types::lease::{LEASE_EXTENSION_URI, LEASE_RENEWAL_ONLY_URI};
use a2a_protocol_types::message::Part;
use a2a_protocol_types::params::TaskQueryParams;
use a2a_protocol_types::task::TaskState;

use super::tests::{TTL, reaches, start, tasks};
use crate::builder::RequestHandlerBuilder;
use crate::store::lease::now_unix_ms;
use crate::store::{InMemoryLeaseStore, LeaseStore};
use crate::{EventEmitter, agent_executor};

/// Writes an artifact, then works until cancelled.
struct Produces;
agent_executor!(Produces, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.artifact("a", vec![Part::text("big")], None, Some(true))
        .await?;
    emit.status(TaskState::Working).await?;
    ctx.cancellation_token.cancelled().await;
    Ok(())
});

/// A renewal that activates the renewal-only extension beside the lease one
/// renews and is answered with the status alone; one that does not, and a
/// plain read, get the whole task (ADR 0044, audit NEW-h-designs-3).
#[tokio::test]
async fn a_renewal_only_read_renews_and_answers_with_the_status_alone() {
    let leases = InMemoryLeaseStore::new();
    let h = RequestHandlerBuilder::new(Produces)
        .without_agent_card()
        .with_task_store_arc(tasks())
        .with_lease_store(leases.clone())
        .build()
        .expect("handler");
    let id = start(&h, Some(TTL)).await.unwrap();
    reaches(&h, &id, TaskState::Working, Duration::from_secs(5)).await;
    let get = |uris: &str| {
        let headers = HashMap::from([("a2a-extensions".to_owned(), uris.to_owned())]);
        let h = &h;
        let id = id.clone();
        async move {
            h.on_get_task(TaskQueryParams::new(id), Some(&headers))
                .await
                .expect("get")
        }
    };
    // A deadline at the epoch, so the renewal's is now plus the time to live,
    // bracketed by the clock read before and after the call.
    let ttl_ms = i64::try_from(TTL.as_millis()).unwrap();
    leases.grant("", &id, ttl_ms, 0).await.unwrap();
    let from = now_unix_ms();
    let only = get(&format!("{LEASE_EXTENSION_URI}, {LEASE_RENEWAL_ONLY_URI}")).await;
    let to = now_unix_ms();
    let renewed = leases.deadline("", &id).await.unwrap().expect("a lease");
    assert!(
        from + ttl_ms <= renewed && renewed <= to + ttl_ms,
        "renewed to {renewed}, not now ({from}..={to}) plus {ttl_ms}"
    );
    assert_eq!(only.status.state, TaskState::Working);
    assert_eq!(
        (only.artifacts, only.history, only.metadata),
        (None, None, None)
    );
    for uris in [LEASE_EXTENSION_URI, LEASE_RENEWAL_ONLY_URI, ""] {
        let whole = get(uris).await;
        assert_eq!(whole.artifacts.map(|a| a.len()), Some(1), "{uris:?}");
        assert!(whole.history.is_some_and(|h| !h.is_empty()), "{uris:?}");
    }
}
