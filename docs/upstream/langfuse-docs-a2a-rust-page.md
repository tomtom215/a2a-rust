<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Draft page for `langfuse/langfuse-docs` — A2A agents in Rust

**Status: DRAFT, not sent.** A candidate
`content/integrations/frameworks/a2a-rust.mdx`, modelled on that directory's
existing third-party Rust page, `swiftide.mdx` (front matter and `<Steps>`
layout read from `langfuse-docs@b050447`). It would also need an entry in
`content/integrations/frameworks/meta.json` and a logo. I found no written
policy for contributing an integration page; Langfuse's integrations index
says it tracks interest in new integrations through GitHub Discussions
(`content/integrations/index.mdx`), so opening a discussion first may be the
expected route.

Before sending:

- **Langfuse Cloud is untested.** Everything below was verified against a
  self-hosted Langfuse 4.53.0. Run the example against Cloud first.
- **The version** in the TOML must be the release that ships `Telemetry`.
  The page says `0.15` as a placeholder.
- **No Langfuse change is needed.** The page documents an existing path:
  `gen_ai.operation.name = invoke_agent` → AGENT (`ObservationTypeMapper.ts`)
  and `gen_ai.conversation.id` → session (`extractSessionId`).

Everything below the rule is the page.

---

````mdx
---
title: Trace A2A agents in Rust with Langfuse
sidebarTitle: A2A (Rust)
description: Trace Agent2Agent (A2A) protocol agents built with a2a-rust in Langfuse — every agent run, every agent-to-agent call, one trace per delegation chain.
category: Integrations
---

# Tracing A2A agents in Rust with Langfuse

> **What is a2a-rust?** [a2a-rust](https://github.com/tomtom215/a2a-rust) is
> a Rust SDK for the [Agent2Agent (A2A) protocol](https://a2a-protocol.org):
> client, server and the four bindings (JSON-RPC, HTTP+JSON, gRPC,
> WebSocket). It exports OpenTelemetry traces itself, using the OpenTelemetry
> GenAI conventions Langfuse maps.

What you get, with no tracing code in your agent:

- each agent run as an **agent** observation, `invoke_agent {agent name}`;
- each A2A call, caller and callee side, as spans between them, so an agent
  that delegates to another shows the other's run nested under its own — one
  trace across processes, through W3C `traceparent`;
- each A2A context (`contextId`) as a Langfuse **session**;
- optionally, the messages in and out as the observation's input and output.

<Steps>

## Install

```toml
a2a-protocol-sdk = { version = "0.15", features = ["otel"] }
tracing-subscriber = "0.3"
```

## Configure Langfuse credentials

The same variables as the Langfuse SDKs. Unlike them, a2a-rust defaults the
base URL to a [self-hosted](/self-hosting) instance at `http://localhost:3000`;
set it for any other, including Langfuse Cloud:

```bash
export LANGFUSE_PUBLIC_KEY=pk-lf-...
export LANGFUSE_SECRET_KEY=sk-lf-...
export LANGFUSE_BASE_URL=http://localhost:3000  # default; or https://cloud.langfuse.com, https://us.cloud.langfuse.com
```

## Install telemetry

```rust
use a2a_protocol_sdk::server::otel::{Langfuse, Telemetry};
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

let telemetry = Telemetry::builder()
    .with_default_service_name("my-agent")
    .with_langfuse(Langfuse::from_env()?)
    .build()?;
tracing_subscriber::registry().with(telemetry.layer()).init();

// build and serve your agent ...

telemetry.shutdown()?; // flushes before exit
```

This sends traces to `/api/public/otel/v1/traces` over OTLP/HTTP with
`x-langfuse-ingestion-version: 4`. Metrics and logs are off, since Langfuse
ingests traces.

## Capture messages (optional)

```rust
RequestHandlerBuilder::new(executor)
    .with_span_content_capture(true)
```

This copies user and agent messages into Langfuse; it is off by default.

## See it in Langfuse

Run the repository's example, an orchestrator agent delegating to a worker:

```bash
cargo run -p langfuse-agent -- "summarise the quarterly report"
```

</Steps>
````
