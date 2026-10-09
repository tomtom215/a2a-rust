<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->
<!-- GENERATED from controls.toml by scripts/check_compliance_map.py --write. Do not edit. -->

# Regulatory control map

This is a map from regulatory and standards provisions to what a2a-rust, a protocol library, provides toward them. It is not a claim that the SDK is compliant with anything: the EU AI Act places no obligation on this SDK, which is neither an AI system nor a provider (Regulation (EU) 2024/1689, Articles 3(1) and 3(3)), and Article 25(4) exempts free and open-source components from the value-chain duties, an exemption Regulation (EU) 2026/1744 kept. The obligations below belong to whoever builds an AI system or a product with this SDK. Each row says how much of one this SDK carries for them, with the test that proves it, and what is left. Not legal advice.

As of 2026-10-08. Every cited test is checked to exist, as a test, by `scripts/check_compliance_map.py` in CI; a row cannot outlive its evidence.

| Supported | Documented | Partial | Gap | Integrator's duty |
|---|---|---|---|---|
| 8 | 2 | 13 | 0 | 4 |

## EU AI Act — Regulation (EU) 2024/1689, as amended by Regulation (EU) 2026/1744

Source: <https://eur-lex.europa.eu/eli/reg/2024/1689/oj/eng> — read in the primary text.

Article text read on EUR-Lex on 2026-10-08. Application dates per the Commission's page and the amending regulation: Article 50 from 2 August 2026; Annex III high-risk obligations from 2 December 2027; Annex I high-risk obligations from 2 August 2028.

| ID | Provision | Requirement | What the SDK provides | Status | Evidence | Gap or reason |
|---|---|---|---|---|---|---|
| AIA-12-1 | Art. 12(1) | High-risk AI systems shall technically allow for the automatic recording of events (logs) over the lifetime of the system. | With the `audit` feature, every call (refused ones included), every run of a task, every event an agent emits and every cancel request is recorded with the authenticated caller, the authentication scheme, the tenant and the trace, in a SHA-256 hash chain per tenant with signed checkpoints that make edits, deletions, reordering and truncation detectable (ADR 0015). The task event log and per-call spans remain beside it. | Supported | `every_call_and_event_is_recorded_and_attributable`<br>`each_tenant_gets_its_own_chain`<br>`a_resealed_edit_breaks_the_next_link`<br>`truncating_the_tail_past_a_checkpoint_is_found`<br>`concurrent_appends_to_one_chain_never_fork_it`<br>[0015-audit-trail.md](../../docs/adr/0015-audit-trail.md)<br>[0012-event-log-and-resumption.md](../../docs/adr/0012-event-log-and-resumption.md) | — |
| AIA-12-2 | Art. 12(2)(a)–(c) | Logging shall enable the recording of events relevant for identifying risk situations or substantial modifications, for post-market monitoring (Art. 72) and for deployers' monitoring (Art. 26(5)). | Every state transition — including failed, rejected and canceled — is an audit record naming the run, and so the caller, it belongs to; failed and refused calls are recorded with a bounded error name; records are retained rather than sampled. Metrics and spans carry the same states for live monitoring. | Supported | `every_call_and_event_is_recorded_and_attributable`<br>`sync_mode_working_to_canceled`<br>[0015-audit-trail.md](../../docs/adr/0015-audit-trail.md)<br>[0013-observability.md](../../docs/adr/0013-observability.md) | — |
| AIA-13-3f | Art. 13(3)(f) | The instructions for use shall describe the mechanisms that allow deployers to properly collect, store and interpret the logs. | The book's Audit Trail chapter says what each record holds, how to configure the store and the checkpoint key, how to verify a chain and an export, how to follow one request across agents, and how retention and legal holds behave; ADR 0015 gives the threat model and limits. Text a provider can adapt into its instructions for use. | Documented | [audit.md](../../book/src/deployment/audit.md)<br>[0015-audit-trail.md](../../docs/adr/0015-audit-trail.md) | — |
| AIA-14-4d | Art. 14(4)(d) | Natural persons overseeing the system can decide not to use it, or disregard, override or reverse its output. | The approval extension: an executor asks with `EventEmitter::request_approval`, naming the action by the SHA-256 digest of its canonical JSON, and the task waits at `input-required`. With an `ApprovalGate`, the answer reaches the executor (`RequestContext::approval`) only if it answers the pending request, echoes its digest, and comes from an authenticated approver the gate allows, by default not the caller whose run asked. With the audit trail, each admitted decision is an `approval` record naming the approver. | Partial | `input_required_continuation_reuses_task_id`<br>`an_approval_flows_through_the_gate_to_the_executor`<br>`a_decision_for_another_request_or_action_is_refused`<br>`who_may_approve`<br>`an_admitted_decision_is_audited_with_the_approver` | Which actions wait for a person is the executor's decision: the gate checks and records decisions, and cannot make an executor ask, or stop one that acts without asking. |
| AIA-14-4e | Art. 14(4)(e) | Natural persons can intervene in or interrupt the system through a 'stop' button or similar procedure that brings it to a halt in a safe state. | `CancelTask` on every binding signals the executor's cancellation token. `RequestHandler::halt` stops one tenant or the whole server: new sends are refused (HTTP 503, gRPC UNAVAILABLE) and every running task's token fires, until `resume`; each is an audit record naming the operator. A `Delegation` in the client cancels the tasks a task delegated to other agents when it is cancelled, so a stop reaches the whole tree. Graceful shutdown cancels in-flight work and reports what ignored cancellation. | Partial | `cancel_task_signals_cancellation_token`<br>`shutdown_with_timeout_cancels_tokens`<br>`halting_a_tenant_cancels_its_tasks_and_refuses_its_sends`<br>`no_send_racing_a_halt_is_left_running`<br>`a_halted_server_refuses_sends_on_both_http_bindings`<br>`the_parent_cancelling_cancels_the_child`<br>`aborting_the_parent_task_cancels_the_child`<br>[swarm-orchestration.md](../../docs/swarm-orchestration.md)<br>[oversight.md](../../book/src/deployment/oversight.md) | A halt holds in one process and is not persisted: each replica is halted separately, and a restart starts unhalted. An executor that ignores its token keeps running. A parent that crashes leaves its delegated children running (G1-B in swarm-orchestration.md). |
| AIA-15-5 | Art. 15(5) | High-risk AI systems shall be resilient against attempts by unauthorised third parties to alter their use, outputs or performance by exploiting system vulnerabilities. | Authentication (API key in constant time, bearer, JWT with a fixed algorithm list and HS256 secrets of at least 32 bytes, wiped from memory on drop), signed agent cards (ES256 or EdDSA, verified against a JWK Set), signed messages and artifacts, an SSRF guard on push delivery, body and rate limits, refusal of tenants the stores cannot isolate, 13 fuzz targets. | Partial | `alg_none_is_rejected`<br>`algorithm_confusion_rejected`<br>`hs256_secret_shorter_than_32_bytes_fails_closed`<br>`verify_rejects_tampered_card`<br>`rejects_loopback_ipv4`<br>`the_default_store_refuses_a_client_named_tenant`<br>CI: cargo +nightly fuzz run | The model-level attacks the article names — data and model poisoning, adversarial examples — are the provider's to address; the SDK carries messages and offers interceptors as the hook for a content filter, but ships none. |
| AIA-19-26-6 | Art. 19(1), 26(6) | Providers and deployers keep the automatically generated logs for a period appropriate to the intended purpose, of at least six months, unless Union or national law provides otherwise. | Audit records are kept until purged, and purge deletes nothing younger than the configured floor (`AuditRetention::six_months()` is 184 days; a shorter floor must be asked for by name), never a chain's newest record, and nothing under a legal hold. A signed anchor keeps the remaining chain verifiable. | Supported | `purge_deletes_only_what_is_past_the_floor_and_the_rest_still_verifies`<br>`a_legal_hold_stops_purge_until_released`<br>`six_months_is_the_longest_six_calendar_months`<br>`a_signed_anchor_lets_a_purged_chain_verify`<br>[0015-audit-trail.md](../../docs/adr/0015-audit-trail.md)<br>[0011-task-retention.md](../../docs/adr/0011-task-retention.md) | — |
| AIA-50-1 | Art. 50(1) | Natural persons are informed that they are interacting with an AI system. | A2A carries agent-to-agent traffic; the agent card names the agent and describes it. | Integrator's duty | — | The person interacts with the integrator's interface, which this SDK never renders; the disclosure belongs there. |
| AIA-50-2 | Art. 50(2) | Synthetic audio, image, video or text outputs are marked in a machine-readable format and detectable as artificially generated. | The provenance extension: `mark_ai_generated` marks a message or artifact as AI-generated, naming the generator, in metadata under a declared extension URI; `sign_content` signs the whole content with ES256 or EdDSA, marker included, so the marker cannot be stripped without breaking the signature; `verify_content` checks it against a JWK Set. Signed content survives a round trip through a server, and file bytes (a C2PA manifest inside them included) pass through unchanged. | Partial | `the_marker_is_machine_readable_in_the_declared_shape`<br>`any_change_after_signing_breaks_it`<br>`bytes_pass_through_untouched_and_a_signed_artifact_still_verifies` | The agent must call the marker; the SDK cannot tell generated content from any other. The marker is in A2A metadata, so it does not travel with a file once saved: marking inside the content itself (a watermark, a C2PA manifest) is the generator's. |
| AIA-72-2 | Art. 72(2) | The post-market monitoring system shall include an analysis of the interaction with other AI systems, where relevant. | W3C trace context end to end: the client sends a `traceparent` naming its own span and the server continues it, and every audit record keeps the trace and span id, so one request's records can be joined across every audited agent it reached, long after the trace backend has expired the spans. | Supported | `every_call_and_event_is_recorded_and_attributable`<br>`the_traceparent_names_the_call_span`<br>`a_traceparent_header_reaches_the_call_context` | — |
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
| GDPR-25-1 | Art. 25(1), 5(1)(c) | Data protection by design: measures such as pseudonymisation, implementing data minimisation. | Audit records hold content only as SHA-256 digests; message content is kept out of spans unless the operator opts in; `CallContext` debug output names headers but never prints their values; the JWT validator, push configuration and token provider redact their secrets in debug output. | Partial | `every_call_and_event_is_recorded_and_attributable`<br>`the_executor_span_is_invoke_agent_without_content_by_default`<br>`debug_impls_render_type_and_redact_secrets` | The task store and event log keep full message content in plaintext, with no hook to pseudonymise it before it is written; audit records name the caller's subject, which is personal data in most deployments. |
| GDPR-32-1a | Art. 32(1)(a) | Security of processing, including pseudonymisation and encryption of personal data. | The client speaks TLS through rustls with certificate validation; the gRPC listener terminates TLS and mutual TLS. | Partial | `tls_client_rejects_unknown_ca`<br>`https_endpoint_completes_a_tls_handshake_with_the_pinned_ca` | The JSON-RPC and HTTP+JSON listeners do not terminate TLS (a reverse proxy does), and nothing encrypts stored task data at rest. |
| GDPR-5-1e | Art. 5(1)(e), 17 | Storage limitation, and erasure on request. | `purge_expired` removes terminal tasks past an age, with their events and artifact journal, and expires idempotency keys; audit purge removes records past the configured floor. | Partial | `purges_terminal_tasks_past_the_age_and_nothing_else`<br>`purge_deletes_only_what_is_past_the_floor_and_the_rest_still_verifies` | No erasure of one person's data on request; neither store has a notion of a data subject, and deleting one record from the middle of an audit chain would break it by design. |

## OWASP Top 10 for Agentic Applications (2026)

Source: <https://genai.owasp.org/resource/owasp-top-10-for-agentic-applications-for-2026/> — **identifiers and wording from secondary sources; check against the published text before relying on them**.

Published 9 December 2025. The identifiers and names below agree across three secondary summaries; OWASP's own page carries the list only as a download.

| ID | Provision | Requirement | What the SDK provides | Status | Evidence | Gap or reason |
|---|---|---|---|---|---|---|
| ASI03 | ASI03 Identity and Privilege Abuse | Agents act under identities and delegated privileges that are scoped, attributable and not escalated. | Authenticated callers are named, and the name reaches the executor through the request context. | Partial | `a_labelled_api_key_names_its_caller`<br>`an_executor_sees_the_caller_identity_the_interceptor_established` | A delegated call carries the delegating agent's credential, not the originating principal, and nothing records the chain. |
| ASI04 | ASI04 Agentic Supply Chain Vulnerabilities | Agents, tools and schemas an agent imports are verified before they are trusted. | Agent cards can be signed (JWS over RFC 8785 canonical form, ES256 or EdDSA) and verified against a raw or SPKI key, or against a JWK Set by `kid`, trying every signature the card carries (key rotation). The client fetches a JWK Set from a URL the caller trusts, over HTTPS, bounded in size and time; removing a key from the set revokes it. | Partial | `sign_and_verify_agent_card`<br>`verify_rejects_tampered_card`<br>`a_jwks_verifies_by_kid_and_alg`<br>`removing_a_key_from_the_set_revokes_it`<br>`the_rfc_8037_ed25519_example_verifies`<br>`a_card_verifies_under_a_fetched_set` | Which key set to trust is the caller's decision: the SDK never follows a `jku` taken from the card itself, and has no X.509 chain or key-expiry check. Tools and schemas an agent imports outside A2A are out of scope. |
| ASI07 | ASI07 Insecure Inter-Agent Communication | Messages between agents are authenticated and protected against interception and forgery. | TLS on every client transport, mutual TLS on gRPC, authenticated calls, signed agent cards, and signed messages and artifacts (the provenance extension) that a receiver can verify end to end, across intermediaries. | Partial | `tls_client_rejects_unknown_ca`<br>`a_signed_message_verifies_under_either_algorithm` | Signing messages and artifacts (the provenance extension) is opt-in, and verifying them is the receiver's call; unsigned content can still be altered by an intermediary that terminates TLS. Mutual TLS is gRPC-only. |
| ASI08 | ASI08 Cascading Failures | A failure in one agent or tool does not propagate unchecked through the agents that depend on it. | Per-caller rate limits, per-tenant concurrency limits, an executor timeout, bounded queues and stores. A `Delegation` cancels a task's children on other agents when the task is cancelled, its stream to a child is lost, or its handle is dropped. | Partial | `fast_path_rate_limit_exceeded`<br>`a_tenant_at_its_concurrency_limit_is_refused`<br>`a_lost_stream_cancels_the_child`<br>`dropping_an_unsettled_handle_cancels_the_child` | A parent that crashes leaves its children running until they finish on their own (G1-B, a lease, is not built). |
| ASI09 | ASI09 Human-Agent Trust Exploitation | Sensitive actions require explicit human confirmation rather than trust in confident output. | The approval extension binds a person's confirmation to the digest of the exact action, and the `ApprovalGate` refuses a confirmation of anything else, from anyone not allowed, or from the caller who asked; each admitted confirmation is an audit record. | Partial | `a_decision_for_another_request_or_action_is_refused`<br>`who_may_approve`<br>`without_a_gate_a_decision_is_never_reported_as_checked` | Which actions are sensitive enough to ask about is the executor's decision; the SDK cannot make it ask. |
| ASI10 | ASI10 Rogue Agents | Agents acting outside their intended behaviour are detected and stopped. | Spans and metrics for every call; cancellation of a task; `RequestHandler::halt`, which stops every task of a tenant or of the server and refuses new ones until resumed. | Partial | `cancel_task_signals_cancellation_token`<br>`halting_all_cancels_every_tenant` | Nothing detects a rogue agent: deciding to halt is a person's or an integrator's monitor's. A halt holds in one process and does not survive a restart. |
| ASI-MODEL | ASI01, ASI02, ASI05, ASI06 | Goal hijack, tool misuse, unexpected code execution, memory and context poisoning. | Server and client interceptors are where an integrator's filter or policy runs, before and after every call. | Integrator's duty | — | These attack the model, its tools and its memory, which live in the integrator's executor, not in the transport. |

## ISO/IEC 42001:2023 — AI management systems

Source: <https://www.iso.org/standard/81230.html> — **identifiers and wording from secondary sources; check against the published text before relying on them**.

A management-system standard: an organisation is certified against it, a library is not. Annex A control numbers are taken from secondary sources, which disagree on some of them; check against the published standard.

| ID | Provision | Requirement | What the SDK provides | Status | Evidence | Gap or reason |
|---|---|---|---|---|---|---|
| ISO42001-A.6.2.8 | Annex A.6.2.8, AI system recording of event logs (numbering unverified) | The organisation determines at which life-cycle phases event logging is enabled, at minimum while the AI system is in use. | As for AIA-12-1: the audit trail, on whenever the handler is built with it. | Supported | `every_call_and_event_is_recorded_and_attributable` | — |
| ISO42001-AIMS | Clauses 4–10 | An AI management system: policy, roles, risk assessment, impact assessment, monitoring, improvement. | Nothing beyond the records above. | Integrator's duty | — | A management system is an organisation's; a library cannot hold one. |

## Where each cited test lives

- `every_call_and_event_is_recorded_and_attributable` — `crates/a2a-protocol-server/tests/audit_trail.rs` (AIA-12-1)
- `each_tenant_gets_its_own_chain` — `crates/a2a-protocol-server/tests/audit_trail.rs` (AIA-12-1)
- `a_resealed_edit_breaks_the_next_link` — `crates/a2a-protocol-types/src/audit/tests.rs` (AIA-12-1)
- `truncating_the_tail_past_a_checkpoint_is_found` — `crates/a2a-protocol-types/src/audit/tests.rs` (AIA-12-1)
- `concurrent_appends_to_one_chain_never_fork_it` — `crates/a2a-protocol-server/src/audit/tests.rs` (AIA-12-1)
- `every_call_and_event_is_recorded_and_attributable` — `crates/a2a-protocol-server/tests/audit_trail.rs` (AIA-12-2)
- `sync_mode_working_to_canceled` — `crates/a2a-protocol-server/tests/event_processing_tests/state_transitions.rs` (AIA-12-2)
- `input_required_continuation_reuses_task_id` — `crates/a2a-protocol-server/src/handler/messaging/tests.rs` (AIA-14-4d)
- `an_approval_flows_through_the_gate_to_the_executor` — `crates/a2a-protocol-server/src/approval/tests.rs` (AIA-14-4d)
- `a_decision_for_another_request_or_action_is_refused` — `crates/a2a-protocol-server/src/approval/tests.rs` (AIA-14-4d)
- `who_may_approve` — `crates/a2a-protocol-server/src/approval/tests.rs` (AIA-14-4d)
- `an_admitted_decision_is_audited_with_the_approver` — `crates/a2a-protocol-server/src/approval/tests.rs` (AIA-14-4d)
- `cancel_task_signals_cancellation_token` — `crates/a2a-protocol-server/tests/edge_case_tests/handler_basics.rs` (AIA-14-4e)
- `shutdown_with_timeout_cancels_tokens` — `crates/a2a-protocol-server/src/handler/shutdown/tests/mod.rs` (AIA-14-4e)
- `halting_a_tenant_cancels_its_tasks_and_refuses_its_sends` — `crates/a2a-protocol-server/src/handler/halt/tests.rs` (AIA-14-4e)
- `no_send_racing_a_halt_is_left_running` — `crates/a2a-protocol-server/src/handler/halt/tests.rs` (AIA-14-4e)
- `a_halted_server_refuses_sends_on_both_http_bindings` — `crates/a2a-protocol-server/tests/halt_e2e.rs` (AIA-14-4e)
- `the_parent_cancelling_cancels_the_child` — `crates/a2a-protocol-client/tests/delegation_tests.rs` (AIA-14-4e)
- `aborting_the_parent_task_cancels_the_child` — `crates/a2a-protocol-client/tests/delegation_tests.rs` (AIA-14-4e)
- `alg_none_is_rejected` — `crates/a2a-protocol-server/src/auth/jwt.rs` (AIA-15-5)
- `algorithm_confusion_rejected` — `crates/a2a-protocol-server/src/auth/jwt.rs` (AIA-15-5)
- `hs256_secret_shorter_than_32_bytes_fails_closed` — `crates/a2a-protocol-server/src/auth/jwt.rs` (AIA-15-5)
- `verify_rejects_tampered_card` — `crates/a2a-protocol-types/src/signing.rs` (AIA-15-5)
- `rejects_loopback_ipv4` — `crates/a2a-protocol-server/src/push/sender.rs` (AIA-15-5)
- `the_default_store_refuses_a_client_named_tenant` — `crates/a2a-protocol-server/src/handler/tenant_isolation_tests.rs` (AIA-15-5)
- `purge_deletes_only_what_is_past_the_floor_and_the_rest_still_verifies` — `crates/a2a-protocol-server/src/audit/tests.rs` (AIA-19-26-6)
- `a_legal_hold_stops_purge_until_released` — `crates/a2a-protocol-server/src/audit/tests.rs` (AIA-19-26-6)
- `six_months_is_the_longest_six_calendar_months` — `crates/a2a-protocol-server/src/audit/tests.rs` (AIA-19-26-6)
- `a_signed_anchor_lets_a_purged_chain_verify` — `crates/a2a-protocol-types/src/audit/tests.rs` (AIA-19-26-6)
- `the_marker_is_machine_readable_in_the_declared_shape` — `crates/a2a-protocol-types/src/provenance/tests.rs` (AIA-50-2)
- `any_change_after_signing_breaks_it` — `crates/a2a-protocol-types/src/provenance/sign/tests.rs` (AIA-50-2)
- `bytes_pass_through_untouched_and_a_signed_artifact_still_verifies` — `crates/a2a-protocol-server/tests/content_provenance_e2e.rs` (AIA-50-2)
- `every_call_and_event_is_recorded_and_attributable` — `crates/a2a-protocol-server/tests/audit_trail.rs` (AIA-72-2)
- `the_traceparent_names_the_call_span` — `crates/a2a-protocol-client/src/call_span/tests.rs` (AIA-72-2)
- `a_traceparent_header_reaches_the_call_context` — `crates/a2a-protocol-server/src/handler/helpers.rs` (AIA-72-2)
- `every_call_and_event_is_recorded_and_attributable` — `crates/a2a-protocol-server/tests/audit_trail.rs` (GDPR-25-1)
- `the_executor_span_is_invoke_agent_without_content_by_default` — `crates/a2a-protocol-server/tests/telemetry_export/agent_spans.rs` (GDPR-25-1)
- `debug_impls_render_type_and_redact_secrets` — `crates/a2a-protocol-server/src/auth/jwt.rs` (GDPR-25-1)
- `tls_client_rejects_unknown_ca` — `crates/a2a-protocol-client/tests/tls_integration_tests.rs` (GDPR-32-1a)
- `https_endpoint_completes_a_tls_handshake_with_the_pinned_ca` — `crates/a2a-protocol-client/tests/grpc_tls_apply_tests.rs` (GDPR-32-1a)
- `purges_terminal_tasks_past_the_age_and_nothing_else` — `crates/a2a-protocol-server/src/store/sqlite_store/retention_tests.rs` (GDPR-5-1e)
- `purge_deletes_only_what_is_past_the_floor_and_the_rest_still_verifies` — `crates/a2a-protocol-server/src/audit/tests.rs` (GDPR-5-1e)
- `a_labelled_api_key_names_its_caller` — `crates/a2a-protocol-server/src/auth/identity_tests.rs` (ASI03)
- `an_executor_sees_the_caller_identity_the_interceptor_established` — `crates/a2a-protocol-server/tests/request_context_tests.rs` (ASI03)
- `sign_and_verify_agent_card` — `crates/a2a-protocol-types/src/signing.rs` (ASI04)
- `verify_rejects_tampered_card` — `crates/a2a-protocol-types/src/signing.rs` (ASI04)
- `a_jwks_verifies_by_kid_and_alg` — `crates/a2a-protocol-types/src/signing/keys/tests.rs` (ASI04)
- `removing_a_key_from_the_set_revokes_it` — `crates/a2a-protocol-types/src/signing/keys/tests.rs` (ASI04)
- `the_rfc_8037_ed25519_example_verifies` — `crates/a2a-protocol-types/src/signing/keys/tests.rs` (ASI04)
- `a_card_verifies_under_a_fetched_set` — `crates/a2a-protocol-client/tests/jwks_fetch_tests.rs` (ASI04)
- `tls_client_rejects_unknown_ca` — `crates/a2a-protocol-client/tests/tls_integration_tests.rs` (ASI07)
- `a_signed_message_verifies_under_either_algorithm` — `crates/a2a-protocol-types/src/provenance/sign/tests.rs` (ASI07)
- `fast_path_rate_limit_exceeded` — `crates/a2a-protocol-server/src/rate_limit/tests.rs` (ASI08)
- `a_tenant_at_its_concurrency_limit_is_refused` — `crates/a2a-protocol-server/src/handler/tenant_limits_tests.rs` (ASI08)
- `a_lost_stream_cancels_the_child` — `crates/a2a-protocol-client/tests/delegation_tests.rs` (ASI08)
- `dropping_an_unsettled_handle_cancels_the_child` — `crates/a2a-protocol-client/tests/delegation_tests.rs` (ASI08)
- `a_decision_for_another_request_or_action_is_refused` — `crates/a2a-protocol-server/src/approval/tests.rs` (ASI09)
- `who_may_approve` — `crates/a2a-protocol-server/src/approval/tests.rs` (ASI09)
- `without_a_gate_a_decision_is_never_reported_as_checked` — `crates/a2a-protocol-server/src/approval/tests.rs` (ASI09)
- `cancel_task_signals_cancellation_token` — `crates/a2a-protocol-server/tests/edge_case_tests/handler_basics.rs` (ASI10)
- `halting_all_cancels_every_tenant` — `crates/a2a-protocol-server/src/handler/halt/tests.rs` (ASI10)
- `every_call_and_event_is_recorded_and_attributable` — `crates/a2a-protocol-server/tests/audit_trail.rs` (ISO42001-A.6.2.8)
