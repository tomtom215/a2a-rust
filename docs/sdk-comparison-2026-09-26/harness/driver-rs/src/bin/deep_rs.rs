// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tier-2 interop checks D01-D15, a2a-rs client. Mirrors
//! driver-rust/src/bin/deep_rust.rs check for check.
#[path = "../../../common/report.rs"] mod report;
#[path = "../../../common/webhook.rs"] mod webhook;

use a2a::event::StreamResponse;
use a2a::*;
use a2a_client::A2AClientFactory;
use a2a_client::agent_card::AgentCardResolver;
use a2a_grpc::GrpcTransportFactory;
use futures::StreamExt;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn req(m: Message) -> SendMessageRequest { SendMessageRequest { message: m, configuration: None, metadata: None, tenant: None } }
fn user(text: &str) -> Message { Message::new(Role::User, vec![Part::text(text)]) }
fn msg(text: &str) -> SendMessageRequest { req(user(text)) }
fn texts(parts: &[Part]) -> String { parts.iter().filter_map(|p| match &p.content { PartContent::Text(t) => Some(t.as_str()), _ => None }).collect() }
fn task_text(t: &Task) -> String { t.artifacts.iter().flatten().map(|a| texts(&a.parts)).collect() }
fn status_text(t: &Task) -> String { t.status.message.as_ref().map(|m| texts(&m.parts)).unwrap_or_default() }
fn state_of(ev: &StreamResponse) -> Option<TaskState> {
    match ev { StreamResponse::Task(t) => Some(t.status.state.clone()), StreamResponse::StatusUpdate(u) => Some(u.status.state.clone()), _ => None }
}
fn get(id: &TaskId, h: Option<i32>) -> GetTaskRequest { GetTaskRequest { id: id.clone(), history_length: h, tenant: None } }

#[tokio::main]
async fn main() {
    let (base, binding, _) = report::args();
    let mut r = report::Report { driver: "a2a-rs-client", binding: binding.clone(), failures: 0 };
    let card = AgentCardResolver::new(None).resolve(&base).await.expect("card");
    let f = A2AClientFactory::builder().register(Arc::new(GrpcTransportFactory::new())).preferred_bindings(vec![binding.clone()]).build();
    let c = f.create_from_card(&card).await.expect("client");

    // D01
    let t = Instant::now();
    let mut mt_id = None;
    match c.send_message(&msg("ask:first")).await {
        Ok(SendMessageResponse::Task(t1)) if t1.status.state == TaskState::InputRequired => {
            let mut follow = user("more"); follow.task_id = Some(t1.id.clone()); follow.context_id = Some(t1.context_id.clone());
            match c.send_message(&req(follow)).await {
                Ok(SendMessageResponse::Task(t2)) => {
                    let ok = t2.id == t1.id && t2.status.state == TaskState::Completed && task_text(&t2) == "Echo: more" && status_text(&t1) == "need more input";
                    mt_id = Some(t1.id.clone());
                    r.rec("D01_multi_turn", ok, format!("same_id={} state={:?} text={:?} ask_msg={:?}", t2.id == t1.id, t2.status.state, task_text(&t2), status_text(&t1)), t);
                }
                other => r.rec("D01_multi_turn", false, format!("follow-up: {other:?}"), t),
            }
        }
        other => r.rec("D01_multi_turn", false, format!("first turn: {other:?}"), t),
    }

    // D02 / D12
    let t = Instant::now();
    if let Some(tid) = &mt_id {
        match c.get_task(&get(tid, Some(10))).await {
            Ok(task) => {
                let users: Vec<String> = task.history.iter().flatten().filter(|m| m.role == Role::User).map(|m| texts(&m.parts)).collect();
                r.rec("D02_history_both_turns", users == ["ask:first", "more"], format!("user_turns={users:?} total={}", task.history.as_ref().map_or(0, Vec::len)), t);
            }
            Err(e) => r.rec("D02_history_both_turns", false, format!("{e}"), t),
        }
        let t = Instant::now();
        match c.get_task(&get(tid, Some(1))).await {
            Ok(task) => { let n = task.history.as_ref().map_or(0, Vec::len); r.rec("D12_history_length_1", n <= 1, format!("len={n}"), t) }
            Err(e) => r.rec("D12_history_length_1", false, format!("{e}"), t),
        }
    } else { r.rec("D02_history_both_turns", false, "no D01 task", t); r.rec("D12_history_length_1", false, "no D01 task", t); }

    // D03
    let t = Instant::now();
    match c.send_message(&msg("fail:x")).await {
        Ok(SendMessageResponse::Task(tk)) => r.rec("D03_failed_with_reason", tk.status.state == TaskState::Failed && status_text(&tk) == "boom", format!("state={:?} msg={:?}", tk.status.state, status_text(&tk)), t),
        other => r.rec("D03_failed_with_reason", false, format!("{other:?}"), t),
    }

    // D04
    let t = Instant::now();
    match c.send_message(&msg("msg:hi")).await {
        Ok(SendMessageResponse::Message(m)) => { let s = texts(&m.parts); r.rec("D04_message_reply", s == "Reply: msg:hi", format!("text={s:?}"), t) }
        other => r.rec("D04_message_reply", false, format!("{other:?}"), t),
    }

    // D05
    let t = Instant::now();
    let data = serde_json::json!({"k": [1, "two", {"z": null}], "u": "é✓🚀"});
    let mut url = Part::url("https://example.com/a.png").with_media_type("image/png"); url.filename = Some("a.png".into());
    let parts = vec![
        Part::text("parts:x"),
        Part::data(data.clone()).with_media_type("application/json"),
        url,
        Part::raw(vec![0x00, 0x01, b'b', b'i', b'n', b'a', b'r', b'y', 0xff]).with_media_type("application/octet-stream"),
    ];
    let meta: std::collections::HashMap<String, serde_json::Value> = serde_json::from_value(serde_json::json!({"trace": "abc", "n": 7})).unwrap();
    let sent = serde_json::to_value(&parts).unwrap();
    let mut m = Message::new(Role::User, parts); m.metadata = Some(meta.clone());
    match c.send_message(&req(m)).await {
        Ok(SendMessageResponse::Task(tk)) => {
            let a = tk.artifacts.as_ref().and_then(|v| v.first());
            let got = a.map(|a| serde_json::to_value(&a.parts).unwrap());
            let gm = a.and_then(|a| a.metadata.clone());
            let ok = got.as_ref() == Some(&sent) && gm.as_ref() == Some(&meta);
            r.rec("D05_parts_roundtrip", ok, format!("parts_equal={} meta_equal={} got={}", got.as_ref() == Some(&sent), gm.as_ref() == Some(&meta), got.map(|g| g.to_string()).unwrap_or_default().chars().take(300).collect::<String>()), t);
        }
        other => r.rec("D05_parts_roundtrip", false, format!("{other:?}"), t),
    }

    // D06
    let t = Instant::now();
    let big = format!("é✓🚀中文\u{1F600}\n\t\"quoted\"\\{}", "x".repeat(256 * 1024));
    match c.send_message(&msg(&big)).await {
        Ok(SendMessageResponse::Task(tk)) => r.rec("D06_unicode_256k", task_text(&tk) == format!("Echo: {big}"), format!("len={}", task_text(&tk).len()), t),
        other => r.rec("D06_unicode_256k", false, format!("{:?}", other.err().map(|e| e.to_string())), t),
    }

    // D07
    let t = Instant::now();
    let ctxid = new_message_id();
    let mut made = vec![];
    for i in 0..5 {
        let mut m = user(&format!("p{i}")); m.context_id = Some(ctxid.clone());
        if let Ok(SendMessageResponse::Task(tk)) = c.send_message(&req(m)).await { made.push(tk.id.to_string()) }
    }
    let (mut seen, mut pages, mut token, mut err, mut max_page) = (vec![], 0, None::<String>, None, 0);
    loop {
        let l = ListTasksRequest { context_id: Some(ctxid.clone()), status: None, page_size: Some(2), page_token: token.clone(),
            history_length: None, status_timestamp_after: None, include_artifacts: None, tenant: None };
        match c.list_tasks(&l).await {
            Ok(l) => {
                pages += 1; max_page = max_page.max(l.tasks.len());
                seen.extend(l.tasks.iter().map(|x| x.id.to_string()));
                if l.next_page_token.is_empty() || pages > 10 { break } token = Some(l.next_page_token);
            }
            Err(e) => { err = Some(e.to_string()); break }
        }
    }
    let mut uniq = seen.clone(); uniq.sort(); uniq.dedup();
    let mut want = made.clone(); want.sort();
    r.rec("D07_pagination", err.is_none() && uniq == want && uniq.len() == seen.len() && max_page <= 2, format!("made={} seen={} unique={} pages={pages} max_page={max_page} err={err:?}", made.len(), seen.len(), uniq.len()), t);

    // D08
    let t = Instant::now();
    match c.send_streaming_message(&msg("slow:x")).await {
        Ok(mut s) => {
            let (mut tid, mut arts, mut last, mut canceled_sent) = (None::<TaskId>, 0, None, false);
            while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(5), s.next()).await {
                let Ok(ev) = ev else { break };
                match &ev { StreamResponse::Task(tk) => tid = Some(tk.id.clone()), StreamResponse::StatusUpdate(u) => tid = Some(u.task_id.clone()), StreamResponse::ArtifactUpdate(_) => arts += 1, _ => {} }
                if let Some(st) = state_of(&ev) { last = Some(st) }
                if arts == 1 && !canceled_sent { if let Some(id) = &tid { canceled_sent = c.cancel_task(&CancelTaskRequest { id: id.clone(), metadata: None, tenant: None }).await.is_ok(); } }
            }
            r.rec("D08_cancel_midstream", canceled_sent && last == Some(TaskState::Canceled) && arts < 5, format!("cancel_ok={canceled_sent} artifacts={arts} final={last:?}"), t);
        }
        Err(e) => r.rec("D08_cancel_midstream", false, format!("{e}"), t),
    }

    // D09
    let t = Instant::now();
    let reqs: Vec<SendMessageRequest> = (0..32).map(|i| msg(&format!("c{i}"))).collect();
    let res = futures::future::join_all(reqs.iter().map(|q| c.send_message(q))).await;
    let mut ids: Vec<String> = res.iter().filter_map(|x| match x { Ok(SendMessageResponse::Task(t)) if t.status.state == TaskState::Completed => Some(t.id.to_string()), _ => None }).collect();
    ids.sort(); ids.dedup();
    r.rec("D09_concurrent_32", ids.len() == 32, format!("completed_unique={}", ids.len()), t);

    // D10
    let t = Instant::now();
    match c.send_message(&req(Message::new(Role::User, vec![]))).await {
        Ok(o) => r.rec("D10_invalid_params_empty_parts", false, format!("accepted: {o:?}").chars().take(200).collect::<String>(), t),
        Err(e) => r.rec("D10_invalid_params_empty_parts", e.code == -32602, format!("code={} {e}", e.code), t),
    }

    // D11
    let t = Instant::now();
    let (hook, rec) = webhook::start().await;
    let mut p = msg("wait:push");
    p.configuration = Some(SendMessageConfiguration { accepted_output_modes: None, task_push_notification_config: None, history_length: None, return_immediately: Some(true) });
    let wid = match c.send_message(&p).await { Ok(SendMessageResponse::Task(tk)) => Some(tk.id), _ => None };
    if let Some(wid) = wid {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let cfg = TaskPushNotificationConfig { url: hook, id: None, task_id: wid.clone(), token: Some("tok-9".into()),
            authentication: Some(AuthenticationInfo { scheme: "Bearer".into(), credentials: Some("s3cret".into()) }), tenant: None };
        let set = c.create_push_config(&cfg).await;
        let _ = c.cancel_task(&CancelTaskRequest { id: wid.clone(), metadata: None, tenant: None }).await;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && rec.heads().is_empty() { tokio::time::sleep(Duration::from_millis(100)).await }
        tokio::time::sleep(Duration::from_millis(300)).await;
        let heads = rec.heads();
        let auth = heads.iter().any(|h| h.contains("authorization: bearer s3cret"));
        let tok = heads.iter().any(|h| h.contains("tok-9"));
        r.rec("D11_push_auth_headers", set.is_ok() && auth, format!("set_ok={} posts={} authorization_bearer={auth} token_header={tok}", set.is_ok(), heads.len()), t);
    } else { r.rec("D11_push_auth_headers", false, "no task", t) }

    // D13
    let t = Instant::now();
    match c.send_streaming_message(&msg("msg:s")).await {
        Ok(mut s) => {
            let mut kinds = vec![];
            while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(5), s.next()).await {
                match ev { Ok(StreamResponse::Message(_)) => kinds.push("message"), Ok(_) => kinds.push("other"), Err(_) => { kinds.push("err"); break } }
            }
            r.rec("D13_stream_message_reply", kinds == ["message"], format!("events={kinds:?}"), t);
        }
        Err(e) => r.rec("D13_stream_message_reply", false, format!("{e}"), t),
    }

    // D14
    let t = Instant::now();
    if let Ok(SendMessageResponse::Task(tk)) = c.send_message(&msg("done")).await {
        let res = match c.subscribe_to_task(&SubscribeToTaskRequest { id: tk.id.clone(), tenant: None }).await {
            Ok(mut s) => match tokio::time::timeout(Duration::from_secs(3), s.next()).await { Ok(Some(Err(e))) => Err(e), Ok(Some(Ok(ev))) => Ok(format!("{:?}", state_of(&ev))), other => Ok(format!("{:?}", other.map(|o| o.is_some()))) },
            Err(e) => Err(e),
        };
        match res { Err(e) => r.rec("D14_subscribe_terminal_error", e.code == -32004, format!("code={} {e}", e.code), t), Ok(o) => r.rec("D14_subscribe_terminal_error", false, format!("stream opened: {o}"), t) }
    } else { r.rec("D14_subscribe_terminal_error", false, "setup failed", t) }

    // D15
    let t = Instant::now();
    match c.cancel_task(&CancelTaskRequest { id: "no-such-task".into(), metadata: None, tenant: None }).await {
        Ok(_) => r.rec("D15_cancel_unknown", false, "accepted", t),
        Err(e) => r.rec("D15_cancel_unknown", e.code == -32001, format!("code={} {e}", e.code), t),
    }
    std::process::exit(if r.failures == 0 { 0 } else { 3 });
}
