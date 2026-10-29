<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Spans and metrics

Every model turn and every tool call the loop makes is a span, and each run,
turn and call is measured. The names and attributes are the OpenTelemetry
`GenAI` semantic conventions' (`open-telemetry/semantic-conventions-genai` at
`9ffc7e0`, all `development` upstream, so they may change). This page lists
what the loop records; how to export it is in
[Observability](../../deployment/observability.md).

Spans need the `tracing` feature and metrics the `otel` feature; both are on
by default.

## Spans

Under `ToolLoopExecutor` the spans open inside the executor's
`invoke_agent {agent}` span, which the handler records for every executor
unless [`with_agent_span_conventions(false)`](../handler/configuration.md#metrics-and-traces)
turns it into `a2a.execute`.
A run that took three turns and called one tool after each of the first two
has five children under that span, in order: `chat`, `execute_tool`, `chat`,
`execute_tool`, `chat`.

| Span | Kind | Attributes |
|---|---|---|
| `chat {model}`, one per model turn | `CLIENT` | `gen_ai.operation.name` = `chat`, `gen_ai.provider.name`, `gen_ai.request.model`; `server.address` and `server.port` when the model says where it is served; and what the provider reported: `gen_ai.usage.input_tokens`, `gen_ai.usage.output_tokens`, `gen_ai.response.model`, `gen_ai.response.id`, `gen_ai.response.finish_reasons` |
| `execute_tool {name}`, one per call that ran | `INTERNAL` | `gen_ai.operation.name` = `execute_tool`, `gen_ai.tool.name`, `gen_ai.tool.call.id`, `gen_ai.tool.type` = `function`, `gen_ai.tool.description` |

Both record `error.type` and an `Error` status when they fail, and
`error.type` = `cancelled` when they are abandoned unfinished: the
cancellation token fired, or the future running them was dropped, as an
executor timeout does. A turn that ends without a reply records
`gen_ai.response.finish_reasons` = `["error"]`. A call answered from an
earlier identical one ([Idempotency keys](./tools.md#idempotency-keys)) opens
no span, since nothing ran.

**No content.** The prompt, the tool arguments and the tool results
(`gen_ai.input.messages`, `gen_ai.tool.call.arguments`,
`gen_ai.tool.call.result`) are opt-in in the conventions, and these spans
record none of them. The handler's `with_span_content_capture` covers the
executor's own span, not these.

## Metrics

| Instrument | Kind | Unit | Recorded |
|---|---|---|---|
| `gen_ai.client.inference.duration` | histogram | `s` | Each model turn |
| `gen_ai.client.inference.usage.input_tokens` | counter | `{token}` | Each turn whose provider reported a non-zero count |
| `gen_ai.client.inference.usage.output_tokens` | counter | `{token}` | Likewise |
| `gen_ai.client.inference.operation.input_tokens` | histogram | `{token}` | Each turn whose provider reported a count |
| `gen_ai.client.inference.operation.output_tokens` | histogram | `{token}` | Likewise |
| `gen_ai.execute_tool.duration` | histogram | `s` | Each tool call that ran |
| `gen_ai.invoke_agent.duration` | histogram | `s` | Each run of the loop |
| `gen_ai.invoke_agent.inference_calls` | histogram | `{inference_call}` | Each run: how many turns it asked for |
| `gen_ai.invoke_agent.tool_calls` | histogram | `{tool_call}` | Each run: how many calls it ran |

A turn's instruments carry `gen_ai.operation.name` (`chat`),
`gen_ai.provider.name`, `gen_ai.request.model`, and when known
`gen_ai.response.model`, `server.address` and `server.port`; the usage
counters add `gen_ai.token.modality` = `text`. A run's carry
`gen_ai.request.model` and the name `with_agent_name` gives the loop.

`error.type` says why a turn, a call or a run failed:

| On | Values |
|---|---|
| A model turn | The HTTP status, `timeout`, `transport`, `invalid_response`, `response_too_large`, `configuration`, or `_OTHER` for a `ToolModel` of your own (`ModelError::error_type`) |
| A tool call | `unknown_tool`, `invalid_arguments`, `tool_error`, `tool_fatal` |
| A run | The turn's value when the model failed, `tool_fatal`, `turn_limit`, `budget_exhausted` |
| Any of them | `cancelled`, for work abandoned before it ended |

The metrics go to the global meter provider as it is when each is recorded, so
a `Telemetry` installed after the loop is built still receives them;
`ToolLoop::with_meter` sends them to a meter of your choosing instead.
`gen_ai.client.operation.duration` is not recorded: the conventions say it
"SHOULD NOT be reported for inference operations".
