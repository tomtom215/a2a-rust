<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ECO: ecosystem and interoperability

Read [`../README.md`](../README.md) §6 first, then E02: the feature matrix (§1), what peers have that this
SDK lacks (§2), what this SDK has that needs glue elsewhere (§3), and the assessment of the thesis (§6).

Why this workstream exists: the swarm controls are unique among A2A SDKs and are private extension URIs, so
they stop at the language boundary (README §2 finding 3). Every package here is about reach, and most need
an act only the maintainer can perform: publishing, filing, posting. A session prepares; the maintainer
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

## ECO-04: The swarm extensions in other SDKs

- **Goal.** A hop written with another SDK can carry lineage, budget and the cancel cascade through, so a
  mixed-language tree keeps its controls.
- **Evidence.** ROADMAP *Fleet* item 14 (H19); ADR 0049 ("What is not built": middleware for other SDKs);
  E02 §4 (a2a-go's metadata propagator pair is "the smallest thing another SDK needs to carry a2a-rust's
  controls through a foreign hop"); E03 §3 G34 (Python and Go shims run in interop today).
- **Gates.** none to build; publishing a package under another ecosystem's registry is the maintainer's.
- **Depends.** COR-04, COR-05, SWM-01 (steering's schema and vectors). **Release.** outside the published
  crates.
- **Read first.** ADR 0049 whole; `book/src/reference/swarm-extensions.md`;
  `crates/a2a-protocol-types/extensions/` (the schemas and vectors a foreign implementation is tested
  against); `itk/` (how the Python and Go peers are driven); `examples/multi-lang-team`.
- **Do.** Start with one SDK, Go (its propagator hooks are the smallest surface): a small middleware that
  forwards the lineage and budget extensions from an inbound request to outbound calls, decrements the
  budget, stops on a halt refusal, and cancels its children when it is cancelled. Test it against the
  vectors and in `itk/` as a middle hop between two Rust agents.
- **Accept.** An interop test with a Go hop in the middle: the grandchild's lineage root is the first
  task's; its deadline is inside the parent's; cancelling the root ends the grandchild. The vectors pass in
  the Go test suite.
- **Out of scope.** Python, JavaScript, Java and .NET until Go is done and measured.

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
