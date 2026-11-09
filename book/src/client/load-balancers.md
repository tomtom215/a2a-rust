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

There are four remedies. Use whichever your deployment allows:

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

3. **Let the client stream for you.** With
   `ClientBuilder::with_blocking_over_stream(true)`, `send_message` to an
   agent whose card declares streaming is sent as `SendStreamingMessage`
   and answered, as before, with the task: the code that calls it does not
   change, and the stream's keep-alive comments are what cross the balancer.

   ```rust,no_run
   use a2a_protocol_client::ClientBuilder;
   use a2a_protocol_types::AgentCard;

   # fn f(card: AgentCard) -> a2a_protocol_client::ClientResult<()> {
   // Built from the card, which says the agent streams.
   let client = ClientBuilder::from_card(&card)?
       .with_blocking_over_stream(true)
       .build()?;
   // `client.send_message(params).await?` now rides on a stream.
   # drop(client);
   # Ok(())
   # }
   ```

   What the answer is, and what it costs:

   - The stream is read until the task is terminal or interrupted
     (`input-required`, `auth-required`), where a blocking send returns, and
     the answer is the task **as the agent stores it** (a `GetTask` with the
     send's `history_length`), not the client's reading of the events. That
     is what the agent's own blocking answer is, and the same under
     [`history_length`](./sending-messages.md). The store can be one event
     behind the stream for a moment, so the read is repeated for about three
     seconds; a task that never reaches the state the stream ended on is
     `ClientError::InvalidAgentResponse`, never an older state passed off as
     the answer.
   - An agent that answers with one `Message` and no task is answered with it.
   - It needs a card that declares streaming. A client built for a bare
     endpoint has none to read and sends as it always did; so does a send
     with `return_immediately`.
   - Three wire calls instead of one. An interceptor, a span or a metric
     sees `SendStreamingMessage` and `GetTask`, not `SendMessage`.
   - `with_send_timeout` still bounds the whole send. A stream cut before the
     task settles ends the send with the stream's error, as a cut blocking
     send does; the task goes on, and
     [`follow_task`](../building-agents/executor/interrupted.md) picks it up.

   Tested against this repository's server behind a proxy that closes a
   connection after 600 ms of silence: the plain blocking send is cut, on
   both HTTP bindings, and the carried one is not
   (`crates/a2a-protocol-client/tests/blocking_over_stream_e2e.rs`).

4. **Raise the balancer's idle timeout** above the longest turn you expect
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

### Balancing in the client

Recycling spreads connections over time, not calls at a given moment. A
client that knows the replicas can spread its own calls
(`ClientBuilder::with_balancing`, ADR 0050). Each call goes to the endpoint
with the fewest calls in flight (a stream counts until it is dropped), a tie
to the one chosen longest ago, so calls one after another go round the set.
The endpoints come from an `EndpointSource`:

| Source | Endpoints |
|---|---|
| `StaticEndpoints::new([...])` | A list of replica URLs |
| `StaticEndpoints::from_card(&card, binding)` | The card's interfaces of one binding and tenant, for an agent that publishes one URL per replica |
| `DnsEndpoints::new(url)` | Every address the URL's host resolves to, one endpoint each, resolved again every 30 s (`with_refresh_every`): a Kubernetes headless Service |
| your own type | Implement `endpoints()` and, if it changes, `refresh_every()` |

An endpoint from `DnsEndpoints` is *pinned*: the connection goes to its
address, and the `Host` header, TLS server name and certificate check keep
the name, so one certificate for the service name serves every pod. Through a
proxy the proxy resolves the name, and the pins have no effect.

```rust,no_run
use std::time::Duration;
use a2a_protocol_client::ClientBuilder;
use a2a_protocol_client::balance::{BalancePolicy, DnsEndpoints, Hedge};

# fn main() -> a2a_protocol_client::ClientResult<()> {
let url = "https://agent.ns.svc.cluster.local:8443";
let client = ClientBuilder::new(url)
    .with_balancing(
        DnsEndpoints::new(url)?,
        BalancePolicy::new().with_hedge(Hedge::after(Duration::from_millis(200))),
    )
    .build()?;
# drop(client);
# Ok(())
# }
```

**Ejection.** An endpoint that fails 5 calls in a row (a connection error, a
timeout, `502`, `503` or `504`; any other answer counts as the endpoint
answering) is out of rotation for 30 s, then one call probes it: success puts
it back, failure keeps it out twice as long, up to 300 s
(`BalancePolicy::with_ejection(Ejection::new(..))`, `without_ejection()`).
No more than half the set (`with_max_ejected_percent`, rounded down) is out at
once: past that, the endpoints whose ejection ends soonest stay in use, so a
blip on every replica does not become an outage on the client. At 100 the set
is a circuit breaker: with every endpoint out, calls fail at once with
`ClientError::CircuitOpen`, which is retryable.

**Hedging** is off unless asked for (`Hedge::after(delay)`). A unary call
with no answer after `delay` is sent to a second endpoint too, and the first
answer that is not an error is returned. Only idempotent methods are hedged
(`GetTask`, `ListTasks`, `CancelTask`, `GetExtendedAgentCard`, the
push-config reads and delete) and a `SendMessage` carrying an idempotency key
to a peer known to honour it; a send without a key never is, nor a stream.
Copies are paid from a budget, by default 5% of calls.

Balancing applies to the HTTP bindings (JSON-RPC, HTTP+JSON); `build_grpc` and
a custom transport refuse it. With an [`HttpPool`](./builder/pool.md), the
pool's connections and its per-origin state are used, so ejection is shared by
every client of the pool.

## Keep-alive PINGs and the next hop

An HTTP/2 PING is answered by the next hop. Through a layer-4 balancer, which
forwards TCP, that is the agent, and the traffic stops the balancer's own idle
timer running out on a connection that is otherwise quiet. A balancer that
terminates HTTP/2 answers the PING itself: it shows the path to the balancer
is up and nothing beyond it, and it does not stop a layer-7 balancer's read
timeout on a request that is waiting for the agent. So PINGs are no remedy
for a blocking send behind a layer-7 balancer; remedies 1 to 4 above are.

A client sends HTTP/2 PINGs only when asked:
[`HttpPool`](./builder/pool.md) for the HTTP bindings,
[`with_grpc_keep_alive`](./builder/grpc.md) for gRPC. A server that limits
PINGs closes a connection that sends them too often; grpc-go's default is to
allow one every five minutes.
