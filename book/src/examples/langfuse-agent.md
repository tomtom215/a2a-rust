# Langfuse Agent

Two A2A agents, an orchestrator that delegates every message to a worker,
traced into **one** [Langfuse](https://langfuse.com) trace by the SDK's own
`Telemetry`. Neither agent contains tracing code: everything Langfuse shows is
what the SDK records.

```bash
LANGFUSE_PUBLIC_KEY=pk-lf-local LANGFUSE_SECRET_KEY=sk-lf-local \
cargo run -p langfuse-agent -- "summarise the quarterly report"
```

The keys above are for a Langfuse of your own: the example's
[README](https://github.com/tomtom215/a2a-rust/blob/main/examples/langfuse-agent/README.md)
starts one from Langfuse's own `docker-compose.yml` with those keys created on
first start. `LANGFUSE_BASE_URL` defaults to `http://localhost:3000`, where that
compose file serves. Set it for any other instance, including Langfuse Cloud,
with that project's keys.

The program prints the orchestrator's report and the session id to open in
Langfuse. Without `LANGFUSE_PUBLIC_KEY` it exports to whatever
`OTEL_EXPORTER_OTLP_*` names, and to nothing when none is set or with
`OTEL_SDK_DISABLED=true`.

## What you will see

One trace, seven observations, each the child of the one before:

```text
user request                                SPAN
└ SendMessage (client)                      SPAN
  └ SendMessage (server)                    SPAN
    └ invoke_agent orchestrator             AGENT
      └ SendMessage (client, to worker)     SPAN
        └ SendMessage (server)              SPAN
          └ invoke_agent worker             AGENT
```

Each `SendMessage` is a span named `lf.a2a.v1.A2AService/SendMessage`. The two
`AGENT` observations carry the captured input and output.

The trace holds together across the two agents because the client sends a
`traceparent` with each call and the server continues it; nothing in the example
does that by hand. Every observation but the root carries the session: the A2A
`contextId` the example sends.

## Content capture is on, deliberately

The example turns on `with_span_content_capture(true)`, so the messages show as
input and output. That copies what users and agents say into Langfuse. It is off
by default, and a real deployment should decide on it deliberately. (The
[Audit Trail](../deployment/audit.md) takes the opposite default: it records
digests, never text.)

## Tests

`cargo test -p langfuse-agent` checks the same shape with no network: both runs
are `invoke_agent` spans in one trace and one context, the worker's run nested
under the orchestrator's, each with captured messages, and the Langfuse preset
builds offline with metrics and logs off.

## Where next

- [Langfuse](../deployment/langfuse.md) explains each piece of the preset.
- [Observability](../deployment/observability.md) covers the OTLP pipeline, the
  metrics and the dashboards the same telemetry feeds.
