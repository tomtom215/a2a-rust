// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Steering across tenants (ADR 0036): a tenant steers its own tasks only.

use std::sync::Arc;

use a2a_protocol_types::params::MessageSendParams;

use super::steer_tests::{
    Agent, GUARD, Mode, Probe, Recorder, Rig, background, inbox_len, message, refused_as_in_flight,
    send, steering_to,
};
use crate::builder::RequestHandlerBuilder;
use crate::error::ServerError;
use crate::steering::SteeringConfig;
use crate::store::TenantAwareInMemoryTaskStore;

// ── Tenants ─────────────────────────────────────────────────────────────────

fn tenant_rig(steering: bool) -> Rig {
    let probe = Arc::new(Probe::default());
    let metrics = Arc::new(Recorder::default());
    let mut builder = RequestHandlerBuilder::new(Agent {
        mode: Mode::Block,
        probe: Arc::clone(&probe),
    })
    .without_agent_card()
    .with_task_store(TenantAwareInMemoryTaskStore::new())
    .with_metrics(Arc::clone(&metrics));
    if steering {
        builder = builder.with_steering(SteeringConfig::new());
    }
    Rig {
        handler: builder.build().expect("handler"),
        probe,
        metrics,
    }
}

fn in_tenant(tenant: &str, mut params: MessageSendParams) -> MessageSendParams {
    params.tenant = Some(tenant.to_owned());
    params
}

/// A tenant cannot steer another tenant's task: it is not found, as it is for
/// every other operation, and the owner's inbox stays empty.
#[tokio::test]
async fn a_tenant_cannot_steer_another_tenants_task() {
    let rig = tenant_rig(true);
    let task = send(
        &rig.handler,
        in_tenant("acme", background(message(None, "work"))),
    )
    .await
    .expect("starts");
    tokio::time::timeout(GUARD, rig.probe.started.notified())
        .await
        .expect("running");

    let theirs = send(
        &rig.handler,
        in_tenant("globex", steering_to(&task.id.0, "hijack")),
    )
    .await;
    assert!(
        matches!(theirs, Err(ServerError::TaskNotFound(_))),
        "{theirs:?}"
    );
    assert_eq!(inbox_len(&rig, &task.id).await, 0);
    assert_eq!(rig.metrics.outcomes(), Vec::<String>::new());

    // The owning tenant steers it.
    send(
        &rig.handler,
        in_tenant("acme", steering_to(&task.id.0, "mine")),
    )
    .await
    .expect("delivered");
    assert_eq!(inbox_len(&rig, &task.id).await, 1);
    rig.probe.release.notify_one();
}

/// Even over a store that does not keep tenants apart (the operator opted in
/// to that), a turn is steered only by the tenant that started it.
#[tokio::test]
async fn a_shared_store_does_not_let_a_tenant_steer_another_tenants_turn() {
    let probe = Arc::new(Probe::default());
    let metrics = Arc::new(Recorder::default());
    let handler = RequestHandlerBuilder::new(Agent {
        mode: Mode::Block,
        probe: Arc::clone(&probe),
    })
    .without_agent_card()
    .accept_unisolated_tenants()
    .with_steering(SteeringConfig::new())
    .with_metrics(Arc::clone(&metrics))
    .build()
    .expect("handler");
    let rig = Rig {
        handler,
        probe,
        metrics,
    };
    let task = send(
        &rig.handler,
        in_tenant("acme", background(message(None, "work"))),
    )
    .await
    .expect("starts");
    tokio::time::timeout(GUARD, rig.probe.started.notified())
        .await
        .expect("running");

    let theirs = send(
        &rig.handler,
        in_tenant("globex", steering_to(&task.id.0, "hijack")),
    )
    .await;
    refused_as_in_flight(&theirs, &task.id);
    assert_eq!(inbox_len(&rig, &task.id).await, 0);
    assert_eq!(rig.metrics.outcomes(), Vec::<String>::new());
    send(
        &rig.handler,
        in_tenant("acme", steering_to(&task.id.0, "mine")),
    )
    .await
    .expect("delivered");
    rig.probe.release.notify_one();
}
