<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Tools and idempotency

A tool is anything the model may call: a lookup, a search, a restart. This
page shows how to write one, how its errors reach the model, how tools are
collected into a `ToolSet`, and how the idempotency key each call carries
keeps a side effect from happening twice
([ADR 0026](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0026-tool-call-idempotency.md)).

## The `Tool` trait

| Method | What it is for |
|---|---|
| `name()` | The name the model calls it by; unique within a `ToolSet` |
| `description()` | What it does. The model chooses tools by this text, so say when to call it as well as what it returns |
| `parameters()` | A JSON Schema for the arguments, an `object` schema |
| `read_only()` | Whether a call changes nothing, so running it again is harmless. Defaults to `false` |
| `call(invocation)` | Runs the call: `Ok(text)` is the result the model sees |

`call` is given a `ToolInvocation`: `arguments`, always a JSON object, and
`idempotency_key`. A tool with a side effect passes the key to its backend
where the backend takes one (an `Idempotency-Key` header, a unique request id),
so a retried request does its work once:

```rust,no_run
use std::future::Future;
use std::pin::Pin;

use a2a_protocol_sdk::server::tool_loop::{Tool, ToolError, ToolInvocation};

/// Restarts a service. Not read-only, so a repeated call within a run is
/// answered from the first rather than restarting the service again.
struct RestartService;

impl Tool for RestartService {
    fn name(&self) -> &str { "restart_service" }
    fn description(&self) -> &str {
        "Restarts the named service. Call list_services first if you are unsure of the name."
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {"service": {"type": "string"}},
            "required": ["service"]
        })
    }
    fn call<'a>(
        &'a self,
        call: ToolInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        Box::pin(async move {
            let Some(service) = call.arguments.get("service").and_then(|v| v.as_str()) else {
                // The model can fix this: tell it what was wrong.
                return Err(ToolError::Failed("the 'service' argument is required".to_owned()));
            };
            // The backend deduplicates on the key; its outage is not the model's to fix.
            restart(service, &call.idempotency_key).await.map_err(ToolError::Fatal)?;
            Ok(format!("{service} restarted"))
        })
    }
}
# async fn restart(_service: &str, _idempotency_key: &str) -> Result<(), String> { Ok(()) }
```

A lookup declares itself read-only with `fn read_only(&self) -> bool { true }`,
as the `Clock` on [the chapter's landing page](../tool-loop.md#a-minimal-agent)
does.

## How a call can fail

| What happened | What the model is sent | Does the run go on? |
|---|---|---|
| `Err(ToolError::Failed(message))` | `error: <message>` as the call's result | Yes |
| The model named a tool not in the set | `error: no tool named '<name>'; the tools are: <names>` | Yes |
| The arguments are not a JSON object | `error: the arguments must be a JSON object`, or that they are not valid JSON | Yes |
| `Err(ToolError::Fatal(message))` | Nothing: the task ends `Failed`, class `internal` | No |

`Failed` is the common case: a bad argument, a name that does not exist, a
record not found. The model reads the message and tries again. `Fatal` is for
what no further turn can fix, such as the tool's own backend being down. Empty
argument text is taken as no arguments (`{}`), which some providers send for a
tool that takes none.

## Collecting tools: `ToolSet`

`ToolSet` holds the tools a loop offers its model, in the order they were
added. Names are unique: adding a second tool with a name already in the set
returns `DuplicateTool` and leaves the set unchanged.

| Method | Use |
|---|---|
| `ToolSet::new()` | An empty set. A loop over it is a plain chat |
| `with(tool)` | Adds a tool by value, for building a set in one expression |
| `add(tool)` | Adds a tool to a set you hold mutably |
| `add_arc(Arc<dyn Tool>)` | Adds a shared tool, for one instance offered by several sets |
| `get(name)`, `names()`, `specs()`, `len()`, `is_empty()` | Reading the set back; `specs()` is the catalogue as the model is told it |

```rust,no_run
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::server::tool_loop::{Tool, ToolError, ToolInvocation};
# struct ListServices;
# impl Tool for ListServices {
#     fn name(&self) -> &str { "list_services" }
#     fn description(&self) -> &str { "The services in the inventory." }
#     fn parameters(&self) -> serde_json::Value { serde_json::json!({"type": "object"}) }
#     fn read_only(&self) -> bool { true }
#     fn call<'a>(&'a self, _call: ToolInvocation)
#         -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
#         Box::pin(async { Ok("checkout, search".to_owned()) })
#     }
# }
# struct ServiceStatus;
# impl Tool for ServiceStatus {
#     fn name(&self) -> &str { "service_status" }
#     fn description(&self) -> &str { "The status of one service." }
#     fn parameters(&self) -> serde_json::Value { serde_json::json!({"type": "object"}) }
#     fn read_only(&self) -> bool { true }
#     fn call<'a>(&'a self, _call: ToolInvocation)
#         -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
#         Box::pin(async { Ok("healthy".to_owned()) })
#     }
# }
use a2a_protocol_sdk::server::tool_loop::ToolSet;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let tools = ToolSet::new().with(ListServices)?.with(ServiceStatus)?;
assert_eq!(tools.names(), ["list_services", "service_status"]);
# Ok(())
# }
```

## Idempotency keys

Two things call a tool twice for what was meant once. The model repeats
itself: in the tool-calling study ADR 0026 cites, a run repeated a call with
the same tool and arguments in up to 16 of a cell's 60 runs. And a tool that
calls a backend over a network has the ambiguous failure of any retried
request: the first attempt may have landed. So every call carries a key, and
the loop itself refuses to repeat a side effect within a run.

**The key.** HMAC-SHA256, under the loop's secret, of the run's scope, the
tool's name and the arguments in canonical form (object fields sorted, no
whitespace), rendered as 64 lowercase hex digits. The same call in the same
scope has the same key whatever order the model wrote the arguments in;
arguments that differ only in how a number is spelled (`1` and `1.0`) are
different calls.

**The scope.** Under `ToolLoopExecutor`, the task id and the message id, so
another message to the same task is new work with new keys. `ToolLoop::run`
and `run_within` draw a random scope per run; `run_scoped` takes one from the
caller.

**The secret.** 32 bytes, drawn for each loop by default, so keys differ
between processes. Replicas that should give the same call the same key set a
shared one:

```rust,no_run
# use a2a_protocol_sdk::server::tool_loop::{OpenAiChatModel, ToolLoop, ToolSet};
# fn secret_from_your_vault() -> [u8; 32] { [7; 32] }
# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let model = OpenAiChatModel::new("http://127.0.0.1:8080/v1", "qwen3:1.7b")?;
let tool_loop = ToolLoop::new(model, ToolSet::new())
    .with_idempotency_secret(secret_from_your_vault());
# Ok(())
# }
```

Keep it as secret as a credential: with it, anyone can compute keys from task
ids and arguments, which is why the key is keyed rather than a plain hash.

**A repeated call runs once.** Within one run, a call whose key matches a call
that succeeded is not run again. The model is sent
`not run again: an identical call already ran in this task and returned: <result>`,
and no tool span is opened, since nothing ran. A call that failed
(`ToolError::Failed`) is run again, with the same key, so a backend that did
the work before the failure can recognise the retry. A read-only tool runs
every time, so a model can poll it for a change.

Declaring a tool read-only is the opt-out, and the default is the safe
mistake: a lookup left undeclared answers a repeated question from the first
answer, while an effect wrongly declared read-only would act twice.

What the keys do not cover: a call repeated in a later run (another message,
or a run outside the executor) is not answered from the earlier one; that is
the backend's key to honour. A message resent without a send key is a new
task, with new keys, and its tools run again; the send key
([Idempotency](../../client/idempotency.md)) is what makes a resend the same
work.
