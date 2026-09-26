// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tier-2 interop checks D01-D15, a2a-rust client. Mirrors driver-rs/src/bin/deep_rs.rs
//! check for check. Needs the agents' deep.rs behaviour contract.
#[path = "../../../common/report.rs"] mod report;
#[path = "../../../common/webhook.rs"] mod webhook;

use a2a_protocol_sdk::client::discovery::resolve_agent_card;
use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::types::params::*;
use a2a_protocol_sdk::types::push::{AuthenticationInfo, TaskPushNotificationConfig};
use a2a_protocol_sdk::types::responses::SendMessageResponse;
use a2a_protocol_sdk::types::PartContent;
use std::time::{Duration, Instant};

fn id() -> String { uuid::Uuid::new_v4().to_string() }
fn msg(text: &str) -> MessageSendParams { MessageSendParams::new(Message::user_text(id(), text)) }
fn code(e: &ClientError) -> String {
    match e { ClientError::Protocol(p) => format!("{}", p.code as i32), other => format!("non-protocol: {other}") }
}
fn task_text(t: &Task) -> String { t.artifacts.iter().flatten().flat_map(|a| a.texts()).collect() }
fn status_text(t: &Task) -> String { t.status.message.as_ref().map(|m| m.texts().collect()).unwrap_or_default() }
fn state_of(ev: &StreamResponse) -> Option<TaskState> {
    match ev { StreamResponse::Task(t) => Some(t.status.state), StreamResponse::StatusUpdate(u) => Some(u.status.state), _ => None }
}

#[tokio::main]
async fn main() {
    let (base, binding, _) = report::args();
    let mut r = report::Report { driver: "a2a-rust-client", binding: binding.clone(), failures: 0 };
    let card = resolve_agent_card(&base).await.expect("card");
    let b = ClientBuilder::from_card_preferring(&card, &[binding.clone()]).expect("builder");
    let c = if binding == "GRPC" { b.build_grpc().await } else { b.build() }.expect("client");

    // D01 multi-turn: INPUT_REQUIRED then continuation completes the same task
    let t = Instant::now();
    let mut mt_id = None;
    match c.send_message(msg("ask:first")).await {
        Ok(SendMessageResponse::Task(t1)) if t1.status.state == TaskState::InputRequired => {
            let follow = Message::user_text(id(), "more").with_task_id(t1.id.clone()).with_context_id(t1.context_id.clone());
            match c.send_message(MessageSendParams::new(follow)).await {
                Ok(SendMessageResponse::Task(t2)) => {
                    let ok = t2.id == t1.id && t2.status.state == TaskState::Completed && task_text(&t2) == "Echo: more" && status_text(&t1) == "need more input";
                    mt_id = Some(t1.id.to_string());
                    r.rec("D01_multi_turn", ok, format!("same_id={} state={:?} text={:?} ask_msg={:?}", t2.id == t1.id, t2.status.state, task_text(&t2), status_text(&t1)), t);
                }
                other => r.rec("D01_multi_turn", false, format!("follow-up: {other:?}"), t),
            }
        }
        other => r.rec("D01_multi_turn", false, format!("first turn: {other:?}"), t),
    }

    // D02 history carries both user turns, in order; D12 historyLength=1 truncates
    let t = Instant::now();
    if let Some(tid) = &mt_id {
        match c.get_task(TaskQueryParams { tenant: None, id: tid.clone(), history_length: Some(10) }).await {
            Ok(task) => {
                let users: Vec<String> = task.history.iter().flatten().filter(|m| m.role == MessageRole::User).map(|m| m.texts().collect()).collect();
                r.rec("D02_history_both_turns", users == ["ask:first", "more"], format!("user_turns={users:?} total={}", task.history.as_ref().map_or(0, Vec::len)), t);
            }
            Err(e) => r.rec("D02_history_both_turns", false, format!("{e}"), t),
        }
        let t = Instant::now();
        match c.get_task(TaskQueryParams { tenant: None, id: tid.clone(), history_length: Some(1) }).await {
            Ok(task) => { let n = task.history.as_ref().map_or(0, Vec::len); r.rec("D12_history_length_1", n <= 1, format!("len={n}"), t) }
            Err(e) => r.rec("D12_history_length_1", false, format!("{e}"), t),
        }
    } else { r.rec("D02_history_both_turns", false, "no D01 task", t); r.rec("D12_history_length_1", false, "no D01 task", t); }

    // D03 failed task carries its reason
    let t = Instant::now();
    match c.send_message(msg("fail:x")).await {
        Ok(SendMessageResponse::Task(tk)) => r.rec("D03_failed_with_reason", tk.status.state == TaskState::Failed && status_text(&tk) == "boom", format!("state={:?} msg={:?}", tk.status.state, status_text(&tk)), t),
        other => r.rec("D03_failed_with_reason", false, format!("{other:?}"), t),
    }

    // D04 direct Message reply
    let t = Instant::now();
    match c.send_message(msg("msg:hi")).await {
        Ok(SendMessageResponse::Message(m)) => { let s: String = m.texts().collect(); r.rec("D04_message_reply", s == "Reply: msg:hi", format!("text={s:?}"), t) }
        other => r.rec("D04_message_reply", false, format!("{other:?}"), t),
    }

    // D05 text + data + url + raw parts, with metadata, round-trip exactly
    let t = Instant::now();
    let data = serde_json::json!({"k": [1, "two", {"z": null}], "u": "é✓🚀"});
    let parts = vec![
        Part::text("parts:x"),
        Part { content: PartContent::Data(data.clone()), metadata: None, filename: None, media_type: Some("application/json".into()) },
        Part::url("https://example.com/a.png").with_filename("a.png").with_media_type("image/png"),
        Part::raw("AAFiaW5hcnn/").with_media_type("application/octet-stream"),
    ];
    let meta = serde_json::json!({"trace": "abc", "n": 7});
    let sent = serde_json::to_value(&parts).unwrap();
    match c.send_message(MessageSendParams::new(Message::user(id(), parts).with_metadata(meta.clone()))).await {
        Ok(SendMessageResponse::Task(tk)) => {
            let a = tk.artifacts.as_ref().and_then(|v| v.first());
            let got = a.map(|a| serde_json::to_value(&a.parts).unwrap());
            let gm = a.and_then(|a| a.metadata.clone());
            let ok = got.as_ref() == Some(&sent) && gm.as_ref() == Some(&meta);
            r.rec("D05_parts_roundtrip", ok, format!("parts_equal={} meta_equal={} got={}", got.as_ref() == Some(&sent), gm.as_ref() == Some(&meta), got.map(|g| g.to_string()).unwrap_or_default().chars().take(300).collect::<String>()), t);
        }
        other => r.rec("D05_parts_roundtrip", false, format!("{other:?}"), t),
    }

    // D06 unicode + 256 KiB payload echoes exactly
    let t = Instant::now();
    let big = format!("é✓🚀中文\u{1F600}\n\t\"quoted\"\\{}", "x".repeat(256 * 1024));
    match c.send_message(msg(&big)).await {
        Ok(SendMessageResponse::Task(tk)) => r.rec("D06_unicode_256k", task_text(&tk) == format!("Echo: {big}"), format!("len={}", task_text(&tk).len()), t),
        other => r.rec("D06_unicode_256k", false, format!("{:?}", other.err().map(|e| e.to_string())), t),
    }

    // D07 pagination over one context: 5 tasks, pageSize 2
    let t = Instant::now();
    let ctxid = id();
    let mut made = vec![];
    for i in 0..5 {
        if let Ok(SendMessageResponse::Task(tk)) = c.send_message(MessageSendParams::new(Message::user_text(id(), format!("p{i}")).with_context_id(ctxid.clone()))).await { made.push(tk.id.to_string()) }
    }
    let (mut seen, mut pages, mut token, mut err, mut max_page) = (vec![], 0, None::<String>, None, 0);
    loop {
        let mut p = ListTasksParams::default();
        p.context_id = Some(ctxid.clone()); p.page_size = Some(2); p.page_token = token.clone();
        match c.list_tasks(p).await {
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

    // D08 cancel mid-stream: stream observes CANCELED and ends early
    let t = Instant::now();
    match c.stream_message(msg("slow:x")).await {
        Ok(mut s) => {
            let (mut tid, mut arts, mut last, mut canceled_sent) = (None, 0, None, false);
            while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(5), s.next()).await {
                let Ok(ev) = ev else { break };
                match &ev { StreamResponse::Task(tk) => tid = Some(tk.id.to_string()), StreamResponse::StatusUpdate(u) => tid = Some(u.task_id.to_string()), StreamResponse::ArtifactUpdate(_) => arts += 1, _ => {} }
                if let Some(st) = state_of(&ev) { last = Some(st) }
                if arts == 1 && !canceled_sent { if let Some(id) = &tid { canceled_sent = c.cancel_task(id.clone()).await.is_ok(); } }
            }
            r.rec("D08_cancel_midstream", canceled_sent && last == Some(TaskState::Canceled) && arts < 5, format!("cancel_ok={canceled_sent} artifacts={arts} final={last:?}"), t);
        }
        Err(e) => r.rec("D08_cancel_midstream", false, format!("{e}"), t),
    }

    // D09 32 concurrent sends
    let t = Instant::now();
    let futs = (0..32).map(|i| { let c = &c; async move { c.send_message(msg(&format!("c{i}"))).await } });
    let res = futures::future::join_all(futs).await;
    let mut ids: Vec<String> = res.iter().filter_map(|x| match x { Ok(SendMessageResponse::Task(t)) if t.status.state == TaskState::Completed => Some(t.id.to_string()), _ => None }).collect();
    ids.sort(); ids.dedup();
    r.rec("D09_concurrent_32", ids.len() == 32, format!("completed_unique={}", ids.len()), t);

    // D10 empty parts -> InvalidParams (-32602)
    let t = Instant::now();
    match c.send_message(MessageSendParams::new(Message::user(id(), vec![]))).await {
        Ok(o) => r.rec("D10_invalid_params_empty_parts", false, format!("accepted: {o:?}").chars().take(200).collect::<String>(), t),
        Err(e) => r.rec("D10_invalid_params_empty_parts", code(&e) == "-32602", format!("code={} {e}", code(&e)), t),
    }

    // D11 push with Bearer authentication reaches the webhook with the credential
    let t = Instant::now();
    let (hook, rec) = webhook::start().await;
    let wid = match c.send_message({ let mut p = msg("wait:push"); let mut cfg = SendMessageConfiguration::default(); cfg.return_immediately = Some(true); p.configuration = Some(cfg); p }).await {
        Ok(SendMessageResponse::Task(tk)) => Some(tk.id.to_string()), _ => None };
    if let Some(wid) = wid {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let cfg = TaskPushNotificationConfig { tenant: None, id: None, task_id: Some(wid.clone()), url: hook, token: Some("tok-9".into()),
            authentication: Some(AuthenticationInfo { scheme: "Bearer".into(), credentials: Some("s3cret".into()) }) };
        let set = c.set_push_config(cfg).await;
        let _ = c.cancel_task(wid.clone()).await;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && rec.heads().is_empty() { tokio::time::sleep(Duration::from_millis(100)).await }
        tokio::time::sleep(Duration::from_millis(300)).await;
        let heads = rec.heads();
        let auth = heads.iter().any(|h| h.contains("authorization: bearer s3cret"));
        let tok = heads.iter().any(|h| h.contains("tok-9"));
        r.rec("D11_push_auth_headers", set.is_ok() && auth, format!("set_ok={} posts={} authorization_bearer={auth} token_header={tok}", set.is_ok(), heads.len()), t);
    } else { r.rec("D11_push_auth_headers", false, "no task", t) }

    // D13 streaming a Message reply: exactly one Message, then close
    let t = Instant::now();
    match c.stream_message(msg("msg:s")).await {
        Ok(mut s) => {
            let mut kinds = vec![];
            while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(5), s.next()).await {
                match ev { Ok(StreamResponse::Message(_)) => kinds.push("message"), Ok(_) => kinds.push("other"), Err(_) => { kinds.push("err"); break } }
            }
            r.rec("D13_stream_message_reply", kinds == ["message"], format!("events={kinds:?}"), t);
        }
        Err(e) => r.rec("D13_stream_message_reply", false, format!("{e}"), t),
    }

    // D14 subscribe to a terminal task -> UnsupportedOperation (-32004)
    let t = Instant::now();
    if let Ok(SendMessageResponse::Task(tk)) = c.send_message(msg("done")).await {
        let res = match c.subscribe_to_task(tk.id.to_string()).await {
            Ok(mut s) => match tokio::time::timeout(Duration::from_secs(3), s.next()).await { Ok(Some(Err(e))) => Err(e), Ok(Some(Ok(ev))) => Ok(format!("{:?}", state_of(&ev))), other => Ok(format!("{:?}", other.map(|o| o.is_some()))) },
            Err(e) => Err(e),
        };
        match res { Err(e) => r.rec("D14_subscribe_terminal_error", code(&e) == "-32004", format!("code={} {e}", code(&e)), t), Ok(o) => r.rec("D14_subscribe_terminal_error", false, format!("stream opened: {o}"), t) }
    } else { r.rec("D14_subscribe_terminal_error", false, "setup failed", t) }

    // D15 cancel unknown task -> TaskNotFound (-32001)
    let t = Instant::now();
    match c.cancel_task("no-such-task").await {
        Ok(_) => r.rec("D15_cancel_unknown", false, "accepted", t),
        Err(e) => r.rec("D15_cancel_unknown", code(&e) == "-32001", format!("code={} {e}", code(&e)), t),
    }
    std::process::exit(if r.failures == 0 { 0 } else { 3 });
}
