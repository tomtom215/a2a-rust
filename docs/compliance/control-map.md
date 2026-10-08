<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->
<!-- GENERATED from controls.toml by scripts/check_compliance_map.py --write. Do not edit. -->

# Regulatory control map

This is a map from regulatory and standards provisions to what a2a-rust, a protocol library, provides toward them. It is not a claim that the SDK is compliant with anything: the EU AI Act places no obligation on this SDK, which is neither an AI system nor a provider (Regulation (EU) 2024/1689, Articles 3(1) and 3(3)), and Article 25(4) exempts free and open-source components from the value-chain duties, an exemption Regulation (EU) 2026/1744 kept. The obligations below belong to whoever builds an AI system or a product with this SDK. Each row says how much of one this SDK carries for them, with the test that proves it, and what is left. Not legal advice.

As of 2026-10-08. Every cited test is checked to exist, as a test, by `scripts/check_compliance_map.py` in CI; a row cannot outlive its evidence.

| Supported | Documented | Partial | Gap | Integrator's duty |
|---|---|---|---|---|
| 3 | 1 | 18 | 1 | 4 |

## EU AI Act — Regulation (EU) 2024/1689, as amended by Regulation (EU) 2026/1744

Source: <https://eur-lex.europa.eu/eli/reg/2024/1689/oj/eng> — read in the primary text.

Article text read on EUR-Lex on 2026-10-08. Application dates per the Commission's page and the amending regulation: Article 50 from 2 August 2026; Annex III high-risk obligations from 2 December 2027; Annex I high-risk obligations from 2 August 2028.

| ID | Provision | Requirement | What the SDK provides | Status | Evidence | Gap or reason |
|---|---|---|---|---|---|---|
| AIA-12-1 | Art. 12(1) | High-risk AI systems shall technically allow for the automatic recording of events (logs) over the lifetime of the system. | An append-only event log of every task's status and artifact events, durable in the SQLite and PostgreSQL stores and replayable by position (`Last-Event-ID`); an OpenTelemetry span for every RPC on every binding. | Partial | `events_round_trip_in_order_with_their_positions`<br>`appending_the_same_position_twice_leaves_one_row`<br>`subscribe_to_task_from_replays_from_the_offset_on_both_bindings`<br>[0012-event-log-and-resumption.md](../../docs/adr/0012-event-log-and-resumption.md) | The log records what happened to a task, not who caused it: no caller identity, authentication scheme or tenant per event, and nothing makes it tamper-evident. |
| AIA-12-2 | Art. 12(2)(a)–(c) | Logging shall enable the recording of events relevant for identifying risk situations or substantial modifications, for post-market monitoring (Art. 72) and for deployers' monitoring (Art. 26(5)). | Every state transition, including failed, rejected and canceled, is an event; a typed failure taxonomy; semantic-convention metrics (`rpc.server.call.duration`) and spans carrying the task state. | Partial | `sync_mode_working_to_canceled`<br>`the_executor_span_is_invoke_agent_without_content_by_default`<br>[0013-observability.md](../../docs/adr/0013-observability.md) | Telemetry is sampled and exported, not retained: there is no single record stream a deployer can keep for the period Art. 19 and 26(6) require. |
| AIA-13-3f | Art. 13(3)(f) | The instructions for use shall describe the mechanisms that allow deployers to properly collect, store and interpret the logs. | The observability chapter of the book and ADRs 0012 and 0013 describe the event log, its replay, the spans and the metrics. | Partial | [observability.md](../../book/src/deployment/observability.md)<br>[0012-event-log-and-resumption.md](../../docs/adr/0012-event-log-and-resumption.md) | No text a provider can lift into instructions for use that describes these records as an audit log: what each record holds, how long to keep it, how to verify it. |
| AIA-14-4d | Art. 14(4)(d) | Natural persons overseeing the system can decide not to use it, or disregard, override or reverse its output. | The `input-required` and `auth-required` states let an executor pause a task for a person and resume it on the same task id. | Partial | `input_required_continuation_reuses_task_id` | Nothing gates an output on a person's approval or records who approved what. |
| AIA-14-4e | Art. 14(4)(e) | Natural persons can intervene in or interrupt the system through a 'stop' button or similar procedure that brings it to a halt in a safe state. | `CancelTask` on every binding signals the executor's cancellation token; graceful shutdown cancels in-flight work and reports what ignored cancellation. | Partial | `cancel_task_signals_cancellation_token`<br>`shutdown_with_timeout_cancels_tokens`<br>[swarm-orchestration.md](../../docs/swarm-orchestration.md) | Cancelling a task does not reach the tasks it delegated to other agents (all children orphaned in examples/swarm, G1 in swarm-orchestration.md), and there is no tenant-wide or global stop. |
| AIA-15-5 | Art. 15(5) | High-risk AI systems shall be resilient against attempts by unauthorised third parties to alter their use, outputs or performance by exploiting system vulnerabilities. | Authentication (API key in constant time, bearer, JWT with a fixed algorithm list and HS256 secrets of at least 32 bytes), signed agent cards, an SSRF guard on push delivery, body and rate limits, refusal of tenants the stores cannot isolate, 13 fuzz targets. | Partial | `alg_none_is_rejected`<br>`algorithm_confusion_rejected`<br>`hs256_secret_shorter_than_32_bytes_fails_closed`<br>`verify_rejects_tampered_card`<br>`rejects_loopback_ipv4`<br>`the_default_store_refuses_a_client_named_tenant`<br>CI: cargo +nightly fuzz run | The model-level attacks the article names — data and model poisoning, adversarial examples — are the provider's to address; the SDK carries messages and offers interceptors as the hook for a content filter, but ships none. |
| AIA-19-26-6 | Art. 19(1), 26(6) | Providers and deployers keep the automatically generated logs for a period appropriate to the intended purpose, of at least six months, unless Union or national law provides otherwise. | The SQLite and PostgreSQL stores keep everything until told otherwise; `purge_expired` deletes only terminal tasks older than an age the operator sets. | Partial | `purges_terminal_tasks_past_the_age_and_nothing_else`<br>[0011-task-retention.md](../../docs/adr/0011-task-retention.md) | No retention preset that encodes a six-month floor, no legal hold, and the in-memory store's default one-hour TTL makes it unsuitable as a log of record. |
| AIA-50-1 | Art. 50(1) | Natural persons are informed that they are interacting with an AI system. | A2A carries agent-to-agent traffic; the agent card names the agent and describes it. | Integrator's duty | — | The person interacts with the integrator's interface, which this SDK never renders; the disclosure belongs there. |
| AIA-50-2 | Art. 50(2) | Synthetic audio, image, video or text outputs are marked in a machine-readable format and detectable as artificially generated. | Messages and artifacts carry arbitrary metadata, which the SDK passes through. | Gap | — | No defined marker that an artifact was generated by AI, and no signature binding such a marker to the artifact. Watermarking the content itself is the generator's, not the transport's. |
| AIA-72-2 | Art. 72(2) | The post-market monitoring system shall include an analysis of the interaction with other AI systems, where relevant. | W3C trace context: the client sends a `traceparent` naming its own span, and the server continues it under a configurable inbound policy, so a call chain across agents is one trace. | Partial | `the_traceparent_names_the_call_span`<br>`a_traceparent_header_reaches_the_call_context` | The cross-agent link lives in exported traces, which are sampled and expire; nothing persists it beside the task's own records. |
| AIA-73 | Art. 73 | Providers report serious incidents to market surveillance authorities. | Typed failures, error metrics and spans give a provider the signals to detect one. | Integrator's duty | — | Deciding that an incident is serious, and reporting it, is the provider's act. |

## Cyber Resilience Act — Regulation (EU) 2024/2847

Source: <https://eur-lex.europa.eu/eli/reg/2024/2847/oj/eng> — read in the primary text.

Article text read on EUR-Lex on 2026-10-08. Article 14 applies from 11 September 2026 and the rest from 11 December 2027 (Article 71(2)). Non-monetised open-source software is outside the Act (Recital 18); these rows serve the manufacturers who integrate it (Article 13(5)).

| ID | Provision | Requirement | What the SDK provides | Status | Evidence | Gap or reason |
|---|---|---|---|---|---|---|
| CRA-13-5 | Art. 13(5) | Manufacturers exercise due diligence when integrating components sourced from third parties, free and open-source software included. | Per-crate CycloneDX SBOMs and SLSA build-provenance attestations on every release; release tags signed and verified; OSV-Scanner and cargo-deny on every pull request; the OpenSSF Best Practices evidence. | Supported | CI: Generate CycloneDX SBOMs<br>CI: actions/attest-build-provenance<br>CI: scripts/verify_tag_signature.sh --self-test<br>CI: google/osv-scanner-action/.github/workflows/osv-scanner-reusable-pr.yml<br>[PROVENANCE.md](../../PROVENANCE.md)<br>[SECURITY.md](../../SECURITY.md)<br>[openssf-best-practices.md](../../docs/openssf-best-practices.md) | — |
| CRA-13-6 | Art. 13(6) | A manufacturer that identifies a vulnerability in a component reports it to whoever maintains the component. | SECURITY.md names the private reporting channels, accepts Article 13(6) reports and shared fixes on the same three-business-day acknowledgement, and defers to the reporter's statutory notification deadlines. | Documented | [SECURITY.md](../../SECURITY.md) | — |
| CRA-AI-II-1 | Annex I, Part II(1) | Manufacturers draw up a software bill of materials in a commonly used, machine-readable format covering at least the top-level dependencies. | A CycloneDX SBOM per published crate, attached to every GitHub release and attested. | Supported | CI: Generate CycloneDX SBOMs<br>CI: actions/attest-sbom | — |
| CRA-AI-I-2a | Annex I, Part I(2)(a) | Products are made available without known exploitable vulnerabilities. | cargo-deny advisories on every pull request and release; OSV-Scanner over all seven Cargo lockfiles; no published manifest admits a version with a RustSec advisory. | Supported | CI: EmbarkStudios/cargo-deny-action<br>CI: google/osv-scanner-action/.github/workflows/osv-scanner-reusable.yml<br>CI: scripts/check_advisory_floors.py | — |

## GDPR — Regulation (EU) 2016/679

Source: <https://eur-lex.europa.eu/eli/reg/2016/679/oj/eng> — read in the primary text.

| ID | Provision | Requirement | What the SDK provides | Status | Evidence | Gap or reason |
|---|---|---|---|---|---|---|
| GDPR-25-1 | Art. 25(1), 5(1)(c) | Data protection by design: measures such as pseudonymisation, implementing data minimisation. | Message content is kept out of spans unless the operator opts in; `CallContext` debug output names headers but never prints their values; the JWT validator, push configuration and token provider redact their secrets in debug output. | Partial | `the_executor_span_is_invoke_agent_without_content_by_default`<br>`debug_impls_render_type_and_redact_secrets` | The task store and event log keep full message content in plaintext, with no hook to pseudonymise or digest it before it is written. |
| GDPR-32-1a | Art. 32(1)(a) | Security of processing, including pseudonymisation and encryption of personal data. | The client speaks TLS through rustls with certificate validation; the gRPC listener terminates TLS and mutual TLS. | Partial | `tls_client_rejects_unknown_ca`<br>`https_endpoint_completes_a_tls_handshake_with_the_pinned_ca` | The JSON-RPC and HTTP+JSON listeners do not terminate TLS (a reverse proxy does), and nothing encrypts stored task data at rest. |
| GDPR-5-1e | Art. 5(1)(e), 17 | Storage limitation, and erasure on request. | `purge_expired` removes terminal tasks past an age, with their events and artifact journal, and expires idempotency keys. | Partial | `purges_terminal_tasks_past_the_age_and_nothing_else` | No erasure of one person's data on request; the store has no notion of a data subject. |

## OWASP Top 10 for Agentic Applications (2026)

Source: <https://genai.owasp.org/resource/owasp-top-10-for-agentic-applications-for-2026/> — **identifiers and wording from secondary sources; check against the published text before relying on them**.

Published 9 December 2025. The identifiers and names below agree across three secondary summaries; OWASP's own page carries the list only as a download.

| ID | Provision | Requirement | What the SDK provides | Status | Evidence | Gap or reason |
|---|---|---|---|---|---|---|
| ASI03 | ASI03 Identity and Privilege Abuse | Agents act under identities and delegated privileges that are scoped, attributable and not escalated. | Authenticated callers are named, and the name reaches the executor through the request context. | Partial | `a_labelled_api_key_names_its_caller`<br>`an_executor_sees_the_caller_identity_the_interceptor_established` | A delegated call carries the delegating agent's credential, not the originating principal, and nothing records the chain. |
| ASI04 | ASI04 Agentic Supply Chain Vulnerabilities | Agents, tools and schemas an agent imports are verified before they are trusted. | Agent cards can be signed (JWS, RFC 8785 canonical form) and verified. | Partial | `sign_and_verify_agent_card`<br>`verify_rejects_tampered_card` | ES256 only, and verification needs the caller to fetch the key: no JWKS resolution from the card's `jku`/`kid`. |
| ASI07 | ASI07 Insecure Inter-Agent Communication | Messages between agents are authenticated and protected against interception and forgery. | TLS on every client transport, mutual TLS on gRPC, authenticated calls, signed agent cards. | Partial | `tls_client_rejects_unknown_ca` | Messages and artifacts are not signed, so an intermediary that terminates TLS can alter them undetected; mutual TLS is gRPC-only. |
| ASI08 | ASI08 Cascading Failures | A failure in one agent or tool does not propagate unchecked through the agents that depend on it. | Per-caller rate limits, per-tenant concurrency limits, an executor timeout, bounded queues and stores. | Partial | `fast_path_rate_limit_exceeded`<br>`a_tenant_at_its_concurrency_limit_is_refused` | Cancellation does not cascade to delegated tasks, so a stopped parent leaves its children running. |
| ASI09 | ASI09 Human-Agent Trust Exploitation | Sensitive actions require explicit human confirmation rather than trust in confident output. | The `input-required` state can carry a request for confirmation. | Partial | `input_required_continuation_reuses_task_id` | No gate that refuses to complete a task until a person has confirmed it, and no record of the confirmation. |
| ASI10 | ASI10 Rogue Agents | Agents acting outside their intended behaviour are detected and stopped. | Spans and metrics for every call; cancellation of a task. | Partial | `cancel_task_signals_cancellation_token` | No kill switch that halts every task of a tenant or of the server at once. |
| ASI-MODEL | ASI01, ASI02, ASI05, ASI06 | Goal hijack, tool misuse, unexpected code execution, memory and context poisoning. | Server and client interceptors are where an integrator's filter or policy runs, before and after every call. | Integrator's duty | — | These attack the model, its tools and its memory, which live in the integrator's executor, not in the transport. |

## ISO/IEC 42001:2023 — AI management systems

Source: <https://www.iso.org/standard/81230.html> — **identifiers and wording from secondary sources; check against the published text before relying on them**.

A management-system standard: an organisation is certified against it, a library is not. Annex A control numbers are taken from secondary sources, which disagree on some of them; check against the published standard.

| ID | Provision | Requirement | What the SDK provides | Status | Evidence | Gap or reason |
|---|---|---|---|---|---|---|
| ISO42001-A.6.2.8 | Annex A.6.2.8, AI system recording of event logs (numbering unverified) | The organisation determines at which life-cycle phases event logging is enabled, at minimum while the AI system is in use. | As for AIA-12-1: the event log and per-call spans. | Partial | `events_round_trip_in_order_with_their_positions` | As for AIA-12-1: no actor identity, no tamper evidence. |
| ISO42001-AIMS | Clauses 4–10 | An AI management system: policy, roles, risk assessment, impact assessment, monitoring, improvement. | Nothing beyond the records above. | Integrator's duty | — | A management system is an organisation's; a library cannot hold one. |

## Where each cited test lives

- `events_round_trip_in_order_with_their_positions` — `crates/a2a-protocol-server/src/store/sqlite_store/event_log_tests.rs` (AIA-12-1)
- `appending_the_same_position_twice_leaves_one_row` — `crates/a2a-protocol-server/src/store/sqlite_store/event_log_tests.rs` (AIA-12-1)
- `subscribe_to_task_from_replays_from_the_offset_on_both_bindings` — `crates/a2a-protocol-client/tests/resume_e2e_tests.rs` (AIA-12-1)
- `sync_mode_working_to_canceled` — `crates/a2a-protocol-server/tests/event_processing_tests/state_transitions.rs` (AIA-12-2)
- `the_executor_span_is_invoke_agent_without_content_by_default` — `crates/a2a-protocol-server/tests/telemetry_export/agent_spans.rs` (AIA-12-2)
- `input_required_continuation_reuses_task_id` — `crates/a2a-protocol-server/src/handler/messaging/tests.rs` (AIA-14-4d)
- `cancel_task_signals_cancellation_token` — `crates/a2a-protocol-server/tests/edge_case_tests/handler_basics.rs` (AIA-14-4e)
- `shutdown_with_timeout_cancels_tokens` — `crates/a2a-protocol-server/src/handler/shutdown/tests/mod.rs` (AIA-14-4e)
- `alg_none_is_rejected` — `crates/a2a-protocol-server/src/auth/jwt.rs` (AIA-15-5)
- `algorithm_confusion_rejected` — `crates/a2a-protocol-server/src/auth/jwt.rs` (AIA-15-5)
- `hs256_secret_shorter_than_32_bytes_fails_closed` — `crates/a2a-protocol-server/src/auth/jwt.rs` (AIA-15-5)
- `verify_rejects_tampered_card` — `crates/a2a-protocol-types/src/signing.rs` (AIA-15-5)
- `rejects_loopback_ipv4` — `crates/a2a-protocol-server/src/push/sender.rs` (AIA-15-5)
- `the_default_store_refuses_a_client_named_tenant` — `crates/a2a-protocol-server/src/handler/tenant_isolation_tests.rs` (AIA-15-5)
- `purges_terminal_tasks_past_the_age_and_nothing_else` — `crates/a2a-protocol-server/src/store/sqlite_store/retention_tests.rs` (AIA-19-26-6)
- `the_traceparent_names_the_call_span` — `crates/a2a-protocol-client/src/call_span/tests.rs` (AIA-72-2)
- `a_traceparent_header_reaches_the_call_context` — `crates/a2a-protocol-server/src/handler/helpers.rs` (AIA-72-2)
- `the_executor_span_is_invoke_agent_without_content_by_default` — `crates/a2a-protocol-server/tests/telemetry_export/agent_spans.rs` (GDPR-25-1)
- `debug_impls_render_type_and_redact_secrets` — `crates/a2a-protocol-server/src/auth/jwt.rs` (GDPR-25-1)
- `tls_client_rejects_unknown_ca` — `crates/a2a-protocol-client/tests/tls_integration_tests.rs` (GDPR-32-1a)
- `https_endpoint_completes_a_tls_handshake_with_the_pinned_ca` — `crates/a2a-protocol-client/tests/grpc_tls_apply_tests.rs` (GDPR-32-1a)
- `purges_terminal_tasks_past_the_age_and_nothing_else` — `crates/a2a-protocol-server/src/store/sqlite_store/retention_tests.rs` (GDPR-5-1e)
- `a_labelled_api_key_names_its_caller` — `crates/a2a-protocol-server/src/auth/identity_tests.rs` (ASI03)
- `an_executor_sees_the_caller_identity_the_interceptor_established` — `crates/a2a-protocol-server/tests/request_context_tests.rs` (ASI03)
- `sign_and_verify_agent_card` — `crates/a2a-protocol-types/src/signing.rs` (ASI04)
- `verify_rejects_tampered_card` — `crates/a2a-protocol-types/src/signing.rs` (ASI04)
- `tls_client_rejects_unknown_ca` — `crates/a2a-protocol-client/tests/tls_integration_tests.rs` (ASI07)
- `fast_path_rate_limit_exceeded` — `crates/a2a-protocol-server/src/rate_limit/tests.rs` (ASI08)
- `a_tenant_at_its_concurrency_limit_is_refused` — `crates/a2a-protocol-server/src/handler/tenant_limits_tests.rs` (ASI08)
- `input_required_continuation_reuses_task_id` — `crates/a2a-protocol-server/src/handler/messaging/tests.rs` (ASI09)
- `cancel_task_signals_cancellation_token` — `crates/a2a-protocol-server/tests/edge_case_tests/handler_basics.rs` (ASI10)
- `events_round_trip_in_order_with_their_positions` — `crates/a2a-protocol-server/src/store/sqlite_store/event_log_tests.rs` (ISO42001-A.6.2.8)
