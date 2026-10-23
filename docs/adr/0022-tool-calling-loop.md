<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0022: A tool-calling loop in the SDK

**Date:** 2026-10-20
**Status:** Accepted
**Author:** Tom F.

---

## Context

Until 2026-10-20 the maintainer's standing decision was that tool calling
stays in the examples: `examples/rig-agent` and `examples/mcp-agent` each
carry their own copy of the same bounded loop over rig-core. On 2026-10-20
the maintainer reversed it: agents built on this SDK should get full tool
calling from the SDK itself.

What the two examples taught, measured that day against llama.cpp b11751
(`8466395`) on Qwen3-1.7B and Qwen3.5 0.8B and 2B, with the loop and the
same tools driven directly against `/v1/chat/completions` as a control:

- A loop is not a single completion call. A model asks for tools, the
  results go back as their own messages, and the model asks again or
  answers; it needs a turn bound, or a model that never stops calling tools
  holds the A2A task open until the executor timeout.
- Small models sometimes write the call as the reply's text instead of as a
  call, and the server's parser does not lift it out (1 in 20 billing runs
  on Qwen3-1.7B). A loop that does not recognise that answers the caller with
  raw JSON.
- A tool's error is information for the model, not a failure of the task:
  "no service named 'billing'; call list_services" is how a model recovers.
- llama.cpp can itself fail a turn: with thinking on, it answered HTTP 500
  ("Failed to parse tool call arguments as JSON") once in eight runs on
  Qwen3.5-0.8B. A loop must report that as the model provider's failure.
- What the caller can check is the trace: one line per tool call and its
  result, beside the answer (the examples' `tool-trace` artifact).

The constraints are this repository's: `deny.toml` bans `reqwest` from the
published crates, ADR 0002 prefers no new dependency where the graph already
has one, and `CONTRIBUTING.md` "Extending a public trait" governs every new
open trait.

## Options

1. **A module in `a2a-protocol-server`, feature-gated, re-exported by the
   SDK.** The loop, a provider-neutral model trait, a tool registry, an
   executor that runs the loop as an A2A task, and one built-in model client
   for the OpenAI-compatible chat-completions API on the hyper and rustls
   stack the crate already has for push delivery and OTLP. No new
   dependency.
2. **A new crate.** The same content as its own published crate. Cleaner
   separation, but a fifth crate in `RELEASING.md`'s dependency order, its
   lockfiles, `release.yml` and every version pin, for code that only the
   server side uses.
3. **Adapters over rig-core or genai.** Reuse their provider clients. Both
   bring large trees (genai 0.7 pulls AWS SDK crates behind a feature,
   rig-core reqwest), both broke their APIs within the last month (genai 0.7
   removed `Client::default`), and rig-core 0.45 already needs Rust 1.97.1
   against this workspace's MSRV of 1.88.

## Decision

Option 1, feature `tool-loop` on `a2a-protocol-server`, forwarded and
re-exported by `a2a-protocol-sdk`. Off by default.

- **`ToolModel`** (open trait, object-safe, boxed futures like
  `AgentExecutor`): one completion turn. Input: a system preamble, the
  conversation so far, and the tool specifications. Output: the turn's text
  and its tool calls (id, name, JSON arguments). Errors are the provider's.
- **`Tool`** (open trait): a name, a description, a JSON Schema for its
  arguments, and `call(arguments) -> Result<String, ToolError>`. A
  `ToolError` goes back to the model as that call's result; only a
  `ToolModel` error or a panic-free transport failure of the tool's own
  choosing (`ToolError::Fatal`) ends the task.
- **`ToolSet`**: the registry; refuses two tools with one name.
- **`ToolLoop`**: runs turns until the model answers without a call, up to
  `max_turns` (default 8). A reply that is exactly one JSON object naming a
  registered tool (bare, fenced, or in `<tool_call>` tags) is run as that
  call (`recover_text_calls`, on by default). The answer is the final text
  and the trace, one line per call and its result.
- **`ToolLoopExecutor`**: an `AgentExecutor` that takes the user message's
  text, runs the loop, and emits `Working`, the answer artifact, the trace
  artifact (when a tool ran), and `Completed`; or `Failed` with the
  failure-class extension (`internal` for the provider, `budget-exhausted`
  at the turn bound). It honours the cancellation token between turns.
- **`OpenAiChatModel`**: a `ToolModel` for `POST {base}/chat/completions`
  (OpenAI, llama.cpp's llama-server, vLLM, Ollama), on hyper; HTTPS needs
  `tls-rustls`, as push delivery does. API key optional. Request timeout
  bounded. A non-2xx answer is a provider error carrying the status and at
  most 512 bytes of the body.
- **Telemetry**: each model turn and each tool call gets a span named and
  attributed per the GenAI semantic conventions (`chat {model}`,
  `execute_tool {name}`), inside the executor's `invoke_agent` span, with
  no prompt or tool content recorded unless the existing content-capture
  setting allows it.
- **Examples**: `rig-agent` and `mcp-agent` keep rig and MCP as the
  integrations they demonstrate; a new path in `genai-agent` or a new small
  example shows the SDK loop end to end against a live model.

## Consequences

- Two new open traits. Every rule in "Extending a public trait" applies from
  the start: methods added later get defaults with a stated cost.
- The built-in client speaks one wire format. Providers with another
  (Anthropic's messages API, Gemini) are reached through their
  OpenAI-compatible endpoints or a `ToolModel` the integrator writes.
- The loop's limits are measured, not assumed: the examples' live runs on
  2026-10-20 are the baseline (`examples/rig-agent/README.md`).
- The examples-only decision in `docs/handoff.md` is superseded.
