<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Unit and Integration Tests

How to test an executor you wrote: directly against an in-memory event
queue, then through a real server and client, on both HTTP transports and
over a stream.

## Unit testing executors

Test your executor logic directly by creating a `RequestContext` and a real in-memory event queue.
The examples on this page test a calculator executor, `CalcExecutor`, which is
not shown: given `"3 + 5"` it emits `Working`, an artifact holding `8`, then
`Completed`. Substitute your own executor and its expectations:

```rust,no_run
use a2a_protocol_sdk::prelude::*;
use a2a_protocol_server::streaming::event_queue::new_in_memory_queue;

#[tokio::test]
async fn test_calculator_executor() {
    let executor = CalcExecutor;

    // Create a writer/reader pair directly for unit testing
    let (writer, mut reader) = new_in_memory_queue();

    // Build the request context. `RequestContext` is `#[non_exhaustive]`
    // since 0.13, so build it with `new` and the `with_*` methods rather
    // than a struct literal.
    let ctx = RequestContext::new(
        Message::user_text("msg-1", "3 + 5"),
        TaskId::new("test-task"),
        "ctx-1".to_owned(),
    );

    // Run the executor
    executor.execute(&ctx, &writer).await.unwrap();

    // Read events from the queue
    let events: Vec<_> = collect_events(&mut reader).await;

    // Verify: Working → ArtifactUpdate → Completed
    assert!(matches!(&events[0],
        StreamResponse::StatusUpdate(e) if e.status.state == TaskState::Working));
    assert!(matches!(&events[1],
        StreamResponse::ArtifactUpdate(e) if extract_text(&e.artifact) == "8"));
    assert!(matches!(&events[2],
        StreamResponse::StatusUpdate(e) if e.status.state == TaskState::Completed));
}
# fn main() {}
# async fn collect_events(reader: &mut a2a_protocol_server::streaming::event_queue::InMemoryQueueReader) -> Vec<StreamResponse> {
#     use a2a_protocol_server::streaming::EventQueueReader;
#     let mut out = Vec::new();
#     while let Some(Ok(e)) = reader.read().await { out.push(e.event); }
#     out
# }
# fn extract_text(a: &Artifact) -> String { a.text().unwrap_or_default().to_owned() }
# struct CalcExecutor;
# agent_executor!(CalcExecutor, |_ctx, _queue| async { Ok(()) });
```

## Integration testing with HTTP

Test the full stack by starting a real server and using a client:

```rust,no_run
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
# use a2a_protocol_sdk::prelude::*;
# struct CalcExecutor;
# agent_executor!(CalcExecutor, |_ctx, _queue| async { Ok(()) });
use a2a_protocol_sdk::server::{JsonRpcDispatcher, RequestHandlerBuilder, serve_with_addr};
use a2a_protocol_sdk::client::ClientBuilder;
use std::sync::Arc;

#[tokio::test]
async fn test_end_to_end() {
    // Build handler and server. Port 0 lets the OS pick a free port, and
    // `serve_with_addr` returns the address it bound.
    let handler = Arc::new(
        RequestHandlerBuilder::new(CalcExecutor).with_agent_card(card()).build().unwrap()
    );
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler))
        .await
        .unwrap();

    // Build client
    let client = ClientBuilder::new(format!("http://{addr}"))
        .build()
        .unwrap();

    // Send a message
    let response = client
        .send_message(MessageSendParams::new(Message::user_text(
            "test-msg",
            "10 + 20",
        )))
        .await
        .unwrap();

    // Verify
    if let SendMessageResponse::Task(task) = response {
        assert_eq!(task.status.state, TaskState::Completed);
    } else {
        panic!("expected task response");
    }
}
# fn main() {}
```

## Testing both transports

Run the same tests against both JSON-RPC and REST (the `HTTP+JSON` binding):

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# async fn start_jsonrpc_server() -> std::net::SocketAddr { unimplemented!() }
# async fn start_rest_server() -> std::net::SocketAddr { unimplemented!() }
#[tokio::test]
async fn test_jsonrpc_transport() {
    let addr = start_jsonrpc_server().await;
    let client = ClientBuilder::new(format!("http://{addr}")).build().unwrap();
    run_test_suite(&client).await;
}

#[tokio::test]
async fn test_rest_transport() {
    let addr = start_rest_server().await;
    let client = ClientBuilder::new(format!("http://{addr}"))
        .with_protocol_binding("HTTP+JSON")
        .build().unwrap();
    run_test_suite(&client).await;
}

async fn run_test_suite(client: &A2aClient) {
    // Test send_message, stream_message, get_task, etc.
}
# fn main() {}
```

## Testing streaming

The first event of a streaming send is a full `Task` snapshot, so count it
with the events the executor emits:

```rust,no_run
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
use a2a_protocol_sdk::prelude::*;
use std::sync::Arc;
# struct CalcExecutor;
# agent_executor!(CalcExecutor, |_ctx, _queue| async { Ok(()) });

#[tokio::test]
async fn test_streaming() {
    let handler = Arc::new(
        RequestHandlerBuilder::new(CalcExecutor).with_agent_card(card()).build().unwrap()
    );
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler))
        .await
        .unwrap();
    let client = ClientBuilder::new(format!("http://{addr}")).build().unwrap();
    let params = MessageSendParams::new(Message::user_text("m1", "10 + 20"));

    let mut stream = client.stream_message(params).await.unwrap();
    let mut events = vec![];

    while let Some(event) = stream.next().await {
        events.push(event.unwrap());
    }

    // Verify event sequence: at least the initial Task snapshot, the
    // artifact and the terminal status
    assert!(events.len() >= 3);
}
# fn main() {}
```
