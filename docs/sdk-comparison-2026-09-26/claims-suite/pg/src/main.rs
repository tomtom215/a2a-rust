// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Claim: PostgreSQL task store persists tasks across a restart (postgres feature).
//! Requires a local postgres at 127.0.0.1:7990 (db "claims"). Ports 7980-7989 for the agent.
use std::sync::Arc;
use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::PostgresTaskStore;

struct Echo;
agent_executor!(Echo, |ctx, queue| async {
    let e = EventEmitter::new(ctx, queue);
    let who = ctx.message.text().unwrap_or("world");
    e.artifact("greeting", vec![Part::text(format!("Hello, {who}!"))], None, Some(true)).await?;
    e.status(TaskState::Completed).await?;
    Ok(())
});

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "postgres://postgres@127.0.0.1:7990/claims";
    let mode = std::env::args().nth(1).unwrap_or_default();
    let store = PostgresTaskStore::new(url).await?;
    let h = Arc::new(RequestHandlerBuilder::new(Echo).with_task_store(store).build()?);
    let addr = serve_with_addr("127.0.0.1:7981", JsonRpcDispatcher::new(h)).await?;
    let c = ClientBuilder::new(format!("http://{addr}")).build()?;
    if mode == "write" {
        let SendMessageResponse::Task(t) = c.send_message(MessageSendParams::new(Message::user_text("pg-1", "pg"))).await? else { panic!() };
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/task_id.txt"), t.id.to_string())?;
        println!("wrote task {} state={:?}", t.id, t.status.state);
    } else {
        let id = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/task_id.txt"))?;
        let t = c.get_task(TaskQueryParams::new(id.clone())).await?;
        println!("new process read task {id}: state={:?} text={:?}", t.status.state, t.text());
        assert_eq!(t.text(), Some("Hello, pg!"));
    }
    Ok(())
}
