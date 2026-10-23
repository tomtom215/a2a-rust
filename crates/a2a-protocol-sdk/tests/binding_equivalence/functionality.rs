// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `BIND-EQUIV-001` (spec §5.1): "all supported protocols MUST provide the
//! same set of operations and capabilities."
//!
//! [`sweep`] performs all eleven operations through one client, each on its
//! success path, and the agent records what reached it. Every binding must
//! complete the sweep and deliver exactly the operations in `Method::ALL`.

use std::collections::BTreeSet;
use std::time::Duration;

use a2a_protocol_client::A2aClient;
use a2a_protocol_client::error::ClientError;
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::message::Message;
use a2a_protocol_types::method::Method;
use a2a_protocol_types::params::{
    ListPushConfigsParams, ListTasksParams, MessageSendParams, SendMessageConfiguration,
    TaskQueryParams,
};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{Task, TaskState};

use crate::harness::{Binding, HOLD, serve};

/// Outer guard on a stream read: only a hang reaches it.
pub const GUARD: Duration = Duration::from_secs(10);

/// Where push configs point. Never delivered to: configs are set on a task
/// that has already finished.
pub const HOOK: &str = "https://hook.example.com/a2a";

pub fn params(text: &str) -> MessageSendParams {
    MessageSendParams::new(Message::user_text(uuid_like(), text))
}

/// A message id unique within the process.
pub fn uuid_like() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!("m-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

pub fn state_of(event: &StreamResponse) -> Option<TaskState> {
    match event {
        StreamResponse::Task(t) => Some(t.status.state),
        StreamResponse::StatusUpdate(u) => Some(u.status.state),
        _ => None,
    }
}

fn fail(what: &str) -> impl FnOnce(ClientError) -> String + '_ {
    move |e| format!("{what}: {e:?}")
}

/// A blocking send of `text`, answered with its task.
pub async fn send(client: &A2aClient, text: &str) -> Result<Task, String> {
    match client
        .send_message(params(text))
        .await
        .map_err(fail("SendMessage"))?
    {
        SendMessageResponse::Task(t) => Ok(t),
        other => Err(format!("SendMessage answered {other:?}, not a task")),
    }
}

/// Starts a task that stays `WORKING` until canceled.
pub async fn hold(client: &A2aClient) -> Result<Task, String> {
    let held = params(HOLD).with_configuration(SendMessageConfiguration {
        return_immediately: Some(true),
        ..Default::default()
    });
    match client
        .send_message(held)
        .await
        .map_err(fail("SendMessage (held)"))?
    {
        SendMessageResponse::Task(t) => Ok(t),
        other => Err(format!("a held send answered {other:?}, not a task")),
    }
}

/// Reads a stream to its end, returning every event.
pub async fn drain(
    mut stream: a2a_protocol_client::EventStream,
) -> Result<Vec<StreamResponse>, String> {
    let mut events = Vec::new();
    while let Some(item) = tokio::time::timeout(GUARD, stream.next())
        .await
        .map_err(|_| "the stream stalled".to_owned())?
    {
        events.push(item.map_err(fail("stream event"))?);
    }
    Ok(events)
}

/// Every operation, each on its success path.
pub async fn sweep(client: &A2aClient) -> Result<(), String> {
    let done = send(client, "sweep").await?;
    if done.status.state != TaskState::Completed {
        return Err(format!("SendMessage ended {:?}", done.status.state));
    }
    let id = done.id.0.clone();

    let streamed = drain(
        client
            .stream_message(params("streamed"))
            .await
            .map_err(fail("SendStreamingMessage"))?,
    )
    .await?;
    if streamed.iter().rev().find_map(state_of) != Some(TaskState::Completed) {
        return Err(format!("the stream did not end COMPLETED: {streamed:?}"));
    }

    client
        .get_task(TaskQueryParams::new(&id))
        .await
        .map_err(fail("GetTask"))?;
    let listed = client
        .list_tasks(ListTasksParams::default())
        .await
        .map_err(fail("ListTasks"))?;
    if !listed.tasks.iter().any(|t| t.id.0 == id) {
        return Err("ListTasks did not list the task".into());
    }

    let config = client
        .set_push_config(TaskPushNotificationConfig::new(id.as_str(), HOOK))
        .await
        .map_err(fail("CreateTaskPushNotificationConfig"))?;
    let config_id = config.id.ok_or("the created config has no id")?;
    client
        .get_push_config(&id, &config_id)
        .await
        .map_err(fail("GetTaskPushNotificationConfig"))?;
    let configs = client
        .list_push_configs(ListPushConfigsParams {
            task_id: id.clone(),
            page_size: None,
            page_token: None,
            tenant: None,
        })
        .await
        .map_err(fail("ListTaskPushNotificationConfigs"))?;
    if configs.configs.len() != 1 {
        return Err(format!("listed {} configs, not 1", configs.configs.len()));
    }
    client
        .delete_push_config(&id, &config_id)
        .await
        .map_err(fail("DeleteTaskPushNotificationConfig"))?;

    let held = hold(client).await?;
    let mut subscription = client
        .subscribe_to_task(&held.id.0)
        .await
        .map_err(fail("SubscribeToTask"))?;
    match tokio::time::timeout(GUARD, subscription.next()).await {
        Ok(Some(Ok(StreamResponse::Task(t)))) if t.id == held.id => {}
        other => return Err(format!("SubscribeToTask's first event: {other:?}")),
    }
    let canceled = client
        .cancel_task(&held.id.0)
        .await
        .map_err(fail("CancelTask"))?;
    if canceled.status.state != TaskState::Canceled {
        return Err(format!("CancelTask left {:?}", canceled.status.state));
    }

    client
        .get_extended_agent_card()
        .await
        .map_err(fail("GetExtendedAgentCard"))?;
    Ok(())
}

pub fn every_method() -> BTreeSet<String> {
    Method::ALL
        .iter()
        .map(|m| m.wire_name().to_owned())
        .collect()
}

/// BIND-EQUIV-001: every binding serves every operation, and its success
/// path, against one agent.
#[tokio::test]
async fn bind_equiv_001_every_operation_succeeds_on_every_binding() {
    let agent = serve(false, true).await;
    for binding in Binding::ALL {
        let client = agent.client(binding, None).await;
        if let Err(e) = sweep(&client).await {
            panic!("{binding:?}: {e}");
        }
        let reached: BTreeSet<String> = agent.take_seen().into_iter().map(|(m, _)| m).collect();
        assert_eq!(
            reached,
            every_method(),
            "{binding:?} must deliver exactly the operations in Method::ALL"
        );
    }
}

/// Drops every `false`. `AgentExtension.required` is a proto3 `bool`
/// without `optional`, so `false` and absent are one value: gRPC's encoding
/// cannot tell them apart and decodes it as absent, where the JSON bindings
/// carry the `false` this SDK serialises. Semantically the same card.
fn implicit_false_dropped(mut value: serde_json::Value) -> serde_json::Value {
    fn strip(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(map) => {
                map.retain(|_, v| *v != serde_json::Value::Bool(false));
                map.values_mut().for_each(strip);
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    strip(&mut value);
    value
}

/// BIND-EQUIV-001's capability half: the card each binding's client
/// fetches through `GetExtendedAgentCard` is the same card, so a client
/// learns the same capabilities whichever binding it picked.
#[tokio::test]
async fn bind_equiv_001_every_binding_serves_the_same_capabilities() {
    let agent = serve(false, true).await;
    let mut cards = Vec::new();
    for binding in Binding::ALL {
        let client = agent.client(binding, None).await;
        let card = client
            .get_extended_agent_card()
            .await
            .unwrap_or_else(|e| panic!("{binding:?}: {e:?}"));
        cards.push((
            binding,
            implicit_false_dropped(serde_json::to_value(&card).expect("card")),
        ));
    }
    let (first, expected) = &cards[0];
    assert_eq!(expected["capabilities"]["streaming"], true);
    assert_eq!(expected["capabilities"]["pushNotifications"], true);
    for (binding, card) in &cards[1..] {
        assert_eq!(
            card, expected,
            "{binding:?} and {first:?} serve different cards"
        );
    }
}
