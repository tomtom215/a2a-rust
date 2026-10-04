// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! a2a-rust client (a2a-protocol-sdk 0.14.1): usage client <base-url> <text>
//! Streams one message over JSON-RPC, prints {"ttfa_ms","total_ms","text","events","state"}.
use std::time::Instant;
use a2a_protocol_sdk::prelude::*;

#[tokio::main]
async fn main() {
    let a: Vec<String> = std::env::args().collect();
    let card = resolve_agent_card(&a[1]).await.expect("card");
    let client = ClientBuilder::from_card(&card).expect("builder").build().expect("client");
    let params = MessageSendParams::new(Message::user("cli-1", vec![Part::text(a[2].clone())]));
    let t0 = Instant::now();
    let mut s = client.stream_message(params).await.expect("stream");
    let (mut text, mut events, mut ttfa, mut state) = (String::new(), 0, None, String::new());
    while let Some(ev) = s.next().await {
        let ev = ev.expect("event");
        events += 1;
        match ev {
            StreamResponse::ArtifactUpdate(u) => for t in u.artifact.texts() { if !t.is_empty() && ttfa.is_none() { ttfa = Some(t0.elapsed()); } text.push_str(t); },
            StreamResponse::StatusUpdate(u) => state = format!("{:?}", u.status.state),
            StreamResponse::Task(t) => state = format!("{:?}", t.status.state),
            _ => {}
        }
    }
    println!("{}", serde_json::json!({"client":"a2a-rust","ttfa_ms": ttfa.map(|d| d.as_secs_f64()*1e3), "total_ms": t0.elapsed().as_secs_f64()*1e3, "text": text, "events": events, "state": state}));
}
