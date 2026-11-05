// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The control plane's metric hooks are called where the control plane acts
//! (audit H11): a refusal with its cause, a halt and a resume with their
//! scope and the tasks stopped, and the tenant a call was resolved to.

use std::sync::{Arc, Mutex};

use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::{MessageSendParams, TaskQueryParams};

use crate::builder::RequestHandlerBuilder;
use crate::error::ServerError;
use crate::handler::{HaltScope, RequestHandler, SendMessageResult};
use crate::metrics::{Metrics, RpcCall};
use crate::rpc_span::{RpcSystem, ServerSpan};
use crate::tenant_config::{PerTenantConfig, TenantLimits};

/// What the hooks were told, one line each.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<String>>>);

impl Seen {
    fn push(&self, line: String) {
        self.0.lock().unwrap().push(line);
    }
    fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

impl Metrics for Seen {
    fn on_refusal(&self, cause: &str) {
        self.push(format!("refusal {cause}"));
    }
    fn on_halt(&self, scope: &str, action: &str, stopped: usize) {
        self.push(format!("halt {scope} {action} {stopped}"));
    }
    fn on_rpc_call(&self, call: &RpcCall<'_>) {
        self.push(format!("rpc {}", call.tenant.unwrap_or("-")));
    }
}

/// Never finishes, so a send holds its slot for as long as the test runs.
struct Forever;
crate::agent_executor!(Forever, |_ctx, _queue| async {
    std::future::pending::<()>().await;
    Ok(())
});

fn params(tenant: &str) -> MessageSendParams {
    MessageSendParams::new(Message::new(
        MessageId::new(uuid::Uuid::new_v4().to_string()),
        MessageRole::User,
        vec![Part::text("work")],
    ))
    .with_tenant(tenant.to_owned())
}

/// A streaming send, whose stream is returned once the turn is admitted.
async fn admitted(h: &RequestHandler, tenant: &str) -> Result<SendMessageResult, ServerError> {
    h.on_send_message(params(tenant), true, None).await
}

#[tokio::test]
async fn a_tenant_at_its_cap_is_refused_with_its_cause() {
    let seen = Seen::default();
    let h = RequestHandlerBuilder::new(Forever)
        .without_agent_card()
        .with_task_store(crate::store::TenantAwareInMemoryTaskStore::new())
        .with_tenant_config(
            PerTenantConfig::builder()
                .with_override(
                    "busy",
                    TenantLimits::builder().max_concurrent_tasks(1).build(),
                )
                .build(),
        )
        .with_metrics(seen.clone())
        .build()
        .unwrap();
    let _held = admitted(&h, "busy").await.expect("the first is admitted");
    assert_eq!(seen.lines(), Vec::<String>::new());
    let refused = admitted(&h, "busy").await;
    assert!(
        matches!(refused, Err(ServerError::Overloaded(_))),
        "{refused:?}"
    );
    assert_eq!(seen.lines(), ["refusal tenant_cap"]);
}

#[tokio::test]
async fn a_server_at_its_stream_cap_is_refused_with_its_cause() {
    let seen = Seen::default();
    let h = RequestHandlerBuilder::new(Forever)
        .without_agent_card()
        .with_max_concurrent_streams(1)
        .with_metrics(seen.clone())
        .build()
        .unwrap();
    let _held = admitted(&h, "").await.expect("the first is admitted");
    let refused = admitted(&h, "").await;
    assert!(
        matches!(refused, Err(ServerError::Overloaded(_))),
        "{refused:?}"
    );
    assert_eq!(seen.lines(), ["refusal stream_cap"]);
}

#[tokio::test]
async fn a_halt_and_a_resume_are_counted_with_their_scope_and_the_tasks_stopped() {
    let seen = Seen::default();
    let h = RequestHandlerBuilder::new(Forever)
        .without_agent_card()
        .with_metrics(seen.clone())
        .build()
        .unwrap();
    let _held = admitted(&h, "").await.expect("admitted");
    let report = h.halt(HaltScope::All, "oncall", "test").await;
    assert_eq!(report.stopped.len(), 1);
    assert!(h.resume(HaltScope::All, "oncall").await.unwrap());
    h.halt(HaltScope::Tenant("acme".into()), "oncall", "test")
        .await;
    assert!(
        !h.resume(HaltScope::All, "oncall").await.unwrap(),
        "nothing to lift"
    );
    assert_eq!(
        seen.lines(),
        ["halt all halt 1", "halt all resume 0", "halt tenant halt 0"]
    );
}

/// The call's metric carries the tenant the handler resolved, although the
/// call is recorded after the handler's tenant scope has closed.
#[tokio::test]
async fn the_call_is_recorded_with_the_tenant_it_was_resolved_to() {
    let seen = Seen::default();
    let h = RequestHandlerBuilder::new(Forever)
        .without_agent_card()
        .with_task_store(crate::store::TenantAwareInMemoryTaskStore::new())
        .with_metrics(seen.clone())
        .build()
        .unwrap();
    let query = TaskQueryParams::new("nope".to_owned()).with_tenant("acme".to_owned());
    let _ = ServerSpan::open(&h, RpcSystem::JsonRpc, "GetTask", None)
        .run(h.on_get_task(query, None))
        .await;
    let _ = ServerSpan::open(&h, RpcSystem::JsonRpc, "GetTask", None)
        .run(async { Err::<(), _>(ServerError::Internal("before any tenant".into())) })
        .await;
    assert_eq!(seen.lines(), ["rpc acme", "rpc -"]);
}
