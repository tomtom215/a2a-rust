<!-- SPDX-License-Identifier: Apache-2.0 -->

# Rust A2A SDKs — technical and governance assessment

**Prepared for:** Linux Foundation / A2A project technical leadership
**Date:** 27 July 2026
**Supersedes:** the earlier `a2a-rust` v0.6.0 capability comparison

Subjects:

- **`a2a-rs`** — <https://github.com/a2aproject/a2a-rs> — verified at `94f4d32` (`main`, 2026-07-27)
- **`a2a-rust`** — <https://github.com/tomtom215/a2a-rust> — verified at `b416c1a` (v0.7.0 + 3 commits, 2026-07-24)

---

## 0. Method, and what this document is not

Every claim below was checked against primary sources on 27 July 2026: both
repositories were cloned and read at the commits named above; both test suites
were built and run locally; and figures were pulled from crates.io, Codecov,
the GitHub Actions API, and the a2a-rs nightly interop-metrics release asset.
Where a claim could not be verified, it is marked as such rather than
softened.

**Not covered:** independent performance benchmarking, a security audit of
either codebase, and any legal review of code provenance. Section 5 flags a
provenance question that needs an actual lawyer, not this document.

**Disclosure, because it bears on how you should read this.** This assessment
was produced by an AI assistant working inside the `a2a-rust` repository at
the request of its maintainer. That is a real source of bias. It is mitigated
here by citing a verifiable source for every material claim, by including the
findings that cut against `a2a-rust` (Sections 4 and 5, which are the longest
sections in the document), and by correcting three errors in the earlier
comparison that all favoured `a2a-rust`. Verify anything you would act on.

---

## 1. Executive summary

**The consolidation question posed by the earlier document is now closed.**
`a2a-rs` is listed as the Rust SDK in the official SDK table at
[a2a-protocol.org/latest/sdk/](https://a2a-protocol.org/latest/sdk/),
alongside `a2a-python`, `a2a-go`, `a2a-java`, `a2a-js`, and `a2a-dotnet`, with
no distinguishing status label. The live question is therefore not *which
project becomes the official Rust SDK* but *whether anything in `a2a-rust` is
worth absorbing into the one that already is, and on what terms.*

**On that narrower question, the honest answer is: yes, selectively, and not
as a merge.**

- `a2a-rs` is the smaller, better-governed, more widely used, and
  better-connected project. It has real interop evidence running nightly
  against the official Go/Java/Python agents, DCO on 76% of commits, an
  institutional home, and roughly 20–30× the crates.io adoption.
- `a2a-rs` is also genuinely thin where production deployments hurt:
  in-memory task storage only, no server-side authentication primitives, no
  request-size or concurrency limits, no rate limiting, no metrics export, and
  a `tenant` field that is carried but never enforced.
- `a2a-rust` has all of those things, a much larger test suite, and — as of
  three days ago — a protobuf-native gRPC binding that closes the interop gap
  that previously disqualified it.
- `a2a-rust` also carries a provenance question: **478 of its 608 commits are
  authored by `Claude <noreply@anthropic.com>`, and none of the 608 carry a
  per-commit DCO sign-off.** As of 2026-07-27 the project has adopted the DCO,
  gated it in CI, discontinued AI-identity authorship, and published a
  disclosure plus a blanket certification of the existing history
  (`PROVENANCE.md`). What remains is not engineering work but a ruling from LF
  counsel on whether that blanket certification is sufficient — see 5.1.

**Recommendation (Section 7):** do not merge repositories. Port capabilities
into `a2a-rs` as discrete, individually reviewed pull requests under DCO,
starting with the four that matter most (pluggable persistence, server auth
interceptors, resource limits, OTel). Offer the `a2a-rust` maintainer a
committer path in `a2a-rs` earned through those PRs. This gets the Rust SDK
the production depth it lacks without importing ~89,000 lines of unreviewed,
un-signed-off code into a Linux Foundation project, and without spending the
maintainer capacity a repository merge would consume.

---

## 2. Corrections to the earlier v0.6.0 comparison

Four claims in the previous document no longer hold. All four favoured
`a2a-rust`, which is itself worth noting.

| Earlier claim | Status |
|---|---|
| "a2a-rust: gRPC is JSON tunneled in a protobuf `bytes` envelope; a2a-rs is protobuf-native" | **Fixed in a2a-rust v0.7.0** (2026-07-24). It now serves the canonical `lf.a2a.v1.A2AService` with wire fixtures checked against the official Python SDK. The gap is closed. |
| "a2a-rs maintainer: Luca Muscariello (sole)" | **Understated.** `git log` shows 7 committers (Luca Muscariello 73, Mauro Sardara 10, plus four external contributors and a release bot). One *formal* maintainer is listed in `MAINTAINERS.md`, which is a different claim. |
| "a2a-rust multi-OS CI: Linux, macOS" | **Outdated.** The CI matrix is `[ubuntu-latest, macos-latest, windows-latest] × [stable, 1.93]`. |
| "a2a-rust — production depth … hardened" (of v0.6.0) | **Was not accurate at the time.** See Section 4.3: the v0.7.0 changelog documents multiple security defects present in the v0.6.0 the earlier document was assessing, including unenforced tenant isolation. |

The earlier document also framed the two projects as "complementary rather
than redundant." On inspection they are substantially redundant: both
implement the same 11 methods over three of the same transports against the
same v1.0 wire spec. The differences are in depth and in operational
surface, not in scope.

---

## 3. Vitals

| | `a2a-rs` | `a2a-rust` |
|---|---|---|
| Home | `a2aproject` org (AGNTCY / LF) | Personal repository |
| **Listed as the official Rust SDK** | **Yes** (a2a-protocol.org/latest/sdk) | No |
| First commit | 2026-04-03 | 2026-03-15 |
| Commits | 114 | 608 |
| Human committers | 6 (+ release bot) | 1 (+ 478 AI-authored commits, + bot) |
| Formal maintainers | 1 (`MAINTAINERS.md`) | 1 (`GOVERNANCE.md`) |
| DCO sign-off rate | 87 / 114 (76%) | 0 / 608 historical; DCO adopted and CI-gated 2026-07-27, history covered by blanket certification (see 5.1) |
| Copyright attribution | "AGNTCY Contributors" | Single named individual |
| Stars / forks / open issues | 55 / 14 / 6 | 19 / 0 / 0 |
| crates.io downloads (recent 90d) | `a2a-lf` 23.2k, `a2a-client-lf` 18.0k, `a2a-server-lf` 14.9k | `a2a-protocol-sdk` 526, `-server` 616, `-client` 589 |
| Latest release | `a2a-server-lf` 0.4.1 (2026-07-16) | v0.7.0 (2026-07-24; 14 downloads) |
| Rust edition / MSRV | 2024 / **1.85** | 2021 / **1.93** |
| Hand-written source LOC (approx.) | ~17k (+5k generated protobuf) | ~61k src (incl. inline tests) + 28k in `tests/` |
| Tests passing locally | 454 | 2,086 (default) / 2,519 (all features) |
| Codecov, last upload | 96.56%, 2026-07-27 | **93.62%, 2026-07-31** (current; see 4.4 for why local reads 95.75%) |
| Security contact | `security@agntcy.org` | Individual maintainer |

Two figures deserve caveats. **crates.io download counts** include CI, mirror,
and bot traffic and are a weak proxy for real adoption — but a 20–30×
difference is larger than that noise. **`a2a-protocol-types` shows 10.4k
recent downloads**, far out of line with the other `a2a-protocol-*` crates;
this looks like automated traffic rather than use, and the umbrella
`a2a-protocol-sdk` figure of 526 is the more honest indicator.

---

## 4. Capability comparison

Legend: ✅ implemented · ◑ partial or infrastructure-only · ❌ absent
(positively verified in source, not merely unfound).

### 4.1 Protocol and transports

| | `a2a-rs` | `a2a-rust` |
|---|---|---|
| A2A v1.0, all 11 service methods | ✅ | ✅ |
| JSON-RPC 2.0 over HTTP | ✅ | ✅ |
| REST / HTTP+JSON | ✅ | ✅ |
| gRPC, protobuf-native, ProtoJSON | ✅ (`a2a-pb`, `pbjson`) | ✅ **since v0.7.0** (`lf.a2a.v1`, golden wire fixtures vs official Python SDK) |
| SSE streaming | ✅ | ✅ (multi-subscriber broadcast) |
| Push notifications | ✅ | ✅ |
| SLIMRPC | ✅ | ❌ |
| WebSocket | ❌ | ✅ |

Both SLIMRPC and WebSocket are **non-spec transports**. The A2A v1.0 spec
names `JSONRPC`, `GRPC`, and `HTTP+JSON`; both implementations treat
`protocolBinding` as an open string per §12. Neither should be scored as a
compliance advantage. SLIMRPC's practical weight is that it connects the SDK
to the AGNTCY SLIM fabric; its practical cost is that the official Rust SDK's
workspace depends on `agntcy-slim-rpc` **2.0.0-alpha.7** — an alpha-versioned
vendor-adjacent dependency in an official Linux Foundation SDK. That is worth
a conversation independent of anything in this document.

#### 4.1.1 No preparatory refactor is needed to support SLIMRPC here

*Added 2026-07-30. Confidence: verified by reading the current public API,
not inferred from the architecture.*

A recurring question is whether this SDK should be restructured now so a
SLIMRPC binding could slot in later. It does not need to be — the extension
points already exist and are already exercised by a shipping non-spec binding
(WebSocket):

| Extension point | Status |
|---|---|
| `a2a_protocol_client::transport::Transport` | `pub`, object-safe (`Box<dyn Transport>`) |
| `A2aClientBuilder::with_custom_transport(impl Transport)` | `pub` — a third party can inject a transport with no fork |
| `AgentInterface::protocol_binding` | plain `String`, so any custom binding URI is advertisable with no type change |
| `RequestHandler` | `pub`, with `pub` `on_*` methods per operation, so an out-of-tree dispatcher can drive it |

The consequence is that a SLIMRPC binding can live in a **separate crate**
depending on these three, rather than in this workspace as `a2a-rs` does it
in-tree. That is strictly better for the concern raised just above: it keeps
`agntcy-slim-*` alpha versions out of this workspace's `Cargo.lock` and out
of the `deny.toml` allow-list, while still producing a usable binding. It also
means the decision to build one carries no architectural deadline — the cost
of waiting is zero.

Combined with the evidence that SLIM is pre-1.0 across the stack, that the
binding specification is self-described as *"Experimental — community-contributed"*
(`a2aproject/experimental-cpb-slimrpc`), and that the ratified A2A
specification contains zero occurrences of "slim" or "agntcy", the
recommendation is to **not build it now** and to revisit if and when the
binding stabilises or a user asks for it.

### 4.2 Persistence, tenancy, and operations

| | `a2a-rs` | `a2a-rust` |
|---|---|---|
| Pluggable `TaskStore` trait | ✅ | ✅ |
| In-memory store | ✅ | ✅ |
| SQLite store | ❌ | ✅ |
| PostgreSQL store (+ migrations) | ❌ | ✅ |
| Tenant isolation | ◑ — `tenant` is carried on requests and in `CallContext`; nothing enforces it | ✅ — tenant-scoped stores, `TenantResolver`, per-tenant limits |
| OpenTelemetry / OTLP export | ❌ (no `opentelemetry` dependency anywhere in the workspace) | ✅ (`otel` feature: traces + metrics) |
| Pluggable metrics trait | ❌ | ✅ |
| `tracing` | ✅ | ✅ |
| Graceful shutdown | ❌ — no handler-level API; the axum host closes the socket, in-flight executors and event queues are not drained (the `shutdown` symbols in `handler.rs` are test helpers) | ✅ (`RequestHandler::shutdown` cancels tokens and destroys queues) |

`a2a-rs`'s `a2a-server/src/task_store/` contains exactly `inmemory.rs`,
`store.rs`, `mod.rs`. Any deployment that must survive a process restart has
to write its own store today. This is the single largest functional gap
between the two projects and the one most worth closing.

### 4.3 Security and hardening

| | `a2a-rs` | `a2a-rust` |
|---|---|---|
| TLS (rustls) | ✅ | ✅ |
| Client-side auth (credentials store, interceptor) | ✅ | ✅ (+ OAuth2 client-credentials, OIDC discovery) |
| **Server-side auth primitives** | ❌ — `CallInterceptor` hook + a `User` struct; the only shipped interceptor is `LoggingInterceptor`. Token validation is entirely the integrator's job. | ✅ — `ApiKeyAuthInterceptor`, `BearerTokenAuthInterceptor`, `JwtAuthInterceptor` (HS256/RS256/ES256, static or remote JWKS, OIDC discovery) |
| Rate limiting | ❌ | ✅ (`RateLimitInterceptor`, trusted-proxy-hop aware, bounded bucket map) |
| Request body-size limit | ❌ (framework default) | ✅ |
| Response body-size cap (client) | ❌ | ✅ (32 MiB default) |
| Concurrent-stream cap | ❌ | ✅ (1,024 default) |
| Content-Type / 415 validation | ❌ | ✅ |
| SSRF guard on push webhooks | ❌ | ✅ |
| Agent-card signing (JWS / ES256, RFC 8785) | ◑ — `AgentCardSignature` type exists; no signing or verification code | ✅ (`signing` feature, implemented and tested) |
| CORS | ◑ — hand-rolled, agent-card endpoint only; reflects any `Origin` **with `Access-Control-Allow-Credentials: true`**. `tower-http`'s `cors` feature is declared but unused. | ✅ — explicit `CorsConfig { allow_origin }` (`permissive()` is opt-in), applied across the dispatchers with a preflight response |
| HTTP caching (ETag / 304) on card endpoints | ❌ | ✅ |
| `#![forbid(unsafe_code)]` on library crates | ❌ | ✅ |
| `cargo-deny` (license/advisory) in CI | ❌ | ✅ |

A note on the CORS row: `a2a-rs` reflects the caller's `Origin` header back
with `Access-Control-Allow-Credentials: true`. On a public, unauthenticated
discovery endpoint that serves the same document to everyone, the practical
exposure is small — but the pattern is the one browsers' same-origin policy
exists to prevent, and it would not survive a hardening review. It is a
one-line fix and a good example of the class of defect a security-focused
contributor would catch.

**The critical caveat on this table.** `a2a-rust`'s hardening is real but
*young*. Its own v0.7.0 changelog (2026-07-24) records that the v0.6.0
release — the version the earlier comparison document praised as production
hardened — shipped with, among others:

- a `TenantResolver` that was configured but **never consulted**, so the
  client-supplied `tenant` field alone selected the store partition and any
  caller could read or write another tenant's tasks;
- a rate limiter that trusted `X-Forwarded-For` unconditionally, so a forged
  header bypassed it entirely, and that panicked on a zero-length window;
- unbounded push-notification config creation on the SQL stores (disk
  exhaustion + delivery amplification);
- fire-and-forget (`returnImmediately`) sends that spawned no processor, so
  nothing was persisted, no push fired, and the task never left `Submitted`.

These are now fixed. The point is not that the fixes are inadequate — they
look thorough — but that the depth advantage in the table above is **three
days old and self-audited**, has never been exercised by outside users at any
scale, and has not been independently reviewed. Read the table as "this
functionality exists and is tested," not as "this functionality is proven in
the field."

### 4.4 Testing and evidence

| | `a2a-rs` | `a2a-rust` |
|---|---|---|
| Tests passing (measured locally, Section 6) | 454 | 2,519 (all features) |
| Codecov coverage | 96.56%, uploaded 2026-07-27 | **93.62%, uploaded 2026-07-31** |
| Property tests (`proptest`) | ❌ | ✅ |
| Fuzzing | ❌ | ✅ (6 libFuzzer targets; 60s per PR, 10-min nightly) |
| Mutation testing, CI-gated | ❌ | ⚠️ (`cargo-mutants`, sharded, PR `--in-diff` gate + weekly full sweep — but **both gates were vacuous until 2026-08-06** — neither could fail on a surviving mutant; see [`mutation-history.md`](../book/src/reference/mutation-history.md)) |
| Regression-gated benchmarks | ❌ | ✅ (statistical gate on PRs) |
| `cargo-semver-checks` on release | ❌ | ✅ |
| Hostile-peer / adversarial client tests | ❌ | ✅ |
| CI OS matrix | Linux, macOS, Windows | Linux, macOS, Windows |
| MSRV leg in CI | ❌ | ✅ (1.93) |
| Feature-combination linting | ✅ (`cargo hack --each-feature`) | ✅ (explicit per-feature legs) |

**On the Codecov discrepancy — resolved, and re-measured 2026-08-06.** The
staleness described in the 27 July revision of this section is fixed. Codecov's
API now reports `615d01f8` (2026-07-31) as `state: complete` at **93.62%**, so
uploads are current and the patch gate is live again. The cause was the
upstream key-distribution outage recorded in `coverage.yml`, closed by
`codecov-action` v5.5.5.

A second, unrelated discrepancy replaced it, and it is worth stating precisely
because it is the kind of number that gets quoted: the badge reads 93.62% while
local `cargo llvm-cov` reads **95.75%** on the same tree. Both are correct
measurements of *different file sets*. Verified against Codecov's own per-file
report rather than inferred — all five Postgres source files appear in it
(0.00%–8.06%, 793 lines) despite being listed under `ignore` in `codecov.yml`,
while every `tck/` file is correctly absent. Recomputing local lcov with only
the three directory globs applied reproduces Codecov exactly (31519/33668 =
93.62% against its reported 31520/33668).

So Codecov honours the three `**` patterns and silently drops the five bare
file paths. Those files cannot be covered by the coverage job by construction —
they require a live PostgreSQL server, which only the separate `test-postgres`
job has — so they were inflating the denominator with permanently-uncoverable
lines. The entries now carry a glob token; that fix awaits one upload to
confirm.

Note also that `cargo llvm-cov` reports three different workspace totals —
regions 90.87%, functions 89.32%, lines 91.49% (whole workspace, before any
ignore list). Any single "coverage percentage" for this project needs to say
which of those it means.

### 4.5 Cross-SDK interop evidence — the decisive difference

This is where the two projects differ most in *kind* rather than degree.

**`a2a-rs`** runs the official `a2aproject/a2a-itk` harness nightly at
02:00 UTC and publishes machine-readable results to a rolling
`nightly-metrics` release consumed by the ITK dashboard. The current asset
holds 39 entries, 37 of which produced results (the first two, on 2026-06-11
and 06-12, recorded no scenarios):

- Most recent run recorded in the asset (2026-07-23): **168 / 180 scenarios
  passing (93.3%)**.
- All 12 failures are `HTTP_JSON — Resubscribe`, in every combination
  involving a `go_v10` or `java_v10` peer. JSON-RPC (60/60) and gRPC (60/60)
  are clean.
- The same 12 have failed on **every one of the 37 recorded runs**, going back
  to the first successful nightly on 2026-06-14 — a standing, unfixed defect
  roughly six weeks old, not a flake. (Whether the fault lies in `a2a-rs`
  or in the Go/Java peers is not determinable from the metrics alone.)

**`a2a-rust`** has more *breadth* of interop testing but weaker *authority*:

- Its own TCK (22 conformance checks per binding, × {JSON-RPC, REST}) runs on every PR against echo
  agents built on the official Python, JavaScript, Go, and Java SDKs, plus
  hand-written stubs — 8 matrix legs. Two documented reference-SDK
  divergences are `--skip`-ed with written justification.
- The official Python `a2a-sdk` *client* drives its server end to end
  (26 checks).
- gRPC wire compatibility is checked in both directions against golden bytes
  serialized by the official Python SDK.
- **But its upstream ITK job — the one that would be directly comparable to
  the `a2a-rs` number above — is `workflow_dispatch`-only and
  `continue-on-error: true`.** It is not a gate and has never run to
  completion. The stated reason is credible and not the project's fault: the
  upstream ITK's `uv.lock` pins baseline dependencies to a private Google
  Artifact Registry that returns 401 to unauthenticated clients. The
  in-repo deterministic self-test that substitutes for it is a reasonable
  proxy, but it is a proxy, and it is graded by the same project it grades.

**Assessment:** `a2a-rust`'s interop testing is broader and its gRPC wire
evidence is stronger. `a2a-rs`'s is the one that actually counts, because it
runs in the project's own harness against the project's own baselines and
publishes the number publicly every night. Third-party-verifiable evidence
beats self-verified breadth. If `a2a-rust` wants this argument, the move is to
fix the private-registry blocker upstream — which would benefit every SDK, not
just Rust — and turn the job into a gate.

### 4.6 Documentation and distribution

| | `a2a-rs` | `a2a-rust` |
|---|---|---|
| docs.rs | ✅ | ✅ |
| Narrative guide | READMEs per crate | ✅ mdBook (40+ pages) at a2a-rust.com |
| Architecture decision records | ❌ | ✅ (10 ADRs) |
| Spec traceability matrix | ❌ | ✅ (`SPEC_COMPLIANCE.md`, 73 rows, §-referenced) |
| Published CLI | ✅ `a2a-cli` | ❌ |
| Homebrew / winget / prebuilt binaries | ✅ | ❌ (releases carry no assets) |
| SLSA provenance / SBOM | ❌ | ◑ (configured in release CI; not yet exercised on a published release) |
| `CODE_OF_CONDUCT.md` | ✅ | ❌ (a paragraph in `GOVERNANCE.md`) |

---

## 5. Risk register

### 5.1 `a2a-rust` — provenance

This needs to be stated plainly because it will be the first thing LF counsel
asks about.

- **478 of 608 commits (79%) are authored by
  `Claude <noreply@anthropic.com>`.** A further 61 commit bodies carry
  `Co-Authored-By: Claude`. The codebase is predominantly AI-generated.
- **0 of 608 commits carry a `Signed-off-by:` line.** There is no DCO in
  `CONTRIBUTING.md`. `a2a-rs`, by contrast, requires DCO and has it on 76% of
  commits.
- All copyright headers name a single individual, not a contributors
  collective.

None of this makes the code unusable, and AI-assisted contribution is
widespread; this document takes no position on whether it is disqualifying,
because the Linux Foundation's current policy on AI-generated contributions
was not verified as part of this assessment and is a question for counsel, not
for an engineering comparison. What *is* certain is the mechanical problem:
608 commits with no sign-off cannot be accepted into a DCO-gated repository as
they stand, and the sign-off would have to be made by a human asserting rights
in output they did not personally write. Any transfer needs, at minimum: a
written provenance statement from the maintainer covering the AI-assisted
authorship and their rights in the output; a retroactive sign-off or a squashed
re-submission under DCO; and an explicit ruling from LF counsel on whether that
is sufficient. **This should be settled before
substantial code moves, not in parallel with it** — it is the item most likely
to stop the whole thing, and it is cheapest to resolve first.

### 5.1.1 What the project has since done about it

*Added 2026-07-27, after the first draft of this assessment. Verifiable at
`docs/rust-sdk-assessment.md`'s own commit and later.*

The mechanical half of the problem has been closed:

- **`DCO`** — the Developer Certificate of Origin 1.1, verbatim, at the
  repository root.
- **`PROVENANCE.md`** — discloses the AI-assisted development with reproducible
  authorship figures; records a **one-time blanket DCO certification** by the
  maintainer covering every commit through `b416c1a`, in his own name and
  email; and inventories the third-party material in the tree (the spec's
  `a2a.proto`, vendored googleapis stubs, the ITK `instruction.proto`, the
  a2a-inspector card ruleset) with its licensing.
- **`.github/workflows/dco.yml`** — a merge gate that fails any pull request
  containing a non-merge commit without a `Signed-off-by:` matching its git
  author, **and** rejects any commit authored by a known AI-assistant service
  account. The second check is what stops the original problem recurring: a
  sign-off is an assertion by a person, so the human must be the git author and
  the assistant goes in a `Co-Authored-By:` trailer.
- `CONTRIBUTING.md`, `GOVERNANCE.md`, `README.md` and a new PR template carry
  the requirement.

**What this does not settle.** Two things are still open and are not the
project's to decide:

1. Whether a blanket certification is acceptable to the receiving project, or
   whether per-commit sign-off on historical commits is required. The
   maintainer has stated in `PROVENANCE.md` that he will rewrite the history
   with `git filter-repo` on request; the cost is that all 608 SHAs change,
   ten tags must be re-cut, and the SLSA provenance attestations bound to the
   published v0.2.0–v0.7.0 crates stop resolving to real commits. That is a
   real loss of supply-chain metadata in exchange for a formality, which is why
   it was not done unprompted.
2. Whether the Linux Foundation's policy on AI-generated contributions permits
   this arrangement at all. Unchanged from the note above: a counsel question,
   not an engineering one.

So the correct reading is that this has moved from *blocker* to *open item
awaiting a ruling*, with the engineering-side remediation already in place and
the more expensive option pre-agreed if the ruling requires it.

### 5.2 `a2a-rust` — other risks

- **Bus factor 1**, with no second human reviewer. Every one of the 608
  commits was merged by the same person who wrote (or prompted) it.
- **No external validation.** 526 recent downloads of the umbrella crate, 14
  of v0.7.0, 0 forks, 0 open issues, 1 watcher. Nobody outside the project has
  stressed this code.
- **Maintenance surface.** ~89k lines vs ~25k. Four transports, four store
  backends, three auth interceptors, OTel, WebSocket. For a foundation with
  one Rust maintainer, that ratio is a liability, not an asset — every feature
  in the table above is also a feature someone has to keep working.
- **MSRV 1.93** is effectively current stable. `a2a-rs` at 1.85 is reachable
  from distribution toolchains and enterprise pins. For an official SDK the
  lower MSRV is the correct choice, and adopting `a2a-rust` code wholesale
  would drag the floor up.
- **Rapid breaking change.** v0.7.0 alone lists five explicitly labelled
  breaking changes,
  including removing v0.3-style method aliases and rejecting requests without
  `A2A-Version`. Correct decisions, but the API is not settled.
- The stale Codecov pipeline (4.4).

### 5.3 `a2a-rs` — risks

- **In-memory persistence only.** Not viable for any deployment that must
  survive a restart, and every adopter is currently reinventing the same
  store.
- **No server-side authentication primitives.** Every adopter writes their own
  token validation. For an official SDK this is a meaningful gap: it is the
  place where integrators most reliably get security wrong.
- **No resource limits of any kind** — no body-size cap, no concurrent-stream
  cap, no rate limiting. An unauthenticated peer can open unbounded streams.
- **`tenant` is carried but unenforced**, which is arguably worse than absent:
  it reads like a multi-tenancy feature and is not one. (`a2a-rust` shipped
  exactly this bug in v0.6.0 and treated it as a security fix.)
- **No observability export.** No `opentelemetry` dependency in the workspace.
- **A standing interop failure**: the same 12 `HTTP_JSON — Resubscribe`
  scenarios have failed on every recorded nightly run since 2026-06-14 — six
  weeks, publicly visible on the ITK dashboard.
- **One formal maintainer**, and an alpha-versioned vendor dependency
  (`agntcy-slim-rpc 2.0.0-alpha.7`) in the workspace of an official SDK.

---

## 6. Locally reproduced test results

Both suites were built and run on the same machine at the commits named at the
top of this document. Both are green.

| | `a2a-rs` `94f4d32` | `a2a-rust` `b416c1a` |
|---|---|---|
| `cargo test --workspace` | **454 passed**, 0 failed, 0 ignored (26 binaries) | **2,086 passed**, 0 failed, 5 ignored (73 binaries) |
| `cargo test --workspace --all-features` | n/a (no optional features on the server crate beyond TLS) | **2,519 passed**, 0 failed, 21 ignored (73 binaries) |

`a2a-rust` has roughly 5.5× the tests over roughly 3.5× the hand-written source
— a real difference in test density, consistent with its mutation-testing
gate.

**One documentation correction.** `a2a-rust`'s `SPEC_COMPLIANCE.md` states
"4000+ tests" for `cargo test --workspace --all-features`. The measured figure
at this commit is **2,519** (including doctests). The suite is large and
green; the published number is not accurate and should be corrected.

Note also that the `postgres` store tests are gated behind
`A2A_TEST_POSTGRES_URL` and did not execute in this run; they run in a
dedicated CI job with a live database, which is a reasonable arrangement but
means the PostgreSQL backend is not covered by the numbers above.

---

## 7. Options and recommendation

### Option A — Status quo: two Rust SDKs

Cheapest today, worst over time. Duplicated effort in the language whose
official SDK is the youngest and least featured of the six; users forced to pick between "the
official one" and "the one with a database"; both projects stay
under-maintained. Not recommended, but worth stating that this is what happens
by default if nobody acts.

### Option B — Port capabilities into `a2a-rs` as reviewed PRs *(recommended)*

`a2a-rs` remains the official SDK and the base. Specific `a2a-rust`
capabilities are re-contributed as discrete, individually reviewed pull
requests under DCO. The `a2a-rust` maintainer earns committer status through
that work, giving the official SDK a second active human.

Suggested order, highest value first:

1. **Pluggable persistence** — the `TaskStore`/`PushConfigStore` SQLite and
   PostgreSQL implementations behind feature flags. Largest gap, cleanest
   port (both projects already have the trait).
2. **Server auth interceptors** — API-key, bearer, and JWT (JWKS + OIDC
   discovery) on the existing `CallInterceptor` hook.
3. **Resource limits** — body-size cap, concurrent-stream cap, rate limiting,
   bounded push-config counts. Small, high-value, low-controversy.
4. **OpenTelemetry export** behind a feature flag.
5. **Agent-card signing** — implementation behind the type that already
   exists.
6. **Tenant enforcement** — either make `tenant` authoritative via a resolver,
   or remove the field. The current middle state is the worst option.
7. **Test-infrastructure transplants** — fuzz targets and the hostile-peer
   harness port with essentially no coupling to `a2a-rust`'s architecture and
   would harden `a2a-rs` immediately.

Deliberately **not** on the list: WebSocket transport (non-spec, and the
official SDK should not grow a fourth binding without a spec conversation),
and the mdBook/ADR corpus (documents `a2a-rust`'s architecture, not
`a2a-rs`'s).

Why this over a merge: it moves capability without moving provenance risk
(each PR is signed off by a human as it lands), it is reviewable at human
scale, it can start immediately on items 3 and 7 while the provenance
question is settled for the larger ones, and it fails gracefully — if the
collaboration doesn't work out, `a2a-rs` keeps whatever landed.

### Option C — Adopt `a2a-rust` as the base, port `a2a-rs`'s protobuf core and SLIMRPC into it

This is roughly what the earlier document proposed. It is now the weaker
option: it would replace an official, adopted, DCO-gated, 25k-line codebase
with an 89k-line one that has no external users, no DCO history, one human
contributor, and a higher MSRV — in order to gain capabilities that Option B
delivers incrementally at a fraction of the risk. It also throws away the
nightly ITK integration and the published-SDK listing. Not recommended.

### Option D — `a2a-rust` as a downstream layer

`a2a-rs` stays the protocol core; `a2a-rust` re-bases onto `a2a-rs`'s types
and continues independently as an opinionated server distribution (stores,
tenancy, hardening, WebSocket). Legitimate, and it preserves the work — but it
leaves the official SDK thin, which is the actual problem to solve. Reasonable
as a fallback if Option B stalls on provenance.

### The mechanism, verified — "donate to the LF" is not one of the options

*Added 2026-07-30. Confidence: verified against `a2aproject/A2A`'s own
`GOVERNANCE.md` and README, read live.*

Every option above assumes a decision-making body. It is worth naming which
one, because "prepare for a Linux Foundation donation" implies an LF intake
process that does not apply here:

- **A2A is already an LF project.** Its README states: *"The A2A Protocol is
  an open source project under the Linux Foundation, contributed by Google."*
  The repository also carries a `linux-foundation` topic tag. So the earlier
  phrase "official Linux Foundation SDK" elsewhere in this document is
  accurate, not loose — checked rather than assumed in either direction.
- **Governance is a corporate TSC, not individual meritocracy.**
  `A2A/GOVERNANCE.md` defines a Technical Steering Committee with **eight
  voting members, one each from Google, Microsoft, Cisco, AWS, Salesforce,
  ServiceNow, SAP and IBM**, which is "responsible for all technical
  oversight of the open source Project."
- **Maintainership is a TSC vote.** Verbatim: *"A Contributor may become a
  Maintainer by a vote of the TSC. A Maintainer may be removed by a vote of
  the TSC."* There is no contribution threshold that confers it
  automatically.
- Notably, `GOVERNANCE.md` itself does **not** restate LF hosting; its only
  LF reference is that *"TSC Meetings are held on the Linux Foundation's
  meeting platform."* The LF status comes from the README.

**What this changes.** There is no generic "donate a project to the LF"
pathway to target, because the umbrella already exists and already contains a
Rust SDK. The realistic asks are therefore (a) contribute into `a2a-rs`
(Option B), or (b) persuade an eight-corporation TSC to adopt or absorb an
outside codebase — a materially higher bar than an IP transfer, and one where
repository hygiene is necessary but nowhere near sufficient.

It also relocates the remaining blockers. As of this document's measurements
they are **not** primarily repo-quality items: licensing, DCO enforcement,
SPDX coverage, SBOM/SLSA provenance, conformance gating, `NOTICE`,
`CODE_OF_CONDUCT.md` and a written 1.0 policy are all in place. What remains
is (i) an official Rust SDK already occupying the slot, (ii) a corporate TSC
vote as the decision mechanism, (iii) the provenance disclosure in
`PROVENANCE.md` being assessed by that body rather than by this project, and
(iv) a maintainer group of one. Only (iv) is fully within this repository's
control, and it is not a documentation task.

**On (iv), per the maintainer (2026-07-30):** the single-maintainer structure
"is not a blocker in initial discussions that have occurred." That is
recorded here as the maintainer's report of conversations this document's
author has no independent visibility into — it is not an independently
verified finding, and the substance of those discussions is not reproduced
here. It does not change the structural observation that a bus factor of 1
is a risk worth reducing on its own merits, and it does not change the
separate, narrower point in [`CODE_OF_CONDUCT.md`](../CODE_OF_CONDUCT.md)
that a conduct report *about* the sole maintainer has no independent
escalation path inside this project. It does mean this item should not be
presented as gating a conversation that is, by that account, already
underway.

### Recommendation

**Option B**, with three gates before any code moves:

1. **Provenance cleared.** The maintainer-side work is done as of 2026-07-27
   (`DCO`, `PROVENANCE.md`, CI gate — see 5.1.1). What is still needed is a
   ruling from LF counsel on whether the blanket certification suffices or
   history must be rewritten, and on AI-generated contributions generally.
   Blocking for all substantial ports; items 3 and 7 above are small enough to
   be rewritten from scratch if counsel prefers that to any transfer.
2. **Second reviewer in place.** The point of the exercise is to get Rust off
   a bus factor of 1. If the outcome is one maintainer reviewing another's
   large PRs with no third party, the risk has moved rather than reduced.
3. **The standing ITK failure fixed first.** Twelve `HTTP_JSON — Resubscribe`
   scenarios failing nightly is the official SDK's most visible defect. It
   should be closed before the project takes on a large feature-import
   programme — and it is a good first collaboration, since `a2a-rust`'s
   resubscribe implementation was rewritten in v0.7.0 specifically for
   snapshot-then-EOF reconnection semantics.

---

## 8. Questions worth putting to both maintainers

1. To LF counsel: is the blanket DCO certification in `a2a-rust`'s
   `PROVENANCE.md` sufficient for these commits, or is a per-commit
   `Signed-off-by:` on rewritten history required? The maintainer has adopted
   the DCO and pre-agreed to the rewrite if it is.
2. To `a2a-rs`: is the `HTTP_JSON — Resubscribe` failure a defect in `a2a-rs`
   or in the Go/Java baselines, and what is the plan?
3. To both: what is the intended MSRV policy for the official Rust SDK? This
   silently constrains which code can move.
4. To `a2a-rs` / AGNTCY: what is the plan for `agntcy-slim-rpc`'s alpha
   version pin, and is SLIMRPC intended to be proposed as a spec binding or to
   remain an extension?
5. To the A2A project: can the ITK's private-registry dependency be replaced
   with a public one? It currently prevents any non-AGNTCY implementation from
   running the official harness in public CI — a barrier to exactly the kind
   of external contribution this consolidation is meant to encourage.

---

## Appendix — verification log

| Claim | How verified |
|---|---|
| a2a-rs is the official Rust SDK | Raw HTML of `a2a-protocol.org/latest/sdk/`, SDK table, row "Rust → a2a-rs" |
| Commit / author / DCO counts | `git log --format=...` on full (unshallowed) histories of both repos |
| a2a-rust DCO remediation | `DCO`, `PROVENANCE.md`, `.github/workflows/dco.yml` in this repository, added 2026-07-27 |
| a2a-rs task stores | Directory listing of `a2a-server/src/task_store/` |
| a2a-rs has no OTel | `grep -rn "opentelemetry\|otel\|OTLP"` over the workspace — no hits |
| a2a-rs server auth | `a2a-server/src/middleware.rs` — only `LoggingInterceptor` implements `CallInterceptor` |
| a2a-rs card signing | `AgentCardSignature` type in `a2a/src/agent_card.rs`; no signing/verification code in the workspace |
| a2a-rust protobuf-native gRPC | `proto/a2a_v1/a2a.proto` (`package lf.a2a.v1`), `proto/README.md`, ADR 0009, `tck/fixtures/grpc/` |
| a2a-rust v0.6.0 security defects | `CHANGELOG.md`, v0.7.0 "Security" and "Fixed (additional hardening)" sections |
| a2a-rust ITK job non-gating | `.github/workflows/itk.yml` — `if: github.event_name == 'workflow_dispatch'`, `continue-on-error: true` |
| a2a-rs ITK nightly results | `nightly-metrics` release asset `itk_rust.json`, 39 runs; latest 2026-07-23 |
| Coverage figures and freshness | Codecov API v2 `.../commits/?branch=main` for both repos |
| Download counts | crates.io API v1 |
| CI status | GitHub Actions API, `main` branch, last 30 runs |
| a2a-rs CORS behaviour | `a2a-server/src/agent_card.rs::handle_agent_card` — reflects `Origin`, sets `Access-Control-Allow-Credentials: true`; no `tower_http` import anywhere |
| a2a-rs graceful shutdown | `grep -rn "shutdown" a2a-server/src` — matches are inside `#[cfg(test)]` push-webhook helpers only |
| Official SDK table | `curl https://a2a-protocol.org/latest/sdk/`, table parsed from raw HTML (columns: Language, Repository) |
| Test results | `cargo test --workspace` (and `--all-features`) run locally at the commits named above |
