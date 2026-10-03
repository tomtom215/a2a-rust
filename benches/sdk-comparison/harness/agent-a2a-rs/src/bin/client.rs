// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! a2a-rs client (a2a-client-lf 0.2.7): usage client <base-url> <text>
//! Streams one message over JSON-RPC, prints {"ttfa_ms","total_ms","text","events","state"}.
use std::sync::Arc;
use std::time::Instant;
use a2a::event::StreamResponse;
use a2a::*;
use a2a_client::A2AClientFactory;
use a2a_client::agent_card::AgentCardResolver;
use futures::StreamExt;

#[tokio::main]
async fn main() {
    let a: Vec<String> = std::env::args().collect();
    let card = AgentCardResolver::new(None).resolve(&a[1]).await.expect("card");
    let client = A2AClientFactory::builder()
        .preferred_bindings(vec![TRANSPORT_PROTOCOL_JSONRPC.to_string()])
        .build()
        .create_from_card(&card).await.expect("client");
    let _ = Arc::new(());
    let req = SendMessageRequest { message: Message::new(Role::User, vec![Part::text(a[2].clone())]), configuration: None, metadata: None, tenant: None };
    let t0 = Instant::now();
    let mut s = client.send_streaming_message(&req).await.expect("stream");
    let (mut text, mut events, mut ttfa, mut state) = (String::new(), 0, None, String::new());
    while let Some(ev) = s.next().await {
        let ev = ev.expect("event");
        events += 1;
        match ev {
            StreamResponse::ArtifactUpdate(u) => for p in &u.artifact.parts { if let Some(t) = p.as_text() { if !t.is_empty() && ttfa.is_none() { ttfa = Some(t0.elapsed()); } text.push_str(t); } },
            StreamResponse::StatusUpdate(u) => state = format!("{:?}", u.status.state),
            StreamResponse::Task(t) => state = format!("{:?}", t.status.state),
            StreamResponse::Message(_) => {}
        }
    }
    println!("{}", serde_json::json!({"client":"a2a-rs","ttfa_ms": ttfa.map(|d| d.as_secs_f64()*1e3), "total_ms": t0.elapsed().as_secs_f64()*1e3, "text": text, "events": events, "state": state}));
}
