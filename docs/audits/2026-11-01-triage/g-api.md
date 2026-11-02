<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Triage of slice g-api (G-04 to G-23, API ergonomics)

Baseline: `main` at `4a8243c66df5ae07a54a69168ab419ac609b06e9` (checked). Everything
below comes from reading the code. Nothing was compiled or run (the brief forbids
cargo). Full evidence is in `g-api.json`.

## Counts (19 rows)

| Verdict | Rows |
|---|---|
| confirmed | 12: G-04, G-05, G-08, G-12, G-13, G-16, G-17, G-18, G-19, G-21, G-22, G-23 |
| needs_decision | 6: G-06, G-07, G-09, G-10, G-14, G-15 |
| partly_fixed | 1: G-20 (blocking-send timeout fixed in 57f034e, with tests) |
| already_fixed / not_a_defect / duplicate / cannot_check | 0 |

Three of G-23's five sub-items hold: missing `Debug` (three types now, not two),
`mutants` as a normal dependency (now in two crates), and the cosmetic `production`
field/constructor pair. The other two were never defects, so the row is marked
`confirmed` even though rule 6 would call a mixed row `partly_fixed`. G-10's
deploy-agent sub-item was fixed in 57f034e, but five other examples still build
cards as struct literals.

New findings: 4. NEW-g-api-1 is medium; the other three are low.

## What 0.16.0 allows (STABILITY.md sections 2, 3 and 5)

- 0.15.0 shipped on 2026-10-25, so a 0.16.0 in November is this month's one breaking
  minor (section 3.2). The next breaking window is December at the earliest.
- Every crate already reads `0.16.0`. `cargo-semver-checks` therefore calls every
  break a "major change" and passes, so **nothing in this slice is blocked by CI
  today**. Once 0.16.0 is published, every API break below is blocked until 0.17.0.
- Each deliberate break still needs a `### Breaking Changes` entry and an
  upgrade-guide section. The 0.15-to-0.16 guide covers G-03 and G-11 and none of the
  rows here.
- Section 3.1 (deprecate first) means renames and signature changes should add the
  new item and deprecate the old one in 0.16, then remove the old one in 0.17.

The rows fall into three groups:

- **Changes that can only ship in 0.16.0** (no deprecation path):
  - G-10: `#[non_exhaustive]` on the wire structs.
  - G-18: the type of `RequestContext.context_id`. cargo-semver-checks may not detect
    a field-type change, so it has to be listed by hand.
  - G-06: the server's default features.
  - G-07: a `streaming: true` default in `AgentCard::new`. This is a wire change.
  - G-14: what `Telemetry::init()` exports to.
  - G-17: `impl Into<Arc<_>>` on the dispatcher constructors. `GrpcDispatcher::new`
    and `WebSocketDispatcher::new` would lose `const`, which semver-checks flags.
- **Can ship any time as additions:**
  - G-08: a new macro arm.
  - G-12: `impl Stream`. futures-core is already in the dependency graph through
    hyper and http-body-util.
  - G-13: a message-id generator.
  - G-16: `EventEmitter` helpers.
  - G-21: a warning.
  - G-22: accessors on `SendMessageResponse`.
  - G-23: `Debug` impls.
  - The additive half of G-07: a minimal constructor and a typed check.
  - The additive half of G-09: `A2aRouter` as a `Dispatcher`, and `ServeConfig` on
    gRPC and WebSocket.
  - G-14: `#[must_use]` and deprecating `init_otlp_pipeline`.
- **Behaviour fixes with no API change:** G-04 (`build_grpc` uses the card's gRPC
  interface), G-19 (an explicit tenant wins), G-20 (timeout messages).
- **Renames (G-15):** add the new names with deprecated aliases in 0.16 and remove
  the old ones in 0.17. The SDK prelude must re-export both in between.

## The five rows that matter most

1. **G-10.** 40 of the 47 generated wire structs are exhaustive, but STABILITY.md
   section 4 (lines 127-129) says protocol structs are `#[non_exhaustive]`. That makes
   every proto field the A2A spec adds a breaking change. 0.16.0 is the cheapest moment
   to mark them: about 1,000 struct literals in the workspace (rough grep count), and
   several types first need constructors. If that does not fit, correct the
   STABILITY.md claim now.
2. **G-04.** `ClientBuilder::from_card(card)?.build_grpc()` still dials the card's
   first interface. For a card that lists JSON-RPC before gRPC, it speaks gRPC to the
   JSON-RPC port with the wrong tenant. It also skips the zero `connection_timeout`
   check that `build()` has, and ignores `without_tls()`.
3. **G-06.** On the server crate alone (the docs' "Server only" recipe), push cannot
   reach any `https://` webhook, and `OpenAiChatModel` cannot reach an `https://`
   model. With `auth-jwt`, every token is refused unless the identity provider is on
   loopback: the JWKS client is plain HTTP, and 0.16 refuses plaintext JWKS URLs. The
   JWKS part rests on hyper-util refusing `https` by default, which I did not run.
   Recommendation: make `tls-rustls` a server default in 0.16.0.
4. **G-07 and G-05 (first impressions).**
   - The flagship hello-agent card cannot stream, because it never sets
     `with_streaming(true)`.
   - `AgentCard::new` produces a card that `build()` refuses, and `AgentCard`'s rustdoc
     still calls it "valid by construction".
   - 15 agent accept loops in 11 example files copy the slowloris-prone loop. All of
     them busy-spin on accept errors, not the five the report counted.
5. **G-12 and G-08.** `EventStream` is not a `Stream`, and `agent_executor!` cannot
   reach `self` (by Rust's hygiene rules; not compiled). Both fixes are cheap,
   additive and safe to ship in any release.

## New findings

- **NEW-g-api-1 (medium).** The 57f034e fix for blocking sends does not work over
  gRPC.
  - The tonic channel is built with `Endpoint::timeout(request_timeout)`, 30 s.
  - tonic 0.14.6's client applies the **shorter** of that and the per-request
    `grpc-timeout`. I read this in its source (`connection.rs:73`,
    `grpc_timeout.rs:47-56`).
  - So a blocking gRPC `SendMessage` still fails at 30 s. The error is
    `Protocol(InternalError)` saying "cancelled by the peer", which is not retryable,
    while the task keeps running.
  - No test covers gRPC here.
- **NEW-g-api-2 (low).** G-02's `max_connections` fix covers `Server` only.
  `GrpcDispatcher::with_max_connections` and `WebSocketDispatcher::with_max_connections`
  still panic on `usize::MAX` and never accept anything on 0.
- **NEW-g-api-3 (low).** With turn claims (the default on PostgreSQL and SQLite), a
  task that G-21 leaves non-terminal is later failed by the reaper as `transient`. Its
  message says "the replica running this task stopped", which is false. On the
  in-memory store the same task stays `working` forever.
- **NEW-g-api-4 (low).** STABILITY.md section 1 still names 0.15.0 as the newest
  release and promises that `sign_agent_card` is removed in 0.16.0. The function is
  still public, and its removal is not in `[Unreleased]` Breaking Changes. Two other
  items deprecated since 0.15.0 can also be removed now: `ClientConfig::preferred_bindings`
  and `HotReloadAgentCardHandler::update`.

## What I could not check

- Nothing was compiled. These claims rest on reading the code:
  - G-08's `self` hygiene follows a Rust language rule.
  - The G-06 compile cost of `otlp` was not measured. The probe is in the JSON.
  - NEW-g-api-1 rests on the tonic source in the local cargo registry. It is the same
    version as `Cargo.lock`, but I did not run it.
  - hyper-util's refusal of `https` URIs (part of G-06) comes from its documented
    default; I did not run it.
- I did not check whether any peer SDK deduplicates on `messageId`, which is why
  G-13 is rated low.
- The struct-literal counts in G-10 come from a rough grep and include tests, the TCK
  and the book.
- None of the slice needs a live probe. Each open question can be settled by a unit
  or integration test, as stated per row.
