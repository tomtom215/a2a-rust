<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# langfuse-agent

Two A2A agents — an orchestrator that delegates every message to a worker —
traced into one [Langfuse](https://langfuse.com) trace by the SDK's own
`Telemetry`. Neither agent contains tracing code; everything Langfuse shows is
what the SDK records. The book's [Langfuse page](../../book/src/deployment/langfuse.md)
explains each piece.

## Run it

Against Langfuse Cloud, with a project's keys:

```bash
export LANGFUSE_PUBLIC_KEY=pk-lf-... LANGFUSE_SECRET_KEY=sk-lf-...
cargo run -p langfuse-agent -- "summarise the quarterly report"
```

`LANGFUSE_BASE_URL` defaults to the EU region, as Langfuse's SDKs do; set it
for another region or a self-hosted instance.

Against a self-hosted Langfuse, from a clone of `langfuse/langfuse` (its
`docker-compose.yml` reads these variables to create a project with known keys
on first start; `TELEMETRY_ENABLED=false` turns off Langfuse's own usage
reporting):

```bash
cat > langfuse.env <<'EOF'
TELEMETRY_ENABLED=false
LANGFUSE_INIT_ORG_ID=local
LANGFUSE_INIT_PROJECT_ID=local
LANGFUSE_INIT_PROJECT_PUBLIC_KEY=pk-lf-local
LANGFUSE_INIT_PROJECT_SECRET_KEY=sk-lf-local
LANGFUSE_INIT_USER_EMAIL=you@example.com
LANGFUSE_INIT_USER_PASSWORD=change-me-please
EOF
docker compose --env-file langfuse.env up -d

LANGFUSE_PUBLIC_KEY=pk-lf-local LANGFUSE_SECRET_KEY=sk-lf-local \
LANGFUSE_BASE_URL=http://localhost:3000 \
cargo run -p langfuse-agent
```

The program prints the orchestrator's report and the session id to open in
Langfuse. Without `LANGFUSE_PUBLIC_KEY` it exports to whatever
`OTEL_EXPORTER_OTLP_*` names, or nothing with `OTEL_SDK_DISABLED=true`.

## What you will see

One trace, seven observations, each the child of the one before:

```text
user request                                  SPAN
└ lf.a2a.v1.A2AService/SendMessage (client)   SPAN
  └ lf.a2a.v1.A2AService/SendMessage (server) SPAN
    └ invoke_agent orchestrator               AGENT   input/output captured
      └ lf.a2a.v1.A2AService/SendMessage      SPAN    (client, to the worker)
        └ lf.a2a.v1.A2AService/SendMessage    SPAN    (server)
          └ invoke_agent worker               AGENT   input/output captured
```

Every observation but the root carries the session — the A2A `contextId` the
example sends.

The example turns on `with_span_content_capture(true)` so the messages show
as input and output. That copies what users and agents say into Langfuse; it
is off by default, and a real deployment should decide on it deliberately.

## Tests

`cargo test -p langfuse-agent` checks the same shape with no network: both
runs are `invoke_agent` spans in one trace and one context, the worker's run
nested under the orchestrator's, each with captured messages; and the Langfuse
preset builds offline with metrics and logs off.
