<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Draft comment for `open-telemetry/semantic-conventions-genai` #195 — notes from a Rust implementation

**Status: DRAFT, not sent.** Written 2026-10-06 against the pull request's
head `842a839` (`model/a2a/{registry,common,spans}.yaml`,
`docs/gen-ai/a2a.md`). Its state then, from GitHub's page (not the API, which
was not reachable here): open, with approving reviews. Re-read the head before
sending; anything below that the pull request has since changed should be
dropped.

---

`a2a-rust` (an A2A v1.0 SDK in Rust; Apache-2.0) now emits this pull
request's attributes on its server spans, whatever the binding, and on its
client spans; its end-to-end gate checks them over JSON-RPC, HTTP+JSON and
gRPC. Implementing it against a second
language turned up five things that may be useful to the review. Each is what
we did and why, not a request.

1. **Span names on HTTP.** We enrich our transport server span, as
   `a2a.http.server` says, but keep one name on every binding —
   `lf.a2a.v1.A2AService/{Method}`, the gRPC method — rather than renaming the
   HTTP span to `{a2a.method.name}`. A dashboard grouping one A2A call across
   bindings then needs no mapping. `a2a.grpc.server` already keeps the gRPC
   name; is a single cross-binding name something the conventions would
   consider for HTTP too?
2. **Streaming client spans and the bridge's end time.** "It covers ... the
   time to consume the response stream for streaming calls." In Rust's
   `tracing-opentelemetry` a span ends at its *last exit*, not when it is
   dropped, so holding a span for the stream's lifetime still ends it when
   the stream opened. We poll the stream inside the span to meet the text.
   Other bridges may have the same trap; a note might save the next
   implementer the same test failure.
3. **`a2a.task.state` values** match the protobuf enum names exactly
   (`TASK_STATE_COMPLETED`), which is also our JSON serialisation — no
   mapping table, and none to drift. That choice works well.
4. **`gen_ai.agent.*` on the client** comes from the card the client was
   built from; a client built for a bare URL has none, and records none.
   "When available from the Agent Card" covers this cleanly.
5. **Propagation is undefined**, as the description says. We send the client
   span's own context as `traceparent` (W3C Trace Context over HTTP headers or
   gRPC metadata) on every binding that has per-request headers; WebSocket has
   none after the upgrade. A sentence on where context travels — headers and
   metadata, not message metadata — would make cross-SDK traces join without
   each SDK deciding separately. MCP's conventions do this for `params._meta`.

The executor span is the GenAI conventions' internal `invoke_agent`, kept
apart from the A2A spans per this pull request's "SHOULD NOT report telemetry
describing higher level GenAI agent operations". Implementation:
`crates/a2a-protocol-server/src/rpc_span/{a2a,agent}.rs` and
`crates/a2a-protocol-client/src/call_span.rs`.
