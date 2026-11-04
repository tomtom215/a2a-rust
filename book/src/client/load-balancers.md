<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Behind a load balancer

A load balancer or reverse proxy between your client and the agent closes a
connection that carries no data for too long, and answers the client with an
error while the agent carries on. Two things in this SDK meet that limit: a
blocking `send_message`, and a long-lived HTTP/2 connection.

## A blocking send that outlasts the balancer's idle timeout

`send_message` without `return_immediately` is answered when the agent's turn
ends, and an agent that calls a model takes as long as the model does. The
client waits for up to ten minutes (`ClientBuilder::with_send_timeout`;
`DEFAULT_SEND_TIMEOUT`). Most balancers do not: AWS ALB and nginx's
`proxy_read_timeout` both default to 60 seconds. During a turn that long
nothing crosses the connection, so the balancer ends it and the client gets
a `504`, or a reset, or a timeout, depending on the balancer.

The task is not cancelled. It goes on running on the agent, and the client,
which never saw the answer, does not know its id.

There are three remedies. Use whichever your deployment allows:

1. **Do not wait on the connection.** Send with `return_immediately` and
   follow the task. The send is answered at once, with the task, and no
   connection is idle for longer than the time between your polls.

   ```rust,no_run
   # use a2a_protocol_sdk::prelude::*;
   use a2a_protocol_sdk::types::params::{SendMessageConfiguration, TaskQueryParams};
   # async fn f(client: A2aClient) -> Result<(), Box<dyn std::error::Error>> {
   let params = MessageSendParams::new(Message::user_text(new_message_id(), "A long job"))
       .with_configuration(SendMessageConfiguration::new().with_return_immediately(true));
   let SendMessageResponse::Task(task) = client.send_message(params).await? else {
       return Ok(()); // the agent answered with a message, not a task
   };
   let finished = loop {
       let task = client
           .get_task(TaskQueryParams::new(task.id.to_string()))
           .await?;
       if task.status.state.is_terminal() {
           break task;
       }
       tokio::time::sleep(std::time::Duration::from_secs(5)).await;
   };
   # let _ = finished;
   # Ok(())
   # }
   ```

   From a shell: `a2a send URL TEXT --no-wait`, then `a2a task get URL ID`
   (see [the command-line client](../reference/cli.md)).

2. **Stream.** `send_streaming_message` (or `subscribe_to_task` on a task you
   already have) holds the connection open and keeps it busy: the SDK's
   server sends an SSE comment every 30 seconds by default
   (`DispatchConfig::sse_keep_alive_interval`), which is inside a 60-second
   idle timeout. See [Streaming Responses](./streaming.md). A server that is
   not this SDK's needs to do the same; a stream behind a balancer whose
   timeout is shorter than the server's heartbeat is cut like a blocking send.

3. **Raise the balancer's idle timeout** above the longest turn you expect
   (`idle_timeout.timeout_seconds` on an ALB, `proxy_read_timeout` in nginx).
   This is the only remedy that keeps a plain blocking send working, and it
   moves the limit rather than removing it.

Polling with `get_task` is safe to repeat, so remedy 1 also works with a
client that has a [retry policy](./builder/options.md).

## A long-lived HTTP/2 connection pinned to one replica

A layer-4 balancer (one that forwards TCP and does not read HTTP) picks a
replica when a connection opens, and every request on it goes there. HTTP/2
puts all of a client's requests on one connection, so a client that talks to
a service name sends everything to one replica for as long as the connection
lives, and a supervisor that fans out to the service sees the imbalance.

An [`HttpPool`](./builder/pool.md) can recycle its connections:
`with_max_connection_lifetime` makes it open new ones once the current ones
are that old, without cutting a call or a stream that is running. A layer-7
balancer, which reads HTTP, spreads requests over replicas itself and needs
none of this.

## Keep-alive PINGs and the next hop

An HTTP/2 PING is answered by the next hop. Through a layer-4 balancer, which
forwards TCP, that is the agent, and the traffic stops the balancer's own idle
timer running out on a connection that is otherwise quiet. A balancer that
terminates HTTP/2 answers the PING itself: it shows the path to the balancer
is up and nothing beyond it, and it does not stop a layer-7 balancer's read
timeout on a request that is waiting for the agent. So PINGs are no remedy
for a blocking send behind a layer-7 balancer; remedies 1 to 3 above are.

A client sends HTTP/2 PINGs only when asked:
[`HttpPool`](./builder/pool.md) for the HTTP bindings,
[`with_grpc_keep_alive`](./builder/grpc.md) for gRPC. A server that limits
PINGs closes a connection that sends them too often; grpc-go's default is to
allow one every five minutes.
