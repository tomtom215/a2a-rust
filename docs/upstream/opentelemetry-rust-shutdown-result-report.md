<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Draft issue for `open-telemetry/opentelemetry-rust` — `BatchSpanProcessor::shutdown` returns `Ok` after its final export failed

**Status: DRAFT, not sent.** Duplicate check: a web search on 2026-10-06
found #1660 (shutdown racing the runtime; closed, different) and #536; no
issue about this result being discarded. The search was not exhaustive — the
GitHub issue search was not reachable from here. Search the tracker before
filing.

---

## Title

`BatchSpanProcessor::shutdown` returns `Ok(())` when its final export failed

## Body

`opentelemetry_sdk` 0.33.0 (also 0.32.1). The batch worker computes the final
export's result and sends it back:

```rust
// src/trace/span_processor.rs:630-642
BatchMessage::Shutdown(sender) => {
    let result = Self::get_spans_and_export(/* ... */);
    let _ = exporter.shutdown();
    let _ = sender.send(result);
```

and `shutdown_with_timeout` discards it:

```rust
// src/trace/span_processor.rs:942-951
receiver.recv_timeout(timeout).map(|_| { /* join */ OTelSdkResult::Ok(()) })
```

So a final export that fails — collector down, `401`, a TLS failure — is
logged (`BatchSpanProcessor.ExportError`) and `shutdown` returns `Ok`. A
caller that checks the result to decide whether telemetry was lost at exit
cannot. Expected: the received `OTelSdkResult` is returned (after the join).

Reproduction: an `SdkTracerProvider` with `with_batch_exporter` over an OTLP
HTTP exporter pointed at a closed port; record one span; `shutdown()` returns
`Ok(())` while the error is logged. Observed in `a2a-rust`'s
`tests/telemetry_export`, which now counts failures in its own HTTP client to
work around it.
