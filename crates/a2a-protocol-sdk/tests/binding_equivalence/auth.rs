// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `BIND-EQUIV-004` (spec §5.1): "all supported protocols MUST support the
//! same authentication schemes declared in the `AgentCard`."
//!
//! The card declares one bearer scheme and requires it. On every binding,
//! every operation is refused without the credential and with a wrong one,
//! and with the declared credential every operation succeeds. That is also
//! `AUTH-SERVER-002` (§7.4, "MUST authenticate every incoming request") on
//! gRPC and WebSocket, which `a2a-protocol-server/tests/auth_every_method.rs`
//! shows for the two HTTP bindings.

use std::collections::BTreeSet;

use a2a_protocol_client::error::ClientError;
use a2a_protocol_client::{A2aClient, EventStream};
use a2a_protocol_types::params::{ListPushConfigsParams, ListTasksParams, TaskQueryParams};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use serde_json::{Value, json};

use crate::functionality::{GUARD, HOOK, every_method, params, sweep};
use crate::harness::{Binding, TOKEN, serve};

fn refused<T: std::fmt::Debug>(result: Result<T, ClientError>) -> Result<(), String> {
    match result {
        Err(ClientError::UnexpectedStatus { status: 401, .. }) => Ok(()),
        other => Err(format!("not a 401: {other:?}")),
    }
}

/// A stream's refusal: the call's, or, where the binding opens the stream
/// first (WebSocket), its first item's.
async fn stream_refused(result: Result<EventStream, ClientError>) -> Result<(), String> {
    match result {
        Err(e) => refused::<()>(Err(e)),
        Ok(mut stream) => match tokio::time::timeout(GUARD, stream.next()).await {
            Ok(Some(Err(e))) => refused::<()>(Err(e)),
            other => Err(format!("the stream was not refused: {other:?}")),
        },
    }
}

/// Every operation, with placeholder arguments, and whether it was refused
/// with `401`.
async fn every_operation(client: &A2aClient) -> Vec<(&'static str, Result<(), String>)> {
    let configs = ListPushConfigsParams {
        task_id: "t1".into(),
        page_size: None,
        page_token: None,
        tenant: None,
    };
    vec![
        (
            "SendMessage",
            refused(client.send_message(params("x")).await),
        ),
        (
            "SendStreamingMessage",
            stream_refused(client.stream_message(params("x")).await).await,
        ),
        (
            "GetTask",
            refused(client.get_task(TaskQueryParams::new("t1")).await),
        ),
        (
            "ListTasks",
            refused(client.list_tasks(ListTasksParams::default()).await),
        ),
        ("CancelTask", refused(client.cancel_task("t1").await)),
        (
            "SubscribeToTask",
            stream_refused(client.subscribe_to_task("t1").await).await,
        ),
        (
            "CreateTaskPushNotificationConfig",
            refused(
                client
                    .set_push_config(TaskPushNotificationConfig::new("t1", HOOK))
                    .await,
            ),
        ),
        (
            "GetTaskPushNotificationConfig",
            refused(client.get_push_config("t1", "c1").await),
        ),
        (
            "ListTaskPushNotificationConfigs",
            refused(client.list_push_configs(configs).await),
        ),
        (
            "DeleteTaskPushNotificationConfig",
            refused(client.delete_push_config("t1", "c1").await),
        ),
        (
            "GetExtendedAgentCard",
            refused(client.get_extended_agent_card().await),
        ),
    ]
}

async fn public_card(base: String) -> Value {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Empty<Bytes>>();
    let uri = format!("{base}/.well-known/agent-card.json");
    let resp = client.get(uri.parse().expect("uri")).await.expect("card");
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    serde_json::from_slice(&bytes).expect("card JSON")
}

/// BIND-EQUIV-004: the scheme the card declares is the one every binding
/// enforces, on every operation.
#[tokio::test]
async fn bind_equiv_004_every_binding_enforces_the_cards_scheme_on_every_operation() {
    let agent = serve(true, true).await;

    // One card, whichever HTTP listener it is fetched from, declaring one
    // bearer scheme and requiring it.
    let card = public_card(agent.url(Binding::JsonRpc)).await;
    assert_eq!(card, public_card(agent.url(Binding::HttpJson)).await);
    assert_eq!(
        card["securitySchemes"],
        json!({"bearer": {"httpAuthSecurityScheme": {"scheme": "bearer"}}})
    );
    assert_eq!(
        card["securityRequirements"],
        json!([{"schemes": {"bearer": {"list": []}}}])
    );

    for binding in Binding::ALL {
        for token in [None, Some("wrong")] {
            let client = agent.client(binding, token).await;
            let answers = every_operation(&client).await;
            let named: BTreeSet<String> = answers.iter().map(|(m, _)| (*m).to_owned()).collect();
            assert_eq!(named, every_method(), "every operation is probed");
            for (method, answer) in answers {
                assert_eq!(answer, Ok(()), "{binding:?} {method} with {token:?}");
            }
        }
        let client = agent.client(binding, Some(TOKEN)).await;
        if let Err(e) = sweep(&client).await {
            panic!("{binding:?} with the declared credential: {e}");
        }
    }
}
