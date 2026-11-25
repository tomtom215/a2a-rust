<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ECO: ecosystem and interoperability

Read [`../README.md`](../README.md) §6 first, then E02: the feature matrix (§1), what peers have that this
SDK lacks (§2), what this SDK has that needs glue elsewhere (§3), and the assessment of the thesis (§6).

Why this workstream exists: the swarm controls are unique among A2A SDKs and are private extension URIs, so
they stop at the language boundary (README §2 finding 3). Every package here is about reach. The controls
use A2A's own extension mechanism and their contracts are written down, so the work is to make existing
contracts travel, not to redesign them. It has four layers, ordered by who must cooperate: the caller alone
(SWM-13, SWM-14, in the swarm file), a gated contract (ECO-06), one small middleware per SDK, a2a-rs first (ECO-04), and an
upstream namespace (ECO-02). Several packages here need an act only the maintainer can perform: publishing,
filing, posting. A session prepares; the maintainer
sends. **Nothing is filed, posted or published upstream by a session** (`docs/upstream/README.md`:
"Nothing is filed upstream without asking").

---

## ECO-01: Distribution

- **Goal.** What exists and is unpublished is published, in the order that costs adopters least.
- **Evidence.** E02 §2 item 3 (the official Rust SDK is the one on a2a-protocol.org and has a published
  CLI; this project's CLI is `publish = false`); ROADMAP *Developer experience* item 6 and *Release
  engineering* item 5; E06 §6 rows 8, 9 and 21; `docs/handoff.md` (the SLIMRPC binding has never been
  published; the discovery crate is not published).
- **Gates.** DEC-12, per crate. **Depends.** none. **Release.** the maintainer's act.
- **Read first.** `RELEASING.md` (the binding's first publish and its MSRV build), `packaging/README.md`
  (the Homebrew and winget drafts), `tools/a2a-cli/Cargo.toml`, ADR 0029.
- **Do.** For each crate the maintainer chooses: the dry run `RELEASING.md` describes, the MSRV build, the
  packaging check, and the book page moved from a path dependency to a version. Prepare; do not publish.
- **Accept.** `cargo package` succeeds as CI runs it for each chosen crate; `python3
  scripts/check_package_excludes.py` and `python3 scripts/check_doc_versions.py` exit 0; the pull request
  lists what the maintainer must run.
- **Out of scope.** Publishing.

## ECO-02: The upstream proposals

- **Goal.** The two drafts that would carry the swarm controls across SDKs are ready to send, re-verified
  against 1.1.
- **Evidence.** E01 §3.2 (none of the `docs/upstream/` drafts was adopted or contradicted by 1.1: lineage
  and halt, and deadline and budget, are "untouched"; the required-empty-array and patch-versioning reports
  are still valid; 1.1's `int64` fields give the lineage draft's third topic more weight); E02 §5 (1.1 adds
  no lineage, cascade, budget or lease; a branch of the official Rust SDK carries collaboration extensions
  under upstream-namespaced URIs).
- **Gates.** DEC-06. **Depends.** COR-04 (the cancel cause belongs in the lineage and halt proposal).
- **Release.** documents.
- **Read first.** `docs/upstream/README.md` (each draft's status), the two drafts, upstream's extension
  governance page at the `dev-1.1` tip (`docs/topics/extension-and-binding-governance.md` in the upstream
  repository), ADR 0049.
- **Do.** Re-read each draft against the `dev-1.1` text; correct every section and line reference; add what
  COR-04 and COR-05 learned; state the migration from the private URIs to an upstream namespace if one is
  granted (a new URI, both served for a release, ADR 0049's rule for a breaking extension change).
- **Accept.** Each draft's references resolve against the vendored 1.1 text (SPC-02); `docs/upstream/README.md`
  says which are ready for the maintainer.
- **Out of scope.** Sending.

## ECO-03: The comparison, refreshed

- **Goal.** One current, dated account of where this SDK stands against the official Rust SDK and the
  others, including where it is behind.
- **Evidence.** E02 "Staleness" (`docs/sdk-comparison-2026-11-11.md` predates 0.16.0 and omits the peer's
  1.1 stack; `docs/rust-sdk-assessment.md` verified the peer at a July commit; upstream's community page
  lists this project at "v1.0.0").
- **Gates.** none. **Depends.** PRF-07 for the performance half. **Release.** a document.
- **Do.** A dated comparison built from E02 §1's matrix with each cell re-checked at the peer's current
  commit (record it), the claims ledger's standard for what may be said (`docs/claims-ledger-2026-09-24.md`),
  and E02 §6's verdict on the swarm thesis stated on the page. Note for the maintainer what upstream's
  community page says about this project.
- **Touch.** A new dated document under `docs/`, `docs/README.md` ("Start here"), `README.md` only where a
  claim changes.
- **Accept.** Every cell cites a file in a named commit or a URL. Every "absent" names what was searched.
  `./scripts/check_benchmark_prose.sh` exits 0.
- **Out of scope.** Marketing copy.

## ECO-04: The swarm extensions in other SDKs, a2a-rs first

- **Goal.** A hop written with another SDK can carry lineage, budget and the cancel cascade through, so a
  mixed-SDK tree keeps its controls. The official Rust SDK is the first target.
- **Evidence.** ROADMAP *Fleet* item 14 (H19); ADR 0049 ("What is not built": middleware for other SDKs);
  E03 §3 G34 (Python and Go shims run in interop today); E02 §4 item 6 (the propagator pattern: a server
  interceptor captures allow-listed metadata and a client interceptor re-emits it).
  Why a2a-rs first, decided by the maintainer on 2026-11-25: it has the hooks on both sides
  (`pub trait CallInterceptor` in its `a2a-server/src/middleware.rs` and `a2a-client/src/middleware.rs`,
  read at its commit `4573e5c`); being Rust, a middleware for it can depend on this project's types crate
  and run this project's vectors with no second implementation of the contracts; and Goose's proposed
  native A2A support names a2a-rs as its library (issue 13388 of `aaif-goose/goose`, opened 2026-11-09,
  open and not yet accepted when read on 2026-11-25), so one middleware would reach that agent and every
  other a2a-rs agent.
- **Gates.** none to build; publishing a crate, or offering it to another project, is the maintainer's.
- **Depends.** COR-04, COR-05, SWM-01 (steering's schema and vectors), ECO-06. **Release.** outside the four
  published crates; where it lives (a crate in this repository outside the workspace, as the SLIMRPC binding
  is, or elsewhere) is part of the package's first step.
- **Read first.** ADR 0049 whole; `book/src/reference/swarm-extensions.md`;
  `crates/a2a-protocol-types/extensions/` (the schemas and vectors); `itk/README.md` (a2a-rs is already one
  of the peers the interop suite runs against); `benches/sdk-comparison/harness/agent-a2a-rs` (an a2a-rs
  agent already built in this repository, with its own lockfile and advisory configuration);
  `CONTRIBUTING.md`, *Working on the SLIMRPC binding* (what an out-of-workspace crate costs).
- **Do.**
  1. Re-read a2a-rs's two `CallInterceptor` traits at its current release and record the version and
     commit; say whether they can read and write message and status metadata and observe a cancel. If they
     cannot, that is the finding: report it and stop.
  2. A pair of interceptors for a2a-rs that does what ADR 0049 decision 4 asks of a hop: forward the
     lineage of the task being served with `depth + 1` (refusing past the depth limit); forward the budget
     with the remainder decremented; stop on a `HALTED` refusal and do not retry it; cancel the tasks it
     started when the task it serves is cancelled. Reuse the types crate; write no second parser.
  3. Declare the extensions on the agent's card, so SWM-13 reports them.
  4. Test it as a middle hop between two agents built on this SDK, in `itk/`.
  5. Go second, by the same contract, once a2a-rs is done and measured.
- **Accept.** An interop test with an a2a-rs hop in the middle: the grandchild's lineage root is the first
  task's; its deadline is inside the parent's; cancelling the root ends the grandchild; a halt of the root
  is not retried by the hop. The extension vectors pass in the middleware's own tests. The middleware's
  crate has its own `cargo deny check` as the comparison harness does.
- **Out of scope.** Python, JavaScript, Java and .NET. Changing a2a-rs itself. Offering the middleware to
  any project (the maintainer's act).

## ECO-05: Discovery beyond one link, by catalog

- **Goal.** A client can find agents from a catalog document, without this SDK becoming a registry.
- **Evidence.** E02 §6 and §5 (the ecosystem is converging on catalogs; this SDK has mDNS and a card
  cache); E01 §2.8 C5 (1.1's discovery topic makes `/.well-known/ai-catalog.json` "preferred" and demotes
  the single well-known card; a topic page, not a normative MUST); ADR 0031 (its sources are designed and
  unbuilt).
- **Gates.** DEC-02 (it follows 1.1's text). **Depends.** SPC-03. **Release.** `[0.17]`, additive.
- **Read first.** ADR 0031, ADR 0051, `crates/a2a-protocol-client/src/discovery.rs`, the discovery crate,
  the upstream discovery topic at the vendored commit.
- **Do.** Amend ADR 0031 with the catalog as a source. Build the one source: fetch, validate, cache with
  the rules the card cache already follows (RFC 9111 handling exists there; reuse it), yield cards. A client
  reading a document is not a registry: nothing here stores, ranks or health-checks agents.
- **Accept.** A test against a served catalog document taken from upstream's own example (vendored with its
  commit); the SSRF guard that protects a card fetch protects the catalog fetch (name the test).
- **Out of scope.** Serving a catalog; a directory service; AGNTCY's directory.

## ECO-06: The extension contracts are gated

- **Goal.** The reference page, the schemas, the vectors and the code for every wire extension cannot drift
  apart, so an implementation in another language can be built from the contract alone.
- **Evidence.** ADR 0049 ("What is not built": `scripts/check_extension_vectors.py` as a CI gate, because
  it needs `jsonschema`, which CI's script gates do not install); E03 §7 D7 (steering has no schema or
  vectors; SWM-01 adds them); `crates/a2a-protocol-types/extensions/` holds a folder per extension.
- **Gates.** none. **Depends.** SWM-01. **Release.** internal.
- **Read first.** `scripts/check_extension_vectors.py`, ADR 0049 (the amendment on schemas and vectors for
  every extension), `.github/requirements/` (how a pinned, hash-checked Python dependency is installed for
  a gate), ADR 0002.
- **Do.** Run the script in `ci.yml` `static-checks`, with `jsonschema` installed the way `pyyaml` and
  `cryptography` already are; or, if that dependency is unwanted, validate in a Rust test inside the types
  crate against the same files and say which was chosen and why. Add a check that every extension URI the
  crates define has a folder with a schema and at least one valid and one invalid vector.
- **Touch.** `.github/workflows/ci.yml`, `.github/requirements/`, `scripts/check_extension_vectors.py`,
  `scripts/prove_gates_fail.sh`, ADR 0049 (a dated amendment).
- **Accept.** `scripts/prove_gates_fail.sh` proves the gate: a vector edited to contradict its schema turns
  it red, and so does an extension URI with no folder. `./scripts/check_gate_inventory.sh` exits 0.
- **Out of scope.** Changing any extension's wire shape.

## ECO-07: A Goose A2A agent in the interop runs

- **Goal.** When Goose serves A2A, this SDK's client and delegation are tested against it, as a peer that
  does not declare this project's controls.
- **Evidence.** Issue 13388 of `aaif-goose/goose`, "Native A2A support: goose a2a server and remote A2A
  agents in delegate" (opened 2026-11-09; open, assigned and marked "Needs info" when read on 2026-11-25):
  a `goose a2a` command behind an optional `a2a` feature, JSON-RPC with a bearer token, streaming,
  `CancelTask`, approvals as `INPUT_REQUIRED`; no push, gRPC or REST in its first version. Goose at commit
  `1551ae80` has no A2A code (a search of the clone for "a2a" and "agent2agent" finds prose only). E02 §5
  ("coding agents as fleet members" is the upstream project's own picture of a fleet).
- **Gates.** An upstream event: a Goose release, or a merged branch, that serves A2A. Until then this
  package is `todo` and nothing is built. **Depends.** SWM-13, SWM-14. **Release.** tests and an example.
- **Read first.** `itk/README.md` and `itk/run_itk.sh` (how a peer is added and pinned);
  `.github/workflows/itk-nightly.yml`; the issue and whatever shipped, read at that time: the design may
  change before it lands.
- **Do.**
  1. Pin the Goose revision as the other peers are pinned, and run it with a model that needs no network
     (the repository already runs llama.cpp for its live tests; `docs/handoff.md` has the recipe).
  2. Tests from this SDK as the caller: card fetch and interface selection; a streamed send to completion;
     `CancelTask` of a running turn; a task that reaches `input-required` and its continuation; a bearer
     token refused and accepted.
  3. Delegation: SWM-13 reports Goose's declared controls; a budgeted delegation is cancelled by the caller
     at its deadline (SWM-14); cancelling the parent cancels the Goose child.
  4. A nightly, informational job, as the other peer runs are; it gates no pull request.
- **Touch.** `itk/`, `.github/workflows/`, `scripts/lib/workflow_inventory.py`,
  `scripts/prove_workflow_gates_fail.py` (a probe or an exemption with its reason), the book's interop page.
- **Accept.** Each test names the Goose revision it ran against. A failure is attributed: this SDK's
  defect (a register row), Goose's (reported to the maintainer, not filed by a session), or the
  specification's.
- **Out of scope.** Changing Goose. Any claim about Goose's behaviour that a run did not show.

