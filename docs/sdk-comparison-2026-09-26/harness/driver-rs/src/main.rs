// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Client driver built on a2a-rs (a2a-client-lf 0.2.5 / a2a-grpc 0.3.7). Runs
//! the SAME named checks, in the same order, as driver-rust.
#[path = "../../common/report.rs"] mod report;
#[path = "../../common/webhook.rs"] mod webhook;

use a2a::event::StreamResponse;
use a2a::error_code;
use a2a::*;
use a2a_client::A2AClientFactory;
use a2a_client::agent_card::AgentCardResolver;
use a2a_grpc::GrpcTransportFactory;
use futures::StreamExt;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn msg(text: &str) -> SendMessageRequest {
    SendMessageRequest { message: Message::new(Role::User, vec![Part::text(text)]), configuration: None, metadata: None, tenant: None }
}

fn part_texts(parts: &[Part]) -> String {
    parts.iter().filter_map(|p| match &p.content { PartContent::Text(t) => Some(t.as_str()), _ => None }).collect()
}

fn task_text(t: &Task) -> String {
    t.artifacts.iter().flatten().map(|a| part_texts(&a.parts)).collect::<Vec<_>>().join("")
}

#[tokio::main]
async fn main() {
    let (base, binding, llm) = report::args();
    let mut r = report::Report { driver: "a2a-rs-client", binding: binding.clone(), failures: 0 };
    let input = "What is the capital of France?";
    let tmo = Duration::from_secs(if llm { 180 } else { 10 });

    // C01
    let t = Instant::now();
    let card = match AgentCardResolver::new(None).resolve(&base).await {
        Ok(c) => c,
        Err(e) => { r.rec("C01_card", false, format!("{e}"), t); std::process::exit(1) }
    };
    let factory = A2AClientFactory::builder()
        .register(Arc::new(GrpcTransportFactory::new()))
        .preferred_bindings(vec![binding.clone()])
        .build();
    let (client, iface) = match factory.create_from_card_with_interface(&card).await {
        Ok(c) => c,
        Err(e) => { r.rec("C01_card", false, format!("build: {e}"), t); std::process::exit(1) }
    };
    r.rec("C01_card", iface.protocol_binding == binding, format!("chosen={}", iface.protocol_binding), t);

    // C02
    let t = Instant::now();
    let mut done_id = None;
    match tokio::time::timeout(tmo, client.send_message(&msg(input))).await {
        Ok(Ok(SendMessageResponse::Task(task))) => {
            let txt = task_text(&task);
            let ok = task.status.state == TaskState::Completed && report::text_ok(&txt, llm, input);
            done_id = Some(task.id.clone());
            r.rec("C02_send", ok, format!("state={:?} text={txt:?}", task.status.state), t);
        }
        Ok(Ok(other)) => r.rec("C02_send", false, format!("non-task response {other:?}"), t),
        Ok(Err(e)) => r.rec("C02_send", false, format!("{e}"), t),
        Err(_) => r.rec("C02_send", false, "timeout", t),
    }

    // C03
    let t = Instant::now();
    let mut stream_id = None;
    let mut stream_text = String::new();
    match client.send_streaming_message(&msg(input)).await {
        Ok(mut s) => {
            let (mut n_art, mut last_state, mut first_art_ms, mut err) = (0, None, None, None);
            let mut saw_last_chunk = false;
            loop {
                match tokio::time::timeout(tmo, s.next()).await {
                    Ok(Some(Ok(ev))) => match ev {
                        StreamResponse::Task(tk) => { stream_id = Some(tk.id.clone()); last_state = Some(tk.status.state); }
                        StreamResponse::StatusUpdate(u) => { stream_id = Some(u.task_id.clone()); last_state = Some(u.status.state); }
                        StreamResponse::ArtifactUpdate(a) => {
                            n_art += 1;
                            first_art_ms.get_or_insert(t.elapsed().as_secs_f64() * 1000.0);
                            let chunk = part_texts(&a.artifact.parts);
                            if a.append == Some(true) { stream_text.push_str(&chunk) } else { stream_text = chunk }
                            saw_last_chunk |= a.last_chunk == Some(true);
                        }
                        other => { err = Some(format!("unexpected {other:?}")); }
                    },
                    Ok(Some(Err(e))) => { err = Some(format!("{e}")); break }
                    Ok(None) => break,
                    Err(_) => { err = Some("timeout".into()); break }
                }
            }
            let ok = err.is_none() && last_state == Some(TaskState::Completed) && n_art >= 1
                && saw_last_chunk && report::text_ok(&stream_text, llm, input);
            r.rec("C03_stream", ok, format!("artifact_events={n_art} first_artifact_ms={first_art_ms:?} last_chunk={saw_last_chunk} final={last_state:?} err={err:?} text={stream_text:?}"), t);
        }
        Err(e) => r.rec("C03_stream", false, format!("{e}"), t),
    }

    // C04
    let t = Instant::now();
    if let Some(id) = &stream_id {
        match client.get_task(&GetTaskRequest { id: id.clone(), history_length: None, tenant: None }).await {
            Ok(task) => {
                let txt = task_text(&task);
                r.rec("C04_get_task", task.status.state == TaskState::Completed && txt == stream_text,
                    format!("state={:?} stored_text={txt:?}", task.status.state), t);
            }
            Err(e) => r.rec("C04_get_task", false, format!("{e}"), t),
        }
    } else { r.rec("C04_get_task", false, "no task id from C03", t) }

    // C05
    let t = Instant::now();
    let lreq = ListTasksRequest { context_id: None, status: None, page_size: None, page_token: None,
        history_length: None, status_timestamp_after: None, include_artifacts: None, tenant: None };
    match client.list_tasks(&lreq).await {
        Ok(l) => {
            let found = done_id.as_ref().is_some_and(|id| l.tasks.iter().any(|x| x.id == *id));
            r.rec("C05_list_tasks", found, format!("n={}", l.tasks.len()), t);
        }
        Err(e) => r.rec("C05_list_tasks", false, format!("{e}"), t),
    }

    // C06
    let t = Instant::now();
    let mut p = msg("wait:hold");
    p.configuration = Some(SendMessageConfiguration { accepted_output_modes: None,
        task_push_notification_config: None, history_length: None, return_immediately: Some(true) });
    let wait_id = match client.send_message(&p).await {
        Ok(SendMessageResponse::Task(task)) => {
            let ok = matches!(task.status.state, TaskState::Working | TaskState::Submitted);
            r.rec("C06_send_nonblocking", ok, format!("state={:?}", task.status.state), t);
            Some(task.id)
        }
        Ok(o) => { r.rec("C06_send_nonblocking", false, format!("{o:?}"), t); None }
        Err(e) => { r.rec("C06_send_nonblocking", false, format!("{e}"), t); None }
    };
    let Some(wait_id) = wait_id else { std::process::exit(2) };
    tokio::time::sleep(Duration::from_millis(200)).await;

    // C07-C09
    let (hook_url, hooks) = webhook::start().await;
    let t = Instant::now();
    let cfg = TaskPushNotificationConfig { url: hook_url.clone(), id: None, task_id: wait_id.clone(),
        token: Some("tok-123".into()), authentication: None, tenant: None };
    let push_id = match client.create_push_config(&cfg).await {
        Ok(c) => { r.rec("C07_push_create", c.url == hook_url && c.id.is_some(), format!("id={:?}", c.id), t); c.id }
        Err(e) => { r.rec("C07_push_create", false, format!("{e}"), t); None }
    };
    let t = Instant::now();
    match &push_id {
        Some(pid) => match client.get_push_config(&GetTaskPushNotificationConfigRequest { task_id: wait_id.clone(), id: pid.clone(), tenant: None }).await {
            Ok(c) => r.rec("C08_push_get", c.url == hook_url, format!("url={}", c.url), t),
            Err(e) => r.rec("C08_push_get", false, format!("{e}"), t),
        },
        None => r.rec("C08_push_get", false, "no id", t),
    }
    let t = Instant::now();
    let lp = ListTaskPushNotificationConfigsRequest { task_id: wait_id.clone(), page_size: None, page_token: None, tenant: None };
    match client.list_push_configs(&lp).await {
        Ok(l) => r.rec("C09_push_list", l.configs.len() == 1, format!("n={}", l.configs.len()), t),
        Err(e) => r.rec("C09_push_list", false, format!("{e}"), t),
    }

    // C10
    let t = Instant::now();
    let mut sub = match client.subscribe_to_task(&SubscribeToTaskRequest { id: wait_id.clone(), tenant: None }).await {
        Ok(mut s) => match tokio::time::timeout(Duration::from_secs(5), s.next()).await {
            Ok(Some(Ok(ev))) => { r.rec("C10_subscribe", true, format!("first={}", kind(&ev)), t); Some(s) }
            other => { r.rec("C10_subscribe", false, format!("{:?}", other.map(|o| o.map(|x| x.map(|e| kind(&e))))), t); None }
        },
        Err(e) => { r.rec("C10_subscribe", false, format!("{e}"), t); None }
    };

    // C11
    let t = Instant::now();
    match client.cancel_task(&CancelTaskRequest { id: wait_id.clone(), metadata: None, tenant: None }).await {
        Ok(task) => r.rec("C11_cancel", task.status.state == TaskState::Canceled, format!("state={:?}", task.status.state), t),
        Err(e) => r.rec("C11_cancel", false, format!("{e}"), t),
    }

    // C12
    let t = Instant::now();
    if let Some(s) = sub.as_mut() {
        let mut saw = false;
        let mut ended = false;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_secs(5), s.next()).await {
                Ok(Some(Ok(ev))) => { if state_of(&ev) == Some(TaskState::Canceled) { saw = true } }
                Ok(Some(Err(_))) | Ok(None) => { ended = true; break }
                Err(_) => break,
            }
        }
        r.rec("C12_subscribe_sees_cancel", saw && ended, format!("saw_canceled={saw} ended={ended}"), t);
    } else { r.rec("C12_subscribe_sees_cancel", false, "no subscription", t) }

    // C13
    let t = Instant::now();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut bodies = hooks.bodies();
    while Instant::now() < deadline && !bodies.iter().any(|b| b.contains("CANCELED")) {
        tokio::time::sleep(Duration::from_millis(100)).await;
        bodies = hooks.bodies();
    }
    let got = bodies.iter().any(|b| b.contains("CANCELED") && b.contains(wait_id.as_str()));
    r.rec("C13_push_delivered", got, format!("posts={} sample={:?}", bodies.len(), bodies.last().map(|b| b.chars().take(160).collect::<String>())), t);

    // C14
    let t = Instant::now();
    match &push_id {
        Some(pid) => {
            let del = client.delete_push_config(&DeleteTaskPushNotificationConfigRequest { task_id: wait_id.clone(), id: pid.clone(), tenant: None }).await;
            let after = client.list_push_configs(&lp).await;
            let ok = del.is_ok() && after.as_ref().map(|l| l.configs.is_empty()).unwrap_or(false);
            r.rec("C14_push_delete", ok, format!("del={:?} after={:?}", del.err().map(|e| e.to_string()), after.map(|l| l.configs.len()).map_err(|e| e.to_string())), t);
        }
        None => r.rec("C14_push_delete", false, "no id", t),
    }

    // C15
    let t = Instant::now();
    match client.get_extended_agent_card(&GetExtendedAgentCardRequest { tenant: None }).await {
        Ok(c) => r.rec("C15_extended_card", c.name == "bench-agent", format!("name={}", c.name), t),
        Err(e) => r.rec("C15_extended_card", false, format!("{e}"), t),
    }

    // C16
    let t = Instant::now();
    match client.get_task(&GetTaskRequest { id: "no-such-task".into(), history_length: None, tenant: None }).await {
        Ok(_) => r.rec("C16_err_not_found", false, "returned a task", t),
        Err(e) => r.rec("C16_err_not_found", e.code == error_code::TASK_NOT_FOUND, format!("code={} {e}", e.code), t),
    }

    // C17
    let t = Instant::now();
    match &done_id {
        Some(id) => match client.cancel_task(&CancelTaskRequest { id: id.clone(), metadata: None, tenant: None }).await {
            Ok(tk) => r.rec("C17_err_not_cancelable", false, format!("state={:?}", tk.status.state), t),
            Err(e) => r.rec("C17_err_not_cancelable", e.code == error_code::TASK_NOT_CANCELABLE, format!("code={} {e}", e.code), t),
        },
        None => r.rec("C17_err_not_cancelable", false, "no completed task", t),
    }

    std::process::exit(if r.failures == 0 { 0 } else { 3 });
}

fn state_of(ev: &StreamResponse) -> Option<TaskState> {
    match ev {
        StreamResponse::Task(t) => Some(t.status.state.clone()),
        StreamResponse::StatusUpdate(u) => Some(u.status.state.clone()),
        _ => None,
    }
}

fn kind(ev: &StreamResponse) -> &'static str {
    match ev {
        StreamResponse::Task(_) => "task",
        StreamResponse::Message(_) => "message",
        StreamResponse::StatusUpdate(_) => "status",
        StreamResponse::ArtifactUpdate(_) => "artifact",
    }
}
