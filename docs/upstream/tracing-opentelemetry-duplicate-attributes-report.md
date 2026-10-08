<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Draft issue for `tokio-rs/tracing-opentelemetry` — a field recorded twice is exported as two attributes with one key

**Status: DRAFT, not sent.** Duplicate check: a web search on 2026-10-06
found `tokio-rs/tracing#2123` (the `fmt` subscriber prints a re-recorded
field twice) and nothing for this crate. Not exhaustive; search the tracker
before filing.

---

## Title

Recording a span field twice exports two attributes with the same key

## Body

`tracing-opentelemetry` 0.34.0 (also 0.33.0). `span.record("k", "a")` then
`span.record("k", "b")` before the span is started appends:

```rust
// src/layer.rs:233-238, SpanBuilderUpdates::update
if let Some(builder_attributes) = &mut span_builder.attributes {
    builder_attributes.extend(attributes);
```

so the exported span carries `k = "a"` and `k = "b"`. The OpenTelemetry trace
API says "Setting an attribute with the same key as an existing attribute
SHOULD overwrite the existing attribute's value" (`specification/trace/api.md`,
*Set Attributes*). Readers disagree on which value wins: an in-memory exporter
consumer taking the first sees the stale `"a"`. Found in `a2a-rust`, where a
task's state recorded as `WORKING` then `COMPLETED` read `WORKING`.

Expected: a later record of a key replaces the earlier one, as the SDK span's
`set_attribute` is specified to.
