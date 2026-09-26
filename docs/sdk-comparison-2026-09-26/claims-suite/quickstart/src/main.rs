// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use std::sync::Arc;

use a2a_protocol_sdk::prelude::*;

struct MyAgent;

// `agent_executor!` writes the `AgentExecutor` impl: no `Pin<Box<dyn Future>>`
// by hand.
agent_executor!(MyAgent, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;
    let who = ctx.message.text().unwrap_or("world");
    emit.artifact("greeting", vec![Part::text(format!("Hello, {who}!"))], None, Some(true))
        .await?;
    emit.status(TaskState::Completed).await?;
    Ok(())
});

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The server, on a port the OS picks.
    let handler = Arc::new(RequestHandlerBuilder::new(MyAgent).build()?);
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler)).await?;

    // A client for it.
    let client = ClientBuilder::new(format!("http://{addr}")).build()?;
    let reply = client
        .send_message(MessageSendParams::new(Message::user_text("m1", "Tom")))
        .await?;
    if let SendMessageResponse::Task(task) = reply {
        println!("{}", task.text().unwrap_or("(no text)")); // Hello, Tom!
    }

    // The same call, streamed: each event as the agent emits it.
    let mut stream = client
        .stream_message(MessageSendParams::new(Message::user_text("m2", "Ana")))
        .await?;
    while let Some(event) = stream.next().await {
        match event? {
            StreamResponse::StatusUpdate(ev) => println!("status: {:?}", ev.status.state),
            StreamResponse::ArtifactUpdate(ev) => println!("artifact: {}", ev.artifact.id),
            // `StreamResponse` is `#[non_exhaustive]`: keep a catch-all.
            _ => {}
        }
    }
    Ok(())
}
