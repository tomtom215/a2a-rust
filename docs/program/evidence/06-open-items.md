<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

> **Frozen audit input, 2026-11-25, of the tree at `2058058d`.** Not maintained. Written by a delegated
> read-only audit for [`docs/program/README.md`](../README.md); its VERIFIED and INFERRED labels are the auditor's own.
> Re-check an anchor before acting on it. `UPSTREAM/` is a local clone of the named upstream repository.

# 06 — Open items: one deduplicated backlog for a2a-rust after 0.16.0

Read-only consolidation at HEAD `2058058d` (merge of #222), 2026-11-25. Nothing in the repository was modified; no cargo build or test was run.
The machine-readable twin is `06-open-items.json` in this directory (same ids). Every number below came from a command; the commands are quoted.

**Labels.** VERIFIED = the cited code or file was opened at HEAD in this session (`evidence_checked: true`). AS-DOCUMENTED = taken from the document cited, not opened in code. Severity is copied from the source; `unstated` means the source gives none.

## 1. What the sweep found, in short

- **206 rows** in the JSON: **190 are open work** (open 78, partial 86, decision 13, maintainer-action 7, waiting-upstream 6); 12 are stale claims (a document says open, HEAD says otherwise, or the reverse); 3 are accepted and documented; 1 cannot be read from a session.
- **67 rows were checked against the tree at HEAD**; the rest are AS-DOCUMENTED.
- **33 rows need the maintainer** before work can start (section 5 is the clean list).
- The canonical register is `docs/audits/0.16.0-release/findings.json` (130 rows: 108 register rows and 22 Known Limitations). `ROADMAP.md` lines 20-275 ("After 0.16.0") group it. `docs/handoff.md` adds about 30 items that are in neither.
- **No source states a severity above "High"**, and only three open rows carry "High": `CI1` ("High (release gate)"), `R3` ("High (conditional)") and the four untested RFC 8725 rows (`BL-035`, ledger severity "high"). The one data-loss path, `X-10`, has no severity in its register; its README calls it the row that "would have" blocked the release had it not been disclosed.
- **Stale claims that would mislead a plan:** `NEW-g-docs-2` and `NEW-h-s0-7` are listed as open in ROADMAP and the register and are fixed in code; the 2026-09-22 adopter audit carries 41 unmarked rows of which every one spot-checked is fixed; `docs/handoff.md` "Now" predates the merges of #220 and #222.

## 2. Method and its limits

- Read in full: `ROADMAP.md` 1-275, 575-660, 822-1101 (bullet heads, then the open ones), 1101-1415; `docs/handoff.md` 1-1000 and a keyword pass over 1000-1519 (Lessons: no open work found); every `findings.json`; the 0.16.0 register README; `docs/compliance/audit-register.md` (Open, Done, Decisions); both ledgers through `jq`; `docs/official-tck-findings.md` section 27; `acts/sut-behaviors.yaml`; `docs/readiness-bar.md`; `docs/upstream/README.md`; `STABILITY.md` 1-62, 120-135, 204-218; the waiver files; ADR status lines (58 files).
- Read by structure and keyword, not line by line: `docs/adopter-audit-2026-09-22.md` (status block, open-work heads, the finding tables), `docs/swarm-orchestration.md`, `docs/swarm-scale-findings.md`, `docs/lf-readiness-review.md` section 4, `docs/openssf-best-practices.md`, the two wave-2 campaign READMEs, `docs/audits/2026-11-01-triage/DECISIONS.md` (fork table and F16), `docs/audits/2026-10-29/README.md` (headings only: its section O is already joined into the 0.16.0 register).
- Not read: `CHANGELOG.md` beyond the 0.16.0 Known Limitations; `docs/handoff-history.md`; `docs/compliance/controls.toml` row by row (the handoff's count of 29 partial rows is quoted, not re-counted); `book/`.
- The 76 `fixed (part)` triage rows are carried one per id with the triage's severity of 2026-11-01. The register warns (README, limit 1) that section O gives one status to a table row naming several ids, "so an id can be listed here as `fixed (part)` when its own part is done". Five of the nine such rows opened here were fixed in whole or in the part the finding states (`NEW-g-docs-2`, `NEW-h-s0-7`, `G-04`, `B-15`, `D-08`). **Treat every `partial` triage row as needing a re-check before it is planned.**
- The clone is shallow, so nothing here rests on `git log` history.

## 3. The backlog

### fleet-correctness (29 rows)

| ID | Title | Severity | Status | Checked | Decision first | Sources |
|---|---|---|---|---|---|---|
| `BL-055` | ADR 0033 group delegation (fan-out): Proposed, nothing built | unstated | open | VERIFIED | no | `docs/adr/0033-group-delegation.md:7` |
| `E-F25` | CancelTask of a parked task writes Canceled with no event in the log; announce it from CancelTask | low | open | VERIFIED | no | `docs/audits/0.16.0-release/findings.json:147`, `ROADMAP.md:68`, `docs/handoff.md:51` (+6 in JSON) |
| `S3` | Fleet-wide inference quota and cost reservation with a declared overshoot bound | Scale | open | VERIFIED | yes: maintainer | `docs/audits/0.16.0-release/findings.json:75`, `ROADMAP.md:93`, `ROADMAP.md:95` (+2 in JSON) |
| `X-10` | A store outage longer than store_write_retry loses the event being written and can leave a task unfinished | unstated | open | VERIFIED | no | `docs/audits/0.16.0-release/findings.json:138`, `ROADMAP.md:65`, `ROADMAP.md:273` (+6 in JSON) |
| `BL-028` | SLIMRPC known limits: federation by manual bundle exchange, rotation covers JWT-SVIDs only, single-machine tests, static-token identity untested; a... | unstated | open | AS-DOC | no | `ROADMAP.md:603`, `ROADMAP.md:642` |
| `BL-047` | Cross-replica: a cancel reaches another replica's executor at the owner's next renewal (1 s) unless a halt store pushes it; subscribe on a non-owni... | unstated | open | AS-DOC | no | `docs/handoff.md:654`, `docs/adr/0057-wake-a-waiting-subscriber-on-log-appends.md:7` |
| `D-18` | Parked-task expiry: the stream that ends without a terminal state; the continuation that races the sweep | low | open | AS-DOC | no | `ROADMAP.md:88`, `docs/audits/2026-11-01-triage/d.json:215`, `CHANGELOG.md:3255` (+3 in JSON) |
| `KL-01` | Steering: a message handed to a replica that just died is accepted and unread until the claim expires; no gRPC/WebSocket tests; two-process proof n... | unstated | open | AS-DOC | no | `ROADMAP.md:84`, `CHANGELOG.md:3222`, `docs/audits/0.16.0-release/findings.json:966` (+2 in JSON) |
| `KL-13` | Load and admission across a fleet: ORCA over gRPC trailers, queue-cap gauge, priority, preemption, fleet-wide admission and limits (H7, H9, H17) | unstated | open | AS-DOC | no | `ROADMAP.md:93`, `CHANGELOG.md:3267`, `docs/audits/0.16.0-release/findings.json:1074` (+4 in JSON) |
| `D-08` | A silently dropped WebSocket flow is never detected by the client: no ping, no TCP keepalive, and a timeout does not mark the connection dead, so e... | medium | partial | VERIFIED | no | `docs/audits/0.16.0-release/findings.json:498`, `docs/audits/2026-11-01-triage/d.json:88`, `ROADMAP.md:97` (+1 in JSON) |
| `E-F12` | max_concurrent_tasks is enforced per process, so a fleet of N admits N times the limit, and the docs do not say so. | medium | partial | VERIFIED | no | `docs/audits/0.16.0-release/findings.json:588`, `docs/audits/2026-11-01-triage/e.json:94`, `ROADMAP.md:93` (+1 in JSON) |
| `G-04` | build_grpc still dials whatever endpoint from_card chose (the card's first interface), so a card listing JSON-RPC before gRPC gets gRPC spoken to t... | medium | partial | VERIFIED | no | `docs/audits/0.16.0-release/findings.json:660`, `docs/audits/2026-11-01-triage/g-api.json:12`, `ROADMAP.md:97` (+2 in JSON) |
| `B-10` | Quiet WebSocket and gRPC streams still carry no traffic by default on either side, so an L7 load balancer's idle timeout (often 60 s) closes them. ... | medium | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:318`, `docs/audits/2026-11-01-triage/b.json:87`, `ROADMAP.md:97` |
| `B-12` | The direct WebSocket dial is still tokio's sequential connect with no happy eyeballs and no TCP keepalive. | medium | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:327`, `docs/audits/2026-11-01-triage/b.json:128`, `ROADMAP.md:97` |
| `D-05` | All four sub-claims hold on main: a few in-memory attempts, nothing persisted across restart (and lanes now also shed under backlog), no dedupe id,... | medium | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:480`, `docs/audits/2026-11-01-triage/d.json:46`, `ROADMAP.md:90` |
| `D-07` | One connection whose peer stops reading can pin the server's read loop on an untimed sink lock, so the idle bound and the closing signal are never ... | medium | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:489`, `docs/audits/2026-11-01-triage/d.json:66`, `ROADMAP.md:97` |
| `D-09` | The connect timeout and protocol-level handshake failures are still non-retryable Transport errors and a 503 handshake's Retry-After is dropped; TL... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:507`, `docs/audits/2026-11-01-triage/d.json:107`, `ROADMAP.md:97` |
| `D-10` | As reported: a Retry-After longer than max_backoff is cut to 30 s and retried early, without jitter, and the HTTP-date form is ignored. The clamp i... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:516`, `docs/audits/2026-11-01-triage/d.json:125`, `ROADMAP.md:97` |
| `D-11` | Plain EventStream and follow_task still never resume a cut stream and retries are still off by default; only the Delegation API resumes now. | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:525`, `docs/audits/2026-11-01-triage/d.json:143`, `ROADMAP.md:97` |
| `D-12` | The idle-timeout race is fixed (50 s); connection max-lifetime, a per-host cap and HTTP/2 keepalive are still absent and are the subject of propose... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:534`, `docs/audits/2026-11-01-triage/d.json:162`, `ROADMAP.md:97` |
| `D-21` | A rolling deploy no longer cancels tasks; it fails every turn still running after 5 s as transient and refuses new sends retryably, but the work is... | medium | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:552`, `docs/audits/2026-11-01-triage/d.json:272`, `ROADMAP.md:86` (+2 in JSON) |
| `D-22` | Push DNS lookups still run getaddrinfo on the blocking pool and a timeout does not free the thread; the new delivery gate bounds concurrency but no... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:561`, `docs/audits/2026-11-01-triage/d.json:293` |
| `G-20` | The blocking-send half is fixed for JSON-RPC, HTTP+JSON and WebSocket with tests; other timeouts still say only 'timed out', and the gRPC blocking ... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:696`, `docs/audits/2026-11-01-triage/g-api.json:294`, `ROADMAP.md:97` |
| `H6` | No retry budget, no circuit breaker, no HTTP in-flight cap and no deadline-aware retries. Retries are off by default; when on, overload refusals ar... | S1 | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:921`, `docs/audits/2026-11-01-triage/h-designs.json:33` |
| `NEW-d-2` | A WebSocket call enqueues its frame with `write_tx.send(..).await` on a 64-slot channel before any timeout starts; request_timeout/send_timeout bou... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:570`, `docs/audits/2026-11-01-triage/d.json:376`, `ROADMAP.md:97` |
| `R3` | Push outbox: a writer whose process dies mid-write is settled only after the hold; rest of R3 and the ADR 0037 not-covered cases | High (conditional) | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:39`, `ROADMAP.md:90`, `docs/audits/2026-11-12-adversarial-review/findings.json:23` (+2 in JSON) |
| `SEC-14` | Outbound webhook concurrency is now bounded per destination and overall, in front of any sender. There is still no per-caller quota, no per-destina... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:624`, `docs/audits/2026-11-01-triage/f.json:104`, `ROADMAP.md:90` |
| `W2-hostile-3` | A finished task whose owner died is completed only after about three claim lifetimes | medium | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:129`, `ROADMAP.md:82`, `docs/audits/2026-11-11-external-review/findings.json:67` (+2 in JSON) |
| `E-F8` | The conjecture is read-confirmed: positions are broadcast before they are logged, a takeover numbers from the log end, and both resume paths treat ... | medium | disclosed-accepted | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:579`, `docs/audits/2026-11-01-triage/e.json:29`, `ROADMAP.md:80` (+2 in JSON) |

### spec-conformance (9 rows)

| ID | Title | Severity | Status | Checked | Decision first | Sources |
|---|---|---|---|---|---|---|
| `BL-002` | The 7 MUSTs the official TCK marks manual and the 16 it marks not tested: cite a test of this repository for each, or say there is none | unstated | open | VERIFIED | no | `ROADMAP.md:42`, `tck/untested-musts.toml:7`, `docs/official-tck-findings.md:2395` (+1 in JSON) |
| `AA22-OW1` | a2a-go cannot read this SDK's securityRequirements (T1, write side) | Medium | open | AS-DOC | no | `docs/adopter-audit-2026-09-22.md:653`, `docs/adopter-audit-2026-09-22.md:1105` |
| `BL-003` | A conformance run for the WebSocket binding, which is outside both external suites | unstated | open | AS-DOC | no | `ROADMAP.md:48`, `ROADMAP.md:1322` |
| `BL-042` | ADR 0025 plaintext refusal and warning were not checked on CombinedDispatcher and SLIMRPC | unstated | open | AS-DOC | no | `docs/handoff.md:363` |
| `BL-043` | Spec ledger: MAY entries relabelled SUPPORTED are not started | unstated | open | AS-DOC | no | `docs/handoff.md:379` |
| `BL-048` | Interop: card_signature_interop.py checks Python only | unstated | open | AS-DOC | no | `docs/handoff.md:735` |
| `BL-001` | Spec ledger: the 4 PARTIAL rows (3.5.2, 5.7, 7.6.1, 12.4): a test for each, or the reason it stays partial | unstated | partial | VERIFIED | yes: maintainer | `ROADMAP.md:39`, `docs/handoff.md:366`, `docs/compliance/spec-ledger.json:2790` (+4 in JSON) |
| `X-22` | Decide whether itk.yml pins A2A_ITK_REVISION as acts.yml does; record this repository's own ACTS counts per binding | unstated | decision | VERIFIED | yes: maintainer | `docs/audits/0.16.0-release/findings.json:228`, `ROADMAP.md:45`, `docs/audits/2026-11-11-external-review/findings.json:171` (+3 in JSON) |
| `BL-005` | Decide: A2A v1.1 (v1.1.0-rc tagged upstream 2026-10-29): diff it against the spec ledger now, or wait for a release | unstated | decision | AS-DOC | yes: maintainer | `ROADMAP.md:54`, `docs/handoff.md:39`, `docs/handoff.md:91` |

### security-compliance (40 rows)

| ID | Title | Severity | Status | Checked | Decision first | Sources |
|---|---|---|---|---|---|---|
| `BL-035` | RFC ledger: 4 high-severity RFC 8725 rows implemented and untested (3.1 alg used, 3.3 all crypto validated, 3.8 keys belong to issuer, 3.9 audience) | high | open | VERIFIED | no | `docs/handoff.md:495`, `docs/compliance/rfc-ledger.json:2578`, `docs/compliance/rfc-ledger.json:2783` (+2 in JSON) |
| `BL-036` | RFC ledger GAP: RFC 7515 5.2 and 7.2.1, the card verifier does not refuse a header name present in both protected and unprotected headers | low | open | VERIFIED | no | `docs/handoff.md:498`, `docs/compliance/rfc-ledger.json:860`, `docs/compliance/rfc-ledger.json:1170` |
| `BL-039` | The card verifier does not read a JWK's key_ops (RFC 7517 4.3): a sign-only public key is still used to verify | low | open | VERIFIED | no | `docs/handoff.md:500` |
| `BL-045` | AES-GCM nonce budget is per process: R replicas can seal R x 2^32 under one key; proposed HKDF-derived per-value key is a format change with a two-... | medium | open | VERIFIED | no | `docs/handoff.md:704`, `crates/a2a-protocol-server/src/store/encrypted/cipher.rs:75` |
| `BL-063` | RUSTSEC-2026-0592 (url <= 2.5.8): whether sqlx calls Url::origin() on untrusted input was not checked, so 0.16.0 users' exposure is not known | unstated | open | VERIFIED | no | `docs/handoff.md:44`, `Cargo.lock:4867` |
| `S2` | Skill content pinning: manifests over bodies and resources, deployment pins, hashes in audit evidence, revocation | Product gap | open | VERIFIED | no | `docs/audits/0.16.0-release/findings.json:66`, `ROADMAP.md:120`, `docs/audits/2026-11-12-adversarial-review/findings.json:183` (+2 in JSON) |
| `W2-LIVE-11` | A trusted project skill can take an operator skill's name with a look-alike character | low | open | VERIFIED | no | `ROADMAP.md:120`, `CHANGELOG.md:3247`, `docs/audits/2026-11-07-campaign-wave2/live/README.md:33` |
| `X-18` | The CLI does not say whether it sends usage telemetry or that it redacts credentials | low | open | VERIFIED | no | `docs/audits/0.16.0-release/findings.json:201`, `ROADMAP.md:122`, `docs/audits/2026-11-11-external-review/findings.json:139` (+1 in JSON) |
| `BL-037` | RFC ledger GAP: RFC 8725 3.2, ES256 card signing is hedged with a random nonce, not RFC 6979 deterministic | low | open | AS-DOC | no | `docs/handoff.md:499`, `docs/compliance/rfc-ledger.json:2762` |
| `BL-040` | Standards ledgers not started: RFC 7518, 7517, 8785, then 6750, 9068, 6749; a ledger for RFC 9111 and 5861 (the card cache) | unstated | open | AS-DOC | no | `docs/handoff.md:501` |
| `BL-046` | Privacy residue: erased persons' audit records stay linkable; erase_caller leaves a tenant's shared partition; EncryptedTaskStore leaves ids, state... | unstated | open | AS-DOC | no | `docs/handoff.md:719`, `docs/handoff.md:740`, `CHANGELOG.md:3282` |
| `KL-16` | Network: Host check for non-loopback listeners, Origin check on HTTP, DNS cache, fips feature, CRL and OCSP (B-25, B-28, B-29) | unstated | open | AS-DOC | no | `ROADMAP.md:115`, `CHANGELOG.md:3273`, `docs/audits/0.16.0-release/findings.json:1101` (+2 in JSON) |
| `X-07` | SSRF corpus never run against both SDKs | unstated | open | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:120`, `ROADMAP.md:114`, `docs/audits/2026-11-11-external-review/findings.json:51` |
| `X-19` | Regulated use: crash-time audit consistency, cross-replica erasure, metadata encryption, onward propagation | unstated | open | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:210`, `ROADMAP.md:106`, `docs/audits/2026-11-11-external-review/findings.json:147` (+3 in JSON) |
| `A3` | Required audit: the spool of refused records is in memory; no result record after a tool ran | Operational | partial | VERIFIED | no | `docs/audits/0.16.0-release/findings.json:48`, `ROADMAP.md:106`, `docs/audits/2026-11-12-adversarial-review/findings.json:113` (+2 in JSON) |
| `BL-038` | RFC ledger PARTIAL (8 rows): RFC 7519 4.1.4, 4.1.5, 7.2; RFC 8725 3.1, 3.2, 3.5 (two rows, one medium: HS256 runtime checks length only), 3.8 | low | partial | VERIFIED | no | `docs/handoff.md:499`, `docs/compliance/rfc-ledger.json:1786`, `docs/compliance/rfc-ledger.json:1885` (+7 in JSON) |
| `E1` | Profile::RegulatedData does not check the tool loop step journal; declared controls reported as declared, not verified | Developer experience / release criterion | partial | VERIFIED | yes: maintainer | `docs/audits/0.16.0-release/findings.json:21`, `ROADMAP.md:106`, `docs/handoff.md:208` (+4 in JSON) |
| `SEC-13` | Unchanged: https webhooks are validated by one lookup and connected by another, so a rebinding name still gets a TCP connection and ClientHello to ... | low | partial | VERIFIED | yes: maintainer | `docs/audits/0.16.0-release/findings.json:615`, `docs/audits/2026-11-01-triage/f.json:86`, `ROADMAP.md:110` (+3 in JSON) |
| `B-03` | Card resolver and client fetch_jwks take a rustls config through CardFetchOptions::with_tls_config, and card, JWKS and token provider all read SSL_... | medium | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:309`, `docs/audits/2026-11-01-triage/b.json:6`, `ROADMAP.md:118` |
| `B-13` | An http:// webhook is still delivered to the first resolved public address on every attempt, with no failover across A/AAAA records. | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:336`, `docs/audits/2026-11-01-triage/b.json:147` |
| `B-18` | A strict PEM constructor exists and is tested, but the old DER helper still drops a bad CA silently, and an SSL_CERT_FILE that holds no usable cert... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:354`, `docs/audits/2026-11-01-triage/b.json:204` |
| `B-20` | Trust still differs by path and has got wider: SSL_CERT_FILE reaches every HTTP path and WebSocket but not the gRPC client or OTLP, and gRPC needs ... | medium | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:372`, `docs/audits/2026-11-01-triage/b.json:243`, `ROADMAP.md:118` |
| `B-23` | Unchanged and now shared by every outbound path: NO_PROXY host:port and *.domain entries never match, upper case wins, loopback is not exempt, and ... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:381`, `docs/audits/2026-11-01-triage/b.json:282` |
| `B-27` | All four IPv6 edge cases are unchanged. | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:390`, `docs/audits/2026-11-01-triage/b.json:338` |
| `B-28` | Unchanged: no DNS caching or bound on client transports, an unpooled client per card fetch, and no ndots guidance. | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:399`, `docs/audits/2026-11-01-triage/b.json:356`, `ROADMAP.md:115` |
| `B-29` | Unchanged: ring only on default paths, TLS 1.2+ AEAD suites, no FIPS, CRL or OCSP, and no insecure mode. A custom ClientConfig is the escape hatch ... | info | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:408`, `docs/audits/2026-11-01-triage/b.json:374`, `ROADMAP.md:115` |
| `B-30` | In tls-rustls builds (the SDK default) the push, JWKS/OIDC and model clients now bound the TCP connect and keep connections alive. No test pins it,... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:417`, `docs/audits/2026-11-01-triage/b.json:394` |
| `D-13` | The missing reserved ranges and the trailing-dot bypass are fixed and tested at every edge; https webhooks are still not pinned, so a rebinding res... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:543`, `docs/audits/2026-11-01-triage/d.json:180`, `ROADMAP.md:110` |
| `H2` | On main a private CA and mTLS work on JSON-RPC and HTTP+JSON (ClientBuilder::with_tls_config), WebSocket (WebSocketTransportConfig::with_tls_config... | S1 | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:948`, `docs/audits/2026-11-01-triage/h-s0.json:6`, `ROADMAP.md:118` |
| `NEW-b-2` | Loopback is never exempt from the proxy variables: hyper-util's Matcher consults only NO_PROXY, and Go's net/http exempts localhost and loopback wh... | medium | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:426`, `docs/audits/2026-11-01-triage/b.json:423` |
| `NEW-f-3` | Since c4404e8 push delivery honours HTTPS_PROXY, but the SSRF pre-flight still resolves the webhook host with the local resolver (tokio::net::looku... | medium | partial | AS-DOC | yes: maintainer | `docs/audits/0.16.0-release/findings.json:642`, `docs/audits/2026-11-01-triage/f.json:216`, `ROADMAP.md:110` (+2 in JSON) |
| `NEW-f-4` | The deny-list approach cannot see an operator's own address plan. Multi-label names resolved through Kubernetes search domains (ndots:5: kubernetes... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:651`, `docs/audits/2026-11-01-triage/f.json:225`, `ROADMAP.md:110` |
| `SEC-17` | A custom PushSender (e.g. reqwest, which follows redirects and re-resolves) still gets only the creation-time static check; the requirement is in t... | info | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:633`, `docs/audits/2026-11-01-triage/f.json:159`, `ROADMAP.md:110` |
| `BL-031` | PGP key for security reports: none published | unstated | maintainer-action | VERIFIED | yes: maintainer | `ROADMAP.md:1122`, `SECURITY.md:124`, `docs/lf-readiness-review.md:150` (+1 in JSON) |
| `BL-057` | File GHSA-hr9h-6jvf-wvg6 with RustSec (draft ready; three decisions left to the maintainer) | unstated | maintainer-action | AS-DOC | yes: maintainer | `docs/handoff.md:554`, `docs/upstream/README.md:33`, `docs/handoff.md:761` |
| `BL-007` | Remove the RUSTSEC-2026-0459 waiver when sqlx allows libsqlite3-sys >= 0.39 | unstated | waiting-upstream | VERIFIED | no | `ROADMAP.md:104`, `deny.toml:80`, `osv-scanner.toml:37` (+7 in JSON) |
| `BL-008` | Waivers RUSTSEC-2026-0543 (chrono) and -0555 (chrono-tz): remove each when its path drops the crate | unstated | waiting-upstream | VERIFIED | no | `deny.toml:79`, `osv-scanner.toml:46`, `bindings/a2a-protocol-slimrpc/deny.toml:173` (+5 in JSON) |
| `W2-hostile-5` | Authentication runs after method dispatch, body parsing and version negotiation | low | disclosed-accepted | AS-DOC | no | `ROADMAP.md:126`, `CHANGELOG.md:3240`, `docs/audits/0.16.0-release/findings.json:1002` (+1 in JSON) |
| `BL-060` | Code-scanning alerts on main: OSV alerts 183-186 (golang.org/x/net in ITK Go peers) and the six alerts of 2026-10-25 | unstated | not-verifiable | AS-DOC | no | `docs/handoff.md:539`, `docs/handoff.md:581` |
| `NEW-h-s0-7` | gRPC alone ignores SSL_CERT_FILE/SSL_CERT_DIR. The default ClientTlsConfig::new().with_enabled_roots() with tonic features tls-ring and tls-webpki-... | medium | stale-claim | VERIFIED | no | `docs/audits/0.16.0-release/findings.json:957`, `docs/audits/2026-11-01-triage/h-s0.json:142`, `ROADMAP.md:118` (+1 in JSON) |

### tool-loop (6 rows)

| ID | Title | Severity | Status | Checked | Decision first | Sources |
|---|---|---|---|---|---|---|
| `W2-LIVE-12` | finish_reason content_filter with partial text completes the task unmarked (only length is marked) | polish | open | VERIFIED | no | `ROADMAP.md:134`, `CHANGELOG.md:3247`, `docs/audits/2026-11-07-campaign-wave2/live/README.md:34` (+1 in JSON) |
| `BL-009` | Run the live tool-loop files against a provider other than llama.cpp with Qwen3-1.7B | unstated | open | AS-DOC | no | `ROADMAP.md:139`, `docs/audits/2026-11-14-qualification/README.md:71` |
| `KL-08` | Agent Skills: no hot reload; not run on Windows or macOS; hard-link and FIFO cases | unstated | open | AS-DOC | no | `ROADMAP.md:136`, `CHANGELOG.md:3257`, `docs/audits/0.16.0-release/findings.json:1029` (+1 in JSON) |
| `W2-LIVE-13` | A tool call cut at max_tokens surfaces as the provider's HTTP 500 and fails the task internal | polish | open | AS-DOC | no | `ROADMAP.md:135`, `CHANGELOG.md:3247`, `docs/audits/2026-11-07-campaign-wave2/live/README.md:35` |
| `W2-LIVE-8` | A provider error body reaches the invoke_agent span events and the log, where the book says spans carry no content: redact it or correct the book | low | open | AS-DOC | no | `ROADMAP.md:131`, `CHANGELOG.md:3247`, `docs/audits/0.16.0-release/findings.json:1011` (+2 in JSON) |
| `S1` | Durable step journal: nothing begins an orphaned task again, no compensation, no two-replica test | Product gap | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:57`, `ROADMAP.md:138`, `docs/audits/2026-11-12-adversarial-review/findings.json:173` (+2 in JSON) |

### observability (8 rows)

| ID | Title | Severity | Status | Checked | Decision first | Sources |
|---|---|---|---|---|---|---|
| `BL-010` | Remove init_otlp_pipeline and init_otlp_pipeline_with_endpoint (deprecated 0.16.0, removable no earlier than 0.17.0) | unstated | open | VERIFIED | no | `ROADMAP.md:148`, `crates/a2a-protocol-server/src/otel/pipeline.rs:99`, `crates/a2a-protocol-server/src/otel/pipeline.rs:125` |
| `NEW-c-5` | OTEL_SDK_DISABLED accepts only case-insensitive 'true'; '1', 'yes' or 'on' are silently false. The spec's boolean rule says any value other than tr... | low | open | VERIFIED | no | `docs/audits/0.16.0-release/findings.json:471`, `docs/audits/2026-11-01-triage/c.json:398`, `ROADMAP.md:146` (+1 in JSON) |
| `S4` | Independent visibility of a failed telemetry export; collector-outage and cardinality qualification | Operational | open | VERIFIED | no | `docs/audits/0.16.0-release/findings.json:84`, `ROADMAP.md:144`, `docs/audits/2026-11-12-adversarial-review/findings.json:203` (+1 in JSON) |
| `BL-041` | OpenTelemetry wiring audit, still open: H3 (SLIMRPC multicast span), M6 (WebSocket upgrade refusals not metered), M7, M8, M10 (client retries invis... | unstated | open | AS-DOC | yes: maintainer | `docs/handoff.md:381`, `docs/handoff.md:414` |
| `C-08` | Unrecognised enum values and unparsable numbers in the OTEL_* variables fail build() and every binary exits 2, contrary to the spec's MUST warn-and... | medium | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:444`, `docs/audits/2026-11-01-triage/c.json:67`, `ROADMAP.md:146` |
| `C-17` | The per-call WARN is real and deliberate; it also fires for the SDK's own non-credential A2A-Extensions header, where its advice is wrong. | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:453`, `docs/audits/2026-11-01-triage/c.json:233`, `ROADMAP.md:146` |
| `NEW-c-2` | logs.md says spans are not filtered by level and that with install_subscriber 'every span is exported'; since the C-01 fix spans below TelemetryBui... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:462`, `docs/audits/2026-11-01-triage/c.json:371`, `ROADMAP.md:146` |
| `C-05` | The SDK cannot emit exemplars (upstream opentelemetry_sdk 0.33.1 hard-codes none and ignores OTEL_METRICS_EXEMPLAR_FILTER), and the shipped stack n... | medium | waiting-upstream | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:435`, `docs/audits/2026-11-01-triage/c.json:6`, `ROADMAP.md:150` (+2 in JSON) |

### performance (5 rows)

| ID | Title | Severity | Status | Checked | Decision first | Sources |
|---|---|---|---|---|---|---|
| `X-14` | Throughput gap to 0.14.1 and to a2a-rs: cheaper header-read bound, the timeout around the watch receiver in streaming | unstated | open | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:165`, `ROADMAP.md:154`, `docs/audits/2026-11-11-external-review/findings.json:107` (+3 in JSON) |
| `BL-056` | ADR 0032 HTTP pool: per-origin state (decision 5), the card resolver (decision 6) and client-side load balancing (H12) are not built | unstated | partial | VERIFIED | no | `docs/adr/0032-http-pool.md:7` |
| `E-F18` | One optimistic append point per tenant chain for the whole fleet: each append is at least 2 round trips and every success invalidates the other rep... | medium | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:597`, `docs/audits/2026-11-01-triage/e.json:213`, `ROADMAP.md:157` |
| `H13` | All four sub-claims hold: count-bounded queues with no byte budget, base64 payloads inside the task row, SQL sends still O(history) twice, and an e... | S1 | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:930`, `docs/audits/2026-11-01-triage/h-designs.json:200`, `ROADMAP.md:160` (+2 in JSON) |
| `H5` | Unchanged since the audit: per-child renewal tasks doing a full GetTask every ttl/3, a per-turn watcher reading the lease store every second, and u... | S1 | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:912`, `docs/audits/2026-11-01-triage/h-designs.json:6`, `ROADMAP.md:158` (+2 in JSON) |

### devex (39 rows)

| ID | Title | Severity | Status | Checked | Decision first | Sources |
|---|---|---|---|---|---|---|
| `BL-011` | Remove the names deprecated in 0.15.0 and 0.16.0 that STABILITY.md section 3 now allows removing | unstated | open | VERIFIED | no | `ROADMAP.md:171`, `STABILITY.md:92`, `crates/a2a-protocol-client/src/config.rs:139` (+9 in JSON) |
| `BL-025` | File-length debt: .file-length-baseline holds 82 files over 500 lines (shrink-only ratchet) | unstated | open | VERIFIED | no | `.file-length-baseline:1`, `ROADMAP.md:1077` |
| `BL-054` | ADR 0031 discovery beyond one link: Proposed, nothing built | unstated | open | VERIFIED | no | `docs/adr/0031-discovery-beyond-one-link.md:7` |
| `G-09` | ServeConfig (idle_timeout, header_read_timeout, allowed_hosts) is not applied to a gRPC or a WebSocket listener | medium | open | VERIFIED | no | `ROADMAP.md:170`, `docs/audits/2026-11-01-triage/g-api.json:109`, `CHANGELOG.md:3279` (+3 in JSON) |
| `NEW-g-docs-1` | `without_tls()` is documented as 'plain HTTP only', but only `build()` honours it. `build_grpc()` ignores `config.tls`: an `https://` endpoint gets... | low | open | VERIFIED | no | `docs/audits/0.16.0-release/findings.json:849`, `docs/audits/2026-11-01-triage/g-docs.json:285`, `ROADMAP.md:165` (+3 in JSON) |
| `AA22-N2` | An OAuth2 token endpoint's 429 or 5xx is classed permanent | Low | open | AS-DOC | yes: maintainer | `docs/adopter-audit-2026-09-22.md:41`, `docs/adopter-audit-2026-09-22.md:866` |
| `BL-050` | Small, left as found: five identical examples/*/src/card_url.rs with no sync check; process metrics count their own fd (+1) and assume USER_HZ is 100 | unstated | open | AS-DOC | no | `docs/handoff.md:537` |
| `WT-D-10` | CLI: a 401 or 429 prints status and body only (no WWW-Authenticate, Retry-After or --header-env hint) | low | open | AS-DOC | no | `docs/audits/2026-11-07-adopter-walkthrough/README.md:47` |
| `WT-D-11` | CLI: a message is one argument (128 KiB at most); `a2a send URL -` sends the text "-" | low | open | AS-DOC | yes: maintainer | `docs/audits/2026-11-07-adopter-walkthrough/README.md:48`, `docs/audits/2026-11-07-adopter-walkthrough/README.md:74` |
| `WT-D-12` | CLI: no flag bounds a stream that has started (library's 300 s) | low | open | AS-DOC | yes: maintainer | `docs/audits/2026-11-07-adopter-walkthrough/README.md:49`, `docs/audits/2026-11-07-adopter-walkthrough/README.md:74` |
| `WT-D-13` | CLI: after a stream idle timeout the "card named another URL" hint is wrong | polish | open | AS-DOC | no | `docs/audits/2026-11-07-adopter-walkthrough/README.md:53` |
| `WT-D-17` | REST dispatcher: OPTIONS with no CORS configured is 200 {"status":"ok"}; JSON-RPC answers 204 | polish | open | AS-DOC | no | `docs/audits/2026-11-07-adopter-walkthrough/README.md:55` |
| `WT-D-18` | genai-agent logs three WARNs about its own card at startup | polish | open | AS-DOC | no | `docs/audits/2026-11-07-adopter-walkthrough/README.md:56` |
| `WT-D-9` | CLI: a URL without a scheme exits 1 though the table puts unparseable values under exit 2 | low | open | AS-DOC | no | `docs/audits/2026-11-07-adopter-walkthrough/README.md:46` |
| `B-15` | The CLI still has no TLS or proxy flags, but it now picks up SSL_CERT_FILE/SSL_CERT_DIR and the proxy variables, which covers private-CA and self-s... | low | partial | VERIFIED | no | `docs/audits/0.16.0-release/findings.json:345`, `docs/audits/2026-11-01-triage/b.json:184`, `ROADMAP.md:165` (+2 in JSON) |
| `A13` | Walking README.md:263-267 line by line still fails twice: the Secret `deploy-agent-db` is never created (CreateContainerConfigError), then `deploy-... | medium | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:237`, `docs/audits/2026-11-01-triage/a-l.json:6`, `ROADMAP.md:168` |
| `A15` | Pool size is now settable (DATABASE_POOL_SIZE, `new_with_pool_size`) and the push pool is a fixed 2, so a replica opens size+2 rather than 20. Read... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:246`, `docs/audits/2026-11-01-triage/a-l.json:46`, `ROADMAP.md:168` (+1 in JSON) |
| `A26` | On a many-core node the pod runs one tokio worker per node CPU and jemalloc sizes its arenas from the node's CPUs (and would even under a CPU limit... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:255`, `docs/audits/2026-11-01-triage/a-l.json:181`, `ROADMAP.md:168` (+1 in JSON) |
| `A27` | Every deployed deploy-agent advertises version 0.0.0, so the card cannot tell rollouts apart. | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:264`, `docs/audits/2026-11-01-triage/a-l.json:199`, `ROADMAP.md:168` |
| `B-19` | Unchanged: the gRPC client needs h2 in ALPN, plaintext means h2c prior knowledge, and one connection pins one replica; none of it is documented for... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:363`, `docs/audits/2026-11-01-triage/b.json:225`, `ROADMAP.md:165` |
| `G-06` | Both directions still hold: the SDK's defaults compile the OTLP exporter and tonic for hello-agent (deliberately), and the server crate alone canno... | medium | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:669`, `docs/audits/2026-11-01-triage/g-api.json:50`, `ROADMAP.md:173` |
| `G-07` | All three sub-claims hold: new() yields a card build() refuses, validate() names no index, and streaming is opt-in, so the flagship example cannot ... | medium | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:678`, `docs/audits/2026-11-01-triage/g-api.json:71`, `ROADMAP.md:173` |
| `G-18` | context_id is still a String beside a TaskId, against the types crate's newtype policy. | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:687`, `docs/audits/2026-11-01-triage/g-api.json:259` |
| `G/D-03` | The `with_max_connections` rustdoc still predicts two fields that the struct already has. | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:741`, `docs/audits/2026-11-01-triage/g-docs.json:48`, `ROADMAP.md:165` |
| `G/D-04` | packaging/README.md still says no signing key is committed and lists committing one as step 1; a key is committed and two releases passed the gate. | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:750`, `docs/audits/2026-11-01-triage/g-docs.json:64`, `ROADMAP.md:165` |
| `G/D-05` | The README's exit-code table and `--header` row now match the code; the CLI's own rustdoc does not, the header rule is case-sensitive, and `a2a car... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:759`, `docs/audits/2026-11-01-triage/g-docs.json:85`, `ROADMAP.md:165` |
| `G/D-08` | The server crate's rustdoc quick start still tells users to wire a dispatcher into their own hyper server instead of `Server`/`serve`. | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:786`, `docs/audits/2026-11-01-triage/g-docs.json:146`, `ROADMAP.md:165` |
| `G/D-09` | The rustdoc lists 'HTTP connection/transport errors' as retryable while `ClientError::Transport` is explicitly not; the variant's own doc still cal... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:795`, `docs/audits/2026-11-01-triage/g-docs.json:163`, `ROADMAP.md:165` |
| `G/D-10` | The example is ~106 lines (52 of code) above its tests, a third of main() is telemetry the README does not mention, and it relies on the SDK's defa... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:804`, `docs/audits/2026-11-01-triage/g-docs.json:180`, `ROADMAP.md:165` |
| `G/D-11` | Both the prelude rustdoc and the SDK README's 'Prelude Contents' list a subset of what the prelude exports, and `ServeConfig` is not in the prelude... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:813`, `docs/audits/2026-11-01-triage/g-docs.json:200`, `ROADMAP.md:165` |
| `NEW-a-l-1` | deploy-agent never purges terminal tasks. With DATABASE_URL set, retention silently changes from the in-memory default (10,000 tasks, 1 h TTL) to f... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:273`, `docs/audits/2026-11-01-triage/a-l.json:239`, `ROADMAP.md:168` |
| `NEW-a-l-3` | Every Secret recipe uses `sslmode=verify-full&sslrootcert=/path/ca.pem` and says 'mount it there', but the manifest has no volumes/volumeMounts and... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:282`, `docs/audits/2026-11-01-triage/a-l.json:257`, `ROADMAP.md:168` |
| `NEW-a-l-4a` | No CI job builds the deploy-agent image or validates the manifest (no docker build of this Dockerfile, no kubeconform/kube-linter/PSS check). L18 r... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:291`, `docs/audits/2026-11-01-triage/a-l.json:266` |
| `NEW-a-l-4b` | The Dockerfile pins rust:1.94-bookworm and debian:bookworm-slim by digest, and nothing refreshes those digests (no Dependabot docker ecosystem, pin... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:300`, `docs/audits/2026-11-01-triage/a-l.json:275`, `ROADMAP.md:168` |
| `NEW-g-api-1` | The gRPC channel is built with Endpoint::timeout(request_timeout) (30 s default, builder transport_factory.rs:208). tonic's client stack wraps ever... | medium | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:705`, `docs/audits/2026-11-01-triage/g-api.json:366` |
| `NEW-g-api-2` | G-02 was fixed for Server only (serve/graceful/mod.rs refuses max_connections 0 and clamps to Semaphore::MAX_PERMITS at :326-327). GrpcDispatcher::... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:714`, `docs/audits/2026-11-01-triage/g-api.json:376`, `ROADMAP.md:173` |
| `NEW-g-api-4` | STABILITY.md section 1 says the crates are at 0.15.0, 'the newest release' (0.15.1 shipped 2026-10-26), and promises that sign_agent_card 'is remov... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:723`, `docs/audits/2026-11-01-triage/g-api.json:394` |
| `NEW-g-docs-5` | The shrink-only ratchet on ignored book blocks matches only fences at column 0. An indented fence (inside a list item, which rustdoc still treats a... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:885`, `docs/audits/2026-11-01-triage/g-docs.json:321`, `ROADMAP.md:165` |
| `NEW-g-docs-2` | `a2a card <URL>` passes no GlobalOpts: it fetches with `resolve_agent_card(url)` / `fetch_card_from_url(url)`, so `--header`, `--timeout` and `--te... | low | stale-claim | VERIFIED | no | `docs/audits/0.16.0-release/findings.json:858`, `docs/audits/2026-11-01-triage/g-docs.json:294`, `ROADMAP.md:165` (+3 in JSON) |

### docs-book (26 rows)

| ID | Title | Severity | Status | Checked | Decision first | Sources |
|---|---|---|---|---|---|---|
| `BL-021` | docs/readiness-bar.md says "F1 is measured" in one section and lists F1 as not measured in two others: make them agree | low | open | VERIFIED | no | `ROADMAP.md:251`, `docs/handoff.md:55`, `docs/readiness-bar.md:181` (+3 in JSON) |
| `BL-026` | Book ignore debt: .book-ignore-baseline holds 1 entry (book/src/bindings/slimrpc.md, 4 ignored blocks) | unstated | open | VERIFIED | no | `.book-ignore-baseline:1`, `docs/handoff.md:453` |
| `BL-012` | One book page that lists the Known Limitations by id, linked from the changelog | unstated | open | AS-DOC | no | `ROADMAP.md:180` |
| `BL-013` | Three documents repeat a sentence the code has outgrown: ADR 0057, ADR 0035, the doc comment of dispatch/websocket/serve_config.rs | unstated | open | AS-DOC | no | `ROADMAP.md:184` |
| `BL-022` | Antipattern classes to remove: present-tense moments, underived counts in prose, multi-id status rows, registers with no status field, limitation r... | unstated | open | AS-DOC | no | `ROADMAP.md:255` |
| `WT-D-16` | Dead anchors and links in design records (plan.md 11, spec/slimrpc_v1 1, vendored spec 10) | polish | open | AS-DOC | no | `docs/audits/2026-11-07-adopter-walkthrough/README.md:54` |
| `WT-D-6` | Server and SDK crate READMEs: hidden "# " lines show on crates.io; SDK Quick Start has no dependency line or fn main | polish | open | AS-DOC | no | `docs/audits/2026-11-07-adopter-walkthrough/README.md:52` |
| `G/C-02` | Nothing stops a breaking behaviour change in a patch: semver-checks sees only API, and the release gate keys on the words '### Breaking Changes', w... | medium | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:831`, `docs/audits/2026-11-01-triage/g-docs.json:240` |
| `G/C-03` | Book coverage is near-total, but 14 rustdoc examples (EventEmitter's among them), the slimrpc README's five blocks and three example-README blocks ... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:840`, `docs/audits/2026-11-01-triage/g-docs.json:262`, `ROADMAP.md:188` |
| `G/D-02` | Two READMEs (server crate README, which is its crates.io page, and the root README) still say `serve` has hyper's defaults / no limits / no idle ti... | medium | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:732`, `docs/audits/2026-11-01-triage/g-docs.json:26` |
| `G/D-06` | The book says both dispatchers return `ServeReport`; gRPC returns `io::Result<ServeReport>`. | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:768`, `docs/audits/2026-11-01-triage/g-docs.json:107`, `ROADMAP.md:188` |
| `G/D-07` | The hidden card on the Axum, REST, gRPC and WebSocket dispatcher pages advertises only JSON-RPC while the page serves another binding; a reader who... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:777`, `docs/audits/2026-11-01-triage/g-docs.json:125`, `ROADMAP.md:188` |
| `NEW-g-docs-3` | Says '`BIND-EQUIV-004` is graded **structurally only** ... Proving each binding *enforces* those schemes identically needs a target configured to r... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:867`, `docs/audits/2026-11-01-triage/g-docs.json:303` |
| `NEW-g-docs-6` | 'TLS verification is the library's, against its bundled roots, with no way to turn it off here.' Since 57f034e the library's default (crates/a2a-pr... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:894`, `docs/audits/2026-11-01-triage/g-docs.json:330` |
| `NEW-g-docs-7` | When a release section exceeds 120,000 characters the GitHub release body keeps only '### (Known Limitations\|Breaking Changes\|Deprecated\|Security)'... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:903`, `docs/audits/2026-11-01-triage/g-docs.json:339` |
| `G/C-01` | The CHANGELOG is 806 KB and growing; [Unreleased] alone is 74 KB with ~100-word bullets, internal notes and lab measurements, and no release has a ... | medium | decision | VERIFIED | yes: maintainer | `docs/audits/0.16.0-release/findings.json:822`, `docs/audits/2026-11-01-triage/g-docs.json:218`, `ROADMAP.md:190` (+2 in JSON) |
| `AA22-unmarked` | adopter-audit-2026-09-22: 41 table rows carry no [Fixed] marker (O3, O6-O9, O12, O14-O16, S2, S4, S5, S10, S11, S13-S16, C7, C12-C14, C17-C21, T1, ... | High | stale-claim | VERIFIED | no | `docs/adopter-audit-2026-09-22.md:1029`, `docs/adopter-audit-2026-09-22.md:936`, `docs/adopter-audit-2026-09-22.md:679` (+3 in JSON) |
| `DOC-01` | ROADMAP "Signed tags" still says "No key is committed yet, so the next release is blocked" | unstated | stale-claim | VERIFIED | no | `ROADMAP.md:1117`, `.github/release-signers/allowed_signers:4`, `SECURITY.md:140` |
| `DOC-02` | docs/lf-readiness-review.md section 4 item 1 says there is no dependency-update automation | unstated | stale-claim | VERIFIED | no | `docs/lf-readiness-review.md:125`, `.github/dependabot.yml:1` |
| `DOC-03` | docs/openssf-best-practices.md vulnerabilities_fixed_60_days row says "with no active waiver" | unstated | stale-claim | VERIFIED | no | `docs/openssf-best-practices.md:106`, `deny.toml:78` |
| `DOC-04` | docs/swarm-orchestration.md: "A SQLite journal is not built"; "Next, in order" 2 still says "G3 - decide" though G3 was decided 2026-10-02 | unstated | stale-claim | VERIFIED | no | `docs/swarm-orchestration.md:70`, `docs/swarm-orchestration.md:309`, `docs/swarm-orchestration.md:83` (+1 in JSON) |
| `DOC-05` | docs/swarm-scale-findings.md Finding 8 ("the single-writer refusal does not cross replicas") is superseded by turn claims | unstated | stale-claim | VERIFIED | no | `docs/swarm-scale-findings.md:601`, `crates/a2a-protocol-server/tests/multi_replica.rs:579`, `docs/swarm-orchestration.md:285` |
| `DOC-06` | docs/handoff.md is behind HEAD: "Now" describes #220 and #222 as open branches; "Next, in order" 6 and line 796 say the benchmark harness pins =0.14.1 | unstated | stale-claim | VERIFIED | no | `docs/handoff.md:84`, `docs/handoff.md:449`, `docs/handoff.md:796` (+6 in JSON) |
| `DOC-07` | Wave-2 live campaign README still lists W2-LIVE-3, -4, -5 and -10 as "not fixed" | unstated | stale-claim | VERIFIED | no | `docs/audits/2026-11-07-campaign-wave2/live/README.md:25`, `docs/audits/2026-11-07-campaign-wave2/live/README.md:32`, `docs/handoff.md:480` (+1 in JSON) |
| `DOC-08` | docs/audits/2026-11-01-triage/DECISIONS.md fork table (2026-11-02) says F7 HttpPool, ADR 0035, ADR 0036 "not built" | unstated | stale-claim | VERIFIED | no | `docs/audits/2026-11-01-triage/DECISIONS.md:25`, `docs/audits/2026-11-01-triage/DECISIONS.md:36`, `docs/adr/0032-http-pool.md:7` (+1 in JSON) |
| `NEW-g-docs-4` | STABILITY.md §1 says 'The crates are at `0.15.0` (2026-10-25), the newest release' although 0.15.1 shipped 2026-10-26 and every crate's Cargo.toml ... | low | stale-claim | VERIFIED | no | `docs/audits/0.16.0-release/findings.json:876`, `docs/audits/2026-11-01-triage/g-docs.json:312`, `STABILITY.md:15` |

### release-eng (10 rows)

| ID | Title | Severity | Status | Checked | Decision first | Sources |
|---|---|---|---|---|---|---|
| `BL-015` | spec-ledger.json cites CHANGELOG.md by line number, so every release preparation breaks check_spec_ledger.py: cite the heading | unstated | open | VERIFIED | no | `ROADMAP.md:201`, `docs/handoff.md:101`, `docs/compliance/spec-ledger.json:76` (+1 in JSON) |
| `BL-016` | check_provenance_manifest.py gave a wrong answer once (pass/nosignoff off by one): find the cause | unstated | open | VERIFIED | no | `ROADMAP.md:204`, `scripts/provenance_manifest.sh:26`, `scripts/provenance_manifest.sh:86` |
| `BL-019` | release.yml's ci job is a reduced copy of ci.yml and nothing compares the two | unstated | open | VERIFIED | no | `ROADMAP.md:237`, `.github/workflows/release.yml:326`, `.github/workflows/release.yml:51` |
| `S6` | Release evidence manifest runs no test suite and is not signed on its own | Governance | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:102`, `ROADMAP.md:229`, `docs/audits/2026-11-12-adversarial-review/findings.json:223` |
| `BL-014` | ci_evidence.py refuses a release when any ci.yml job failed, and Nightly (informational) is continue-on-error on a floating toolchain | unstated | decision | VERIFIED | yes: maintainer | `ROADMAP.md:196`, `scripts/ci_evidence.py:44`, `.github/workflows/ci.yml:909` |
| `X-16` | Publish the SLIMRPC binding (first publish) and build it on its declared MSRV | unstated | decision | VERIFIED | yes: maintainer | `docs/audits/0.16.0-release/findings.json:183`, `ROADMAP.md:175`, `ROADMAP.md:225` (+4 in JSON) |
| `X-17` | Whether and how to distribute the a2a CLI (crates.io, release binaries, Homebrew, winget) | unstated | decision | VERIFIED | yes: maintainer | `docs/audits/0.16.0-release/findings.json:192`, `ROADMAP.md:175`, `ROADMAP.md:1383` (+3 in JSON) |
| `BL-017` | Decide: an absolute coverage floor in codecov.yml | unstated | decision | AS-DOC | yes: maintainer | `ROADMAP.md:227` |
| `BL-018` | Decide: pin the toolchain a session and CI lint with (session image 1.97.0 vs CI 1.100.0; their clippy disagreed) | unstated | decision | AS-DOC | yes: maintainer | `ROADMAP.md:231`, `docs/handoff.md:96` |
| `BL-033` | A v* tag ruleset in the repository settings is unset | unstated | maintainer-action | AS-DOC | yes: maintainer | `ROADMAP.md:1119`, `docs/handoff.md:577`, `.github/release-signers/README.md:1` |

### testing-verification (16 rows)

| ID | Title | Severity | Status | Checked | Decision first | Sources |
|---|---|---|---|---|---|---|
| `BL-020` | statement_count.rs counts 0 statements when PostgreSQL has SSL on: refuse a server that accepts the SSLRequest | low | open | VERIFIED | no | `ROADMAP.md:246`, `docs/handoff.md:67`, `docs/handoff.md:311` (+1 in JSON) |
| `BL-023` | mutants.toml is not read by cargo-mutants (it looks for .cargo/mutants.toml): generated proto code is mutated and the full sweep has no timeout cap | unstated | open | VERIFIED | no | `ROADMAP.md:966`, `mutants.toml:110`, `mutants.toml:162` |
| `CI2` | blocking_over_stream_e2e a_send_carried_over_a_stream_crosses_the_same_proxy tolerates only a 500 ms machine pause | unstated | open | VERIFIED | no | `docs/audits/0.16.0-release/findings.json:1164`, `ROADMAP.md:215`, `crates/a2a-protocol-client/tests/blocking_over_stream_e2e.rs:67` (+1 in JSON) |
| `AA22-OW11` | The coordinator end-to-end test against a2a-go that phase 1 promised and did not build | Medium | open | AS-DOC | no | `docs/adopter-audit-2026-09-22.md:918` |
| `BL-006` | Qualify what the qualification matrix lists as not covered: multi-host network, SQLite under faults, disk full, OOM kills, clock skew, slow databas... | unstated | open | AS-DOC | no | `ROADMAP.md:72`, `docs/audits/2026-11-14-qualification/README.md:61`, `docs/handoff.md:316` |
| `BL-024` | Raise coverage on the weak files: handler/event_processing/background/mod.rs (54.2%), serve.rs (67.5%), background/push_delivery.rs (72.8%) | unstated | open | AS-DOC | no | `ROADMAP.md:1072` |
| `BL-029` | examples/resilient-agent is not run in CI with a database (example-surface runs the older examples only) | unstated | open | AS-DOC | no | `ROADMAP.md:646` |
| `BL-030` | prove_gates_fail.sh: no complete sweep has been run against the current gate set | unstated | open | AS-DOC | no | `ROADMAP.md:1148`, `docs/handoff.md:784` |
| `BL-044` | Mutation testing: read the next scheduled full sweep's logs; whether the incremental run at b1c4a84d was clean was not known | unstated | open | AS-DOC | no | `docs/handoff.md:436`, `docs/handoff.md:118`, `ROADMAP.md:954` |
| `BL-049` | Gates: a mutation shard that starts after its PR merges grades nothing; the Windows stress-test connect failure has no known cause; tck-self-test's... | low | open | AS-DOC | no | `docs/handoff.md:794`, `docs/handoff.md:296` |
| `CI1` | Two macOS mDNS discovery tests failed on main once; not reproduced, not root-caused | High (release gate) | open | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:12`, `ROADMAP.md:70`, `docs/handoff.md:124` (+2 in JSON) |
| `X-15` | the production-readiness measurement matrix is incomplete | unstated | open | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:174`, `ROADMAP.md:78` |
| `S5` | Measure the unmeasured readiness-bar rows (R5-R7, F1-F5, L2, L5), a second replica, PostgreSQL/SQLite, other bindings | Evidence | partial | VERIFIED | no | `docs/audits/0.16.0-release/findings.json:93`, `ROADMAP.md:78`, `docs/audits/2026-11-12-adversarial-review/findings.json:213` (+5 in JSON) |
| `BL-027` | Soak: sustained traffic over hours (rotation under load, RSS and queue growth, store eviction at steady state) | unstated | partial | AS-DOC | no | `ROADMAP.md:577` |
| `E-F26` | SQLite claims use the host wall clock, so a forward step past the TTL (NTP step, VM resume) makes live claims look expired until the next renewal, ... | low | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:606`, `docs/audits/2026-11-01-triage/e.json:375`, `ROADMAP.md:72` |
| `N14` | Migration rehearsal incomplete: PostgreSQL 0.15->0.16 for audit and push-config stores, PostgreSQL rollback, rolling two-version deployment | Medium | partial | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:3`, `ROADMAP.md:72`, `docs/handoff.md:207` (+2 in JSON) |

### upstream (6 rows)

| ID | Title | Severity | Status | Checked | Decision first | Sources |
|---|---|---|---|---|---|---|
| `BL-062` | In flight outside this repository: adk-rust patch needs porting from 0.12 to 0.15+; rig #391 comment drafted, not posted; AutoAgents #52 unanswered... | unstated | open | AS-DOC | yes: maintainer | `docs/handoff.md:639`, `docs/handoff.md:966` |
| `H19` | Trace context crosses non-Rust hops by standard headers; lease, budget, idempotency, failure class and approval are ignored by non-Rust peers; halt... | S1 | partial | AS-DOC | yes: maintainer | `docs/audits/0.16.0-release/findings.json:939`, `docs/audits/2026-11-01-triage/h-designs.json:316`, `ROADMAP.md:99` (+4 in JSON) |
| `BL-058` | Upstream drafts not sent (13) and held (3): see docs/upstream/README.md | unstated | maintainer-action | VERIFIED | yes: maintainer | `docs/handoff.md:600`, `ROADMAP.md:53`, `docs/upstream/README.md:18` (+2 in JSON) |
| `AA22-OW13` | a2a-go's client loses JSON-RPC streaming pre-stream errors (S2) | Medium | waiting-upstream | AS-DOC | no | `docs/adopter-audit-2026-09-22.md:955`, `docs/adopter-audit-2026-09-22.md:1055` |
| `BL-034` | Listing on the ITK dashboard waits on upstream changes (draft not sent) | unstated | waiting-upstream | AS-DOC | yes: maintainer | `ROADMAP.md:1370`, `docs/upstream/README.md:27` |
| `BL-061` | Private advisory to a2a-python is open: file nothing public about it | unstated | waiting-upstream | AS-DOC | no | `docs/handoff.md:604` |

### governance (12 rows)

| ID | Title | Severity | Status | Checked | Decision first | Sources |
|---|---|---|---|---|---|---|
| `BL-053` | ADR 0030 delegation resume and journal: still "Proposed" though built (resume, reattach, in-memory, PostgreSQL and SQLite journals) | unstated | open | VERIFIED | no | `docs/adr/0030-delegation-resume-and-journal.md:7`, `docs/swarm-orchestration.md:70`, `docs/swarm-orchestration.md:316` |
| `BL-065` | STABILITY section 7: 1.0 needs two consecutive minors with no break; 0.13-0.16 each broke, so the count is zero | unstated | open | VERIFIED | no | `STABILITY.md:204`, `STABILITY.md:23` |
| `E6` | Three Scorecard alerts remain: Code-Review #126, Branch-Protection #93 (need G1), Vulnerabilities #125 (RUSTSEC-2026-0459 waiver) | Release criterion | partial | AS-DOC | yes: maintainer | `docs/audits/0.16.0-release/findings.json:30`, `ROADMAP.md:124`, `docs/handoff.md:209` (+1 in JSON) |
| `BL-004` | Decide: is a field a specification revision adds a minor or a patch addition | unstated | decision | VERIFIED | yes: maintainer | `ROADMAP.md:50`, `book/src/reference/upgrading/0.15-to-0.16.md:220`, `STABILITY.md:127` (+1 in JSON) |
| `BL-051` | Decide ADR 0028: whether Tokio stays mandatory on wasm32-unknown-unknown (qualify ADR 0003 so a runtime seam can land as phase 1) | unstated | decision | VERIFIED | yes: maintainer | `docs/handoff.md:544`, `docs/adr/0028-webassembly-and-cloudflare-workers.md:7` |
| `BL-052` | ADR 0029 local network discovery: built and tested, crate not published; whether and under what name is undecided | unstated | decision | VERIFIED | yes: maintainer | `docs/adr/0029-local-network-discovery.md:7` |
| `BL-064` | Handoff "First, next" 2: Phase 2 target sheet (scale, failure detection and recovery, latency, memory, cold start, telemetry cardinality) and a pla... | unstated | decision | AS-DOC | yes: maintainer | `docs/handoff.md:88` |
| `X-13` | MSRV 1.94 against 1.85 (a2a-rs) | unstated | decision | AS-DOC | yes: maintainer | `docs/audits/0.16.0-release/findings.json:156`, `ROADMAP.md:175`, `docs/audits/2026-11-11-external-review/findings.json:99` (+1 in JSON) |
| `BL-032` | Register a2a-rust.dev so conduct@ and security@ addresses can be restored | unstated | maintainer-action | AS-DOC | yes: maintainer | `ROADMAP.md:1140` |
| `BL-059` | Delete two redundant remote branches (claude/inspiring-mccarthy-rgpntj-304-merge, -standards-ledger) | unstated | maintainer-action | AS-DOC | yes: maintainer | `docs/handoff.md:487` |
| `G1` | main is unprotected; the protect-main ruleset is disabled | Governance | maintainer-action | AS-DOC | yes: maintainer | `docs/audits/0.16.0-release/findings.json:111`, `ROADMAP.md:124`, `docs/audits/2026-11-12-adversarial-review/findings.json:253` (+2 in JSON) |
| `X-21` | Single-maintainer concentration | unstated | disclosed-accepted | AS-DOC | no | `docs/audits/0.16.0-release/findings.json:219`, `docs/audits/2026-11-11-external-review/findings.json:163`, `ROADMAP.md:1136` |

### Notes on the rows that were checked against code

- **`E1`** (partial): VERIFIED open: the journal is the executor's, the builder never sees it. Decision sentence (handoff:208): "E1's rest is a decision first: whether the step journal's durability should be checked, which needs the builder to see the ToolLoop's journal." Part of KL-22.
- **`A3`** (partial): VERIFIED open: audit/mod.rs:55 documents "a process that dies with records in its spool loses them". ROADMAP item: a durable audit spool. Part of KL-22.
- **`S2`** (open): VERIFIED open: no digest, manifest or pin code under crates/a2a-protocol-server/src/skills (git grep finds only the spec-snapshot date).
- **`S3`** (open): VERIFIED open: no fleet quota type in crates/*/src. Decision sentence (ROADMAP:95-96): "S3, a fleet-wide inference quota, needs a decision first."
- **`S4`** (open): VERIFIED not examined: the register note is "Not examined this session". A lost-export counter (a2a.telemetry.export.failed) exists in counting.rs:101-116, but it is reported through the same pipeline, so "independent" visibility and the qualification run remain to be established.
- **`S5`** (partial): VERIFIED: the machine-readable block lists R5,R6,R7,F1-F5,L2,L5, any second replica, any SQL store, any binding but JSON-RPC as not measured. Aliases: X-15. Needs an idle machine (handoff:64). Handoff "First, next" 2 makes this Phase 2: a target sheet and plan for the maintainer's approval before building. See contradiction C-01 (F1).
- **`X-10`** (open): VERIFIED open at HEAD: persisting() gives up after store_write_retry (30 s), the status is reverted in memory and Outcome::StoreFailed returned; nothing re-queues it. Register README names it the row closest to blocking 0.16.0. Candidates recorded (neither built): a breaker after one write used the whole budget, and a direct store write of the failure (handoff:691-693). Aliases: KL-20, ROADMAP antipattern 7.
- **`E-F25`** (open): VERIFIED open at HEAD: cancel_task.rs:147-163 writes save_status_delta(Canceled) after a throwaway writer; no event-log append. Aliases: X-11, KL-02. Severity "low" is from the 2026-11-01 triage (e.json:354); the 2026-11-11 review states none. ADR 0057 names the step; ROADMAP:184 says ADR 0057 itself repeats a stale sentence ("the reaper writes no event").
- **`X-16`** (decision): VERIFIED in tree: version 0.7.0, publish = true; crates.io state is as documented (handoff:26 "does not exist"). Decision sentence (ROADMAP:175-176): "[decision] ... publishing the SLIMRPC binding (X-16) and the CLI (X-17)". ROADMAP:225 lists the publish as [0.16.x] work at the same time.
- **`X-17`** (decision): VERIFIED: tools/a2a-cli is publish = false. Binaries wait on vars.PUBLISH_CLI_BINARIES. "The maintainer decided on 2026-10-08 that this is theirs to turn on" (ROADMAP:1387).
- **`X-18`** (open): VERIFIED partly open: the CLI README has a Telemetry section (OTLP only when OTEL_EXPORTER_OTLP_ENDPOINT is set; "Unset ... nothing is sent"), but no explicit no-usage-telemetry statement and no redaction statement were found there.
- **`X-22`** (decision): VERIFIED: itk.yml and itk-nightly.yml float "main"; acts.yml pins a commit. Decision sentence (ROADMAP:46-47): "Decide whether itk.yml pins a revision (X-22)."
- **`B-15`** (partial): VERIFIED the finding text is stale: the CLI has --cacert, --client-cert and --client-key. Remaining part, if any, is not stated in ROADMAP. Triage severity as written: "low".
- **`NEW-c-5`** (open): VERIFIED as described: sdk_disabled() accepts only case-insensitive "true". Triage severity as written: "low".
- **`D-08`** (partial): VERIFIED the finding text is stale in part: the WebSocket client pings by default (DEFAULT_PING_INTERVAL 15 s). ROADMAP lists it as "only part fixed" without saying which part is left. Triage severity as written: "medium (unchanged)".
- **`E-F12`** (partial): VERIFIED: the per-replica scope is documented in the rustdoc; fleet-wide enforcement is not built ([0.17], with H7, H9, H17). Triage severity as written: "medium".
- **`SEC-13`** (partial): VERIFIED: direct https and http deliveries are pinned to the checked addresses (the register's finding text is stale). Open: no pin behind a proxy. Fork F16 was "not chosen 2026-11-03: stays deferred and documented" (DECISIONS.md:33), so the proxy boundary is undecided. Triage severity as written: "low (unchanged); KNOWN".
- **`G-04`** (partial): VERIFIED the stated finding is fixed: build_grpc selects the card's gRPC interface (select_grpc_interface) and refuses a card with none. What ROADMAP:97-98 still counts as "only part fixed" for G-04 is not stated there. Triage severity as written: "medium (unchanged)".
- **`G/C-01`** (decision): VERIFIED size: CHANGELOG.md is 1,007,269 bytes and 14,411 lines at HEAD; the 0.16.0 section spans lines 36-3289. Decision sentence (ROADMAP:190): "[decision] The size of a release's changelog section (G/C-01)". Triage severity as written: "medium (unchanged; the file grew)".
- **`NEW-g-docs-1`** (open): VERIFIED open: the HTTP factories refuse https with TlsConfig::Disabled; grpc_transport_config() reads only grpc_tls_config, not config.tls. Triage severity as written: "low".
- **`NEW-g-docs-2`** (stale-claim): VERIFIED fixed at HEAD: `a2a card` takes GlobalOpts and fetches with fetch_options(opts, true) (headers, timeout, TLS flags). Stale: ROADMAP.md:167 and register row. --tenant was not checked (a card fetch carries no tenant). Triage severity as written: "low".
- **`NEW-g-docs-4`** (stale-claim): VERIFIED closed: STABILITY.md section 1 names 0.16.0. The register row's own evidence says "Closed by this pull request" while its status reads "open": the register disagrees with itself. Triage severity as written: "low".
- **`NEW-h-s0-7`** (stale-claim): VERIFIED fixed at HEAD for the gRPC client: default_client_tls() adds crate::tls::env_roots_pem() (SSL_CERT_FILE/SSL_CERT_DIR) under tls-rustls. Stale: ROADMAP.md:118-119, register row, and the SSL_CERT_FILE clause of B-20/H2. Not checked: OTLP, token provider TLS options (B-03, H2 remainder). Triage severity as written: "medium".
- **`CI2`** (open): VERIFIED open: keep-alive 100 ms against a proxy that cuts at 600 ms is unchanged. Constraint in ROADMAP: fix without widening the window or retrying. Test is in the client crate (the register does not give a path).
- **`W2-LIVE-11`** (open): VERIFIED open: no confusable/normalisation check under crates/a2a-protocol-server/src/skills. Part of KL-06. [0.17] with S2.
- **`W2-LIVE-12`** (open): VERIFIED open: answer.rs marks only FINISH_REASON_LENGTH; content_filter is handled only for an empty answer (loop_error.rs:98). [0.17].
- **`G-09`** (open): VERIFIED as documented: the gRPC with_serve_config rustdoc lists the fields it does not apply. [0.17]. Alias: KL-19.
- **`BL-001`** (partial): VERIFIED count: `python3 scripts/check_spec_ledger.py` prints PARTIAL 4 of 510. handoff:366: "cannot all be closed from this repository": 12.4 waits on agntcy-slim-rpc (upstream), 5.7 is a contradiction in the spec (upstream draft held), 3.5.2 is a design limit (streamLagged past the ceiling), 7.6.1 "means an executor mailbox, an ADR and a decision".
- **`BL-002`** (open): VERIFIED: tck/untested-musts.toml has 22 [[requirement]] entries and its header still says "Measured against a2a-tck at 263b9cf: 21 of them" while the pin is 5e6f37c with 16 not tested + 7 manual + 5 skipped = 28 ungraded. Also: ROADMAP:1319 tracks 13 upstream backlog items.
- **`BL-004`** (decision): VERIFIED contradiction at HEAD: the upgrade guide says "minor-version addition"; STABILITY.md section 4 says "patch-level addition"; README says "patch releases". Decision sentence: ROADMAP:50 "[decision]".
- **`BL-007`** (waiting-upstream): VERIFIED still needed: Cargo.lock holds libsqlite3-sys 0.37.0 under sqlx 0.9.0. osv-scanner ignoreUntil = 2027-02-11 (a hard re-examination date). Alias: KL-15, E6 alert #125.
- **`BL-008`** (waiting-upstream): VERIFIED present. Unmaintained notices, no fix. osv-scanner ignoreUntil = 2027-02-17. Reached only by examples (rmcp, schemars) and the SLIM stack.
- **`BL-010`** (open): VERIFIED present with #[deprecated(since = "0.16.0")]. A break: [0.17].
- **`BL-011`** (open): VERIFIED counts at HEAD: 34 #[deprecated] attributes in *.rs; of those with a since on a following line, 5 are since 0.15.0 (removable now) and 26 since 0.16.0 (removable from 0.17.0). 45 #[allow(deprecated)] in 30 files go with them. handoff:530: there is no gate that deprecation notes carry a removal version (ADR 0053). Blocks 1.0: each removal is a break and restarts STABILITY section 7's two-unbroken-minors count.
- **`BL-014`** (decision): VERIFIED open: BAD = {failure, cancelled, timed_out, ...} with no exemption; the nightly job is continue-on-error: true. Decision sentence (ROADMAP:199-200): "Either the gate skips jobs ci.yml marks continue-on-error, or the nightly job stops being able to fail." Blocks: any tag cut while nightly is red.
- **`BL-015`** (open): VERIFIED open: one citation, CHANGELOG.md:13311 (the 0.5.0 heading today). The same antipattern is in docs/audits/0.16.0-release/findings.json: 91 CHANGELOG.md:<line> strings, and its KL rows cite 3201-3261 where the entries now sit at 3222-3282.
- **`BL-016`** (open): VERIFIED the suspected construct is present: `git log -1 --format=%B "$sha" | grep -Eqi` under `set -uo pipefail`. Cause "Suspected, not shown". Until fixed, re-running the job at the tag is the recorded remedy.
- **`BL-019`** (open): VERIFIED open: release.yml has its own ci job; no workflow has `workflow_call` for ci.yml. The one known divergence (CARGO_PROFILE_DEV_DEBUG) is fixed at release.yml:51. Jobs after ci run only on a tag.
- **`BL-020`** (open): VERIFIED open: the proxy only recognises SSLRequest (80877103) and forwards it. Severity "low" from handoff:67. Workaround: PGSSLMODE=disable.
- **`BL-021`** (open): VERIFIED open. See contradiction C-01. The measured-limits block is copied into the release evidence manifest, so the block is the one a release asserts.
- **`BL-023`** (open): VERIFIED open: no .cargo/ directory exists; mutants.toml is at the root and still carries cap_timeout (an unknown field). "Deliberately not fixed for 0.8"; the recipe is at ROADMAP:997-1003. Changes what the sweep measures, so it needs a sweep to land on.
- **`BL-025`** (open): VERIFIED: 82 non-comment entries. ROADMAP:1077 still says handler/helpers.rs is 612 lines and "77 of 310" files exceed; helpers.rs is 961 lines at HEAD.
- **`BL-026`** (open): VERIFIED: 1 non-comment entry. Tied to X-16: the page shows a path dependency until the binding is published.
- **`BL-031`** (maintainer-action): VERIFIED: SECURITY.md:124 "Not available."
- **`BL-035`** (open): VERIFIED by jq at HEAD: 45 IMPLEMENTED_UNTESTED, 4 of them severity high. A security row is TESTED only with a recorded mutant a test killed. "no decision needed" (handoff:495).
- **`BL-036`** (open): VERIFIED open: no disjointness check found under crates/a2a-protocol-types/src/signing*.
- **`BL-038`** (partial): VERIFIED count by jq: 8 PARTIAL (7 low, 1 medium: 3.5 line 372). The 3.5 runtime was left unchanged by the maintainer's decision of 2026-11-06 (handoff:838-840). The one DEVIATION (RFC 7519 section 8, `none`, rfc-ledger.json:2314) names ADR 0010 and is by design.
- **`BL-039`** (open): VERIFIED open: `git grep key_ops -- crates/a2a-protocol-types/src` finds nothing. RFC 7517 is not in the ledger.
- **`BL-045`** (open): VERIFIED as documented: "The budget is enforced per cipher" (cipher.rs:75); the book tells deployers to split the limit. "it waits" (handoff:711).
- **`BL-051`** (decision): VERIFIED status line: "Proposed ... The decision it asks for (qualifying ADR 0003) is the maintainer's." Phases 2-4 unbuilt. If declined, nothing changes here.
- **`BL-052`** (decision): VERIFIED status line: "Proposed. Built and tested (below); the crate is not published, and whether it is, and under what name, is the maintainer's call."
- **`BL-053`** (open): VERIFIED: SqliteDelegationJournal exists (crates/a2a-protocol-server/src/store/delegation_journal/sqlite.rs). Open per swarm-orchestration.md:316: "a swarm run that kills a supervisor process is not done". The ADR status needs settling now that 0.16.0 is released (its choices were "open to change while 0.16.0 is unreleased").
- **`BL-054`** (open): VERIFIED status line. The maintainer approved all four sources on 2026-10-31; placement and dependencies are open.
- **`BL-055`** (open): VERIFIED status line: recorded "for a later session".
- **`BL-056`** (partial): VERIFIED status line: "Accepted, built in part". H12 "stays conditional on a measurement".
- **`BL-058`** (maintainer-action): VERIFIED from the README table (22 rows): 3 filed (1 fixed upstream), 2 superseded, 13 "Draft, not sent", 3 "Draft, held" (a2a-required-empty-array, slimrpc-a2a-error-mapping, a2a-go-a2a-json-errors), plus 1 reproduction script. "Nothing is filed upstream without asking." The a2a-go R19 draft is deliberately not committed. The agntcy error-details draft is "ready for the maintainer to file" (handoff:632).
- **`BL-063`** (open): VERIFIED the lockfile holds url 2.5.9. The exposure question for published 0.16.0 is recorded as not checked.
- **`BL-065`** (open): VERIFIED text. Other gates to 1.0: TCK no MUST failure outside baselined rows (met: baseline empty); nightly ITK green; semver-checks on all crates; MSRV stable six months (raised to 1.94 on 2026-10-23). Planned 0.17 removals (BL-010, BL-011) are breaks.
- **`AA22-unmarked`** (stale-claim): VERIFIED by spot check that the register is stale, not that each row is fixed. Severities as the source states: 10 High, 17 Medium, 14 Low (counted from the tables). Spot checks at HEAD: OW2/Finding 8 fixed (tests/multi_replica.rs:579 the_single_writer_refusal_crosses_replicas); S10 fixed in its type half (request_context.rs:53 context_id: ContextId); T11 fixed (#[non_exhaustive] wire structs); C13 fixed (crates/a2a-protocol-client/src/proxy.rs and tls.rs read the proxy and SSL_CERT_FILE variables); S5 fixed (crates/a2a-protocol-client/src/delegation); O8 fixed (crates/a2a-protocol-client/src/call_span, call_metric). Not checked: the other rows. Every row needs a status against 0.16.0 before any is planned; C12 and C14 are the two Medium ones the document says to schedule first. Section 8 bar-raiser candidates E1-E7 are proposals, "none started".
- **`DOC-01`** (stale-claim): VERIFIED stale: a key is committed and v0.15.0/v0.16.0 were released under the gate. docs/lf-readiness-review.md:147 repeats it.
- **`DOC-02`** (stale-claim): VERIFIED stale: .github/dependabot.yml exists and Dependabot PRs #213, #216, #217 were taken. Item 7 ("Seven examples still have no tests", :186) is also out of date: 2 example directories have no test at HEAD (live-model, swarm).
- **`DOC-03`** (stale-claim): VERIFIED stale since 2026-11-11: two waivers in deny.toml. RUSTSEC-2026-0459 has a CVE; the 60-day criterion needs a re-read against it (waived 2026-11-11).
- **`DOC-04`** (stale-claim): VERIFIED stale: SqliteDelegationJournal exists.
- **`DOC-05`** (stale-claim): VERIFIED stale: the pinned test is now the_single_writer_refusal_crosses_replicas.
- **`DOC-06`** (stale-claim): VERIFIED: HEAD (2058058d) is the merge of #222; the TCK pin is 5e6f37c and test-postgres runs 16, 18, 19; the harness uses a path dependency. Also stale under "Waiting on the maintainer": the nifty-gauss branch (:459), PR #203 (:463), the turn-claims release number (:572), all settled by the 0.16.0 release.
- **`DOC-07`** (stale-claim): VERIFIED for W2-LIVE-10: Usage fields deserialize through `count`, which yields None for a float, negative or string. W2-LIVE-3/4/5: handoff:480 and :147 record the three bounds (17e124fb) and byte limits (ac63a18f); not opened in code.
- **`DOC-08`** (stale-claim): VERIFIED against the ADR status lines (built in 0.16.0). The table is dated; nothing marks it superseded.

## 4. Dedup map: one item, several names

Each line is one backlog id and every other name it travels under. All refs are in the JSON row.

| Canonical ID | Also appears as | Where |
|---|---|---|
| `X-10` | `KL-20`; ROADMAP Fleet 1; ROADMAP antipattern 7; handoff "Not solved: an outage longer than the retry" | `ROADMAP.md:65`, `ROADMAP.md:273`, `CHANGELOG.md:3280`, `docs/handoff.md:49`, `docs/handoff.md:686` |
| `E-F25` | `X-11`; `KL-02`; DECISIONS row E-F25; ADR 0057's named step | `ROADMAP.md:68`, `CHANGELOG.md:3224`, `docs/audits/2026-11-11-external-review/findings.json:83`, `docs/audits/2026-11-01-triage/e.json:354`, `docs/handoff.md:51` |
| `W2-hostile-3` | `X-09`; `KL-04` | `ROADMAP.md:82`, `CHANGELOG.md:3234`, `docs/audits/2026-11-07-campaign-wave2/hostile/README.md:48` |
| `W2-hostile-5` | `KL-05` | `ROADMAP.md:126`, `CHANGELOG.md:3240` |
| `W2-LIVE-8`, `-11`, `-12`, `-13` | `KL-06` (one changelog entry for four findings plus `W2-LIVE-14`, by design) | `CHANGELOG.md:3247`, `ROADMAP.md:131`, `ROADMAP.md:134`, `ROADMAP.md:120` |
| `D-18` | `KL-07`; ADR 0056 | `ROADMAP.md:88`, `CHANGELOG.md:3255` |
| `C-05` | `KL-09`; "Waiting on upstream" table; ADR 0038 | `ROADMAP.md:150`, `ROADMAP.md:1377`, `CHANGELOG.md:3259` |
| `D-21` | `KL-10`; ADR 0054; fork F5 | `ROADMAP.md:86`, `CHANGELOG.md:3261` |
| `H5` | `KL-11`; ADR 0044 | `ROADMAP.md:158`, `CHANGELOG.md:3263` |
| `H13` | `KL-12`; ADR 0047 | `ROADMAP.md:160`, `CHANGELOG.md:3265` |
| `KL-13` | H7, H9, H17 (no register row of their own); `E-F12`; `S3` | `ROADMAP.md:93`, `CHANGELOG.md:3267` |
| `R3` | `KL-14`; `D-05`; `SEC-14`; H14; ADR 0037, ADR 0043 | `ROADMAP.md:90`, `CHANGELOG.md:3269` |
| `BL-007` (RUSTSEC-2026-0459 waiver) | `KL-15`; `E6` alert #125; handoff "Waivers"; `SECURITY.md:85`; "Waiting on upstream" | `ROADMAP.md:104`, `deny.toml:80`, `osv-scanner.toml:37`, `docs/handoff.md:810` |
| `KL-16` | B-25, `B-28`, `B-29`; ADR 0041, ADR 0055 | `ROADMAP.md:115`, `CHANGELOG.md:3273` |
| `H19` | `KL-17`; ROADMAP Conformance 7 and Fleet 14; part of `BL-058` (the drafts) | `ROADMAP.md:53`, `ROADMAP.md:99`, `CHANGELOG.md:3275` |
| `A15`, `A26` | `KL-18` | `ROADMAP.md:168`, `CHANGELOG.md:3277` |
| `G-09` | `KL-19`; ADR 0052 | `ROADMAP.md:170`, `CHANGELOG.md:3279` |
| `E-F8` | fork F13; `KL-21` | `ROADMAP.md:80`, `CHANGELOG.md:3281` |
| `X-19` | `KL-22` (with `A3` and `E1`); handoff "Privacy" and "Control map"; `BL-046` | `ROADMAP.md:106`, `CHANGELOG.md:3282`, `docs/handoff.md:719` |
| `G1` | `E6` alerts #93 and #126; Scorecard Branch-Protection and Code-Review | `ROADMAP.md:124`, `docs/handoff.md:591` |
| `S5` | `X-15`; ROADMAP Fleet 5; handoff "Measurement"; handoff "First, next" 2 (`BL-064`) | `ROADMAP.md:78`, `docs/readiness-bar.md:288`, `docs/handoff.md:726` |
| `X-14` | `KL-03`; swarm-orchestration G4; handoff "your calls" | `ROADMAP.md:154`, `CHANGELOG.md:3226`, `docs/swarm-orchestration.md:90`, `docs/handoff.md:459` |
| `X-16` | ROADMAP DevEx 6 and Release engineering 5; handoff "Publishing"; `BL-026` depends on it | `ROADMAP.md:175`, `ROADMAP.md:225`, `docs/handoff.md:635` |
| `X-17` | ROADMAP "Open questions" 1; handoff "Publishing" | `ROADMAP.md:175`, `ROADMAP.md:1383`, `docs/handoff.md:636` |
| `G/C-01` | ROADMAP Book 4; handoff "the changelog's size"; fork F11 | `ROADMAP.md:190`, `docs/handoff.md:454` |
| `N14` | part of `BL-006` (qualification "Not covered") | `ROADMAP.md:72`, `docs/audits/2026-11-14-qualification/README.md:69` |
| `S1` | ROADMAP 0.12 "No executor resumes after a restart"; adopter-audit S15 | `ROADMAP.md:138`, `ROADMAP.md:638`, `docs/adopter-audit-2026-09-22.md:1068` |
| `BL-001` (4 PARTIAL spec rows) | audit-register "Open" rows 1 and 2; upstream drafts `agntcy-slim-rpc-error-details`, `a2a-required-empty-array-report`; handoff "Next, in order" 2 | `ROADMAP.md:39`, `docs/compliance/audit-register.md:44`, `docs/handoff.md:366`, `docs/upstream/README.md:28`, `docs/upstream/README.md:31` |
| `SEC-13` | `D-13`; `NEW-f-3`; NEW-b-1; fork F16 | `ROADMAP.md:110`, `docs/audits/2026-11-01-triage/DECISIONS.md:155` |
| `NEW-h-s0-7` | `B-20`, `B-03`, `H2` (one set of trust roots) | `ROADMAP.md:118` |
| `BL-020` (statement_count over TLS) | handoff "Now"; handoff "Facts" | `ROADMAP.md:246`, `docs/handoff.md:67`, `docs/handoff.md:311` |
| `BL-021` (F1 measured or not) | handoff "Smaller defects"; `X-15`'s status text | `ROADMAP.md:251`, `docs/handoff.md:55` |
| `BL-015` (line citations) | ROADMAP antipattern 1; handoff "Environment facts" | `ROADMAP.md:201`, `ROADMAP.md:259`, `docs/handoff.md:101` |
| `BL-014` (nightly vs the evidence gate) | ROADMAP antipattern 4 | `ROADMAP.md:196`, `ROADMAP.md:266` |
| `BL-031` (PGP key) | lf-readiness 4.4; openssf `vulnerability_report_private`; `SECURITY.md` | `ROADMAP.md:1122`, `docs/lf-readiness-review.md:150`, `SECURITY.md:124` |
| `BL-033` (`v*` tag ruleset) | ROADMAP "Signed tags"; handoff | `ROADMAP.md:1119`, `docs/handoff.md:577` |
| `BL-057` (RustSec filing) | upstream README row; control-map rows CRA-AI-I-2a, CRA-AI-II-2-4-8, CRA-14 | `docs/handoff.md:554`, `docs/upstream/README.md:33`, `docs/handoff.md:761` |
| `BL-034` (ITK dashboard) | ROADMAP "Audit, oversight and provenance"; upstream README row | `ROADMAP.md:1370`, `docs/upstream/README.md:27` |
| `BL-005` (A2A v1.1) | handoff "Now" and "First, next" 3 | `ROADMAP.md:54`, `docs/handoff.md:39`, `docs/handoff.md:91` |
| `BL-051` (ADR 0028) | handoff "Waiting on the maintainer" | `docs/handoff.md:544`, `docs/adr/0028-webassembly-and-cloudflare-workers.md:7` |

**Id collisions** (the same label means different things in different documents; the JSON namespaces the later ones):

| Label | Meaning 1 | Meaning 2 (and 3) |
|---|---|---|
| `D-9` … `D-18` / `D-09` … `D-18` | 2026-11-07 adopter walk-through, docs and CLI (`docs/audits/2026-11-07-adopter-walkthrough/README.md:46`) | 2026-11-01 triage, resilience (`docs/audits/2026-11-01-triage/d.json:215`). ROADMAP's "G/D-03 to G/D-11" (`ROADMAP.md:166`) is a third series |
| `S1` … `S6` | 2026-11-12 review, product gaps (`docs/audits/2026-11-12-adversarial-review/findings.json:173`) | 2026-09-22 adopter audit, server findings (`docs/adopter-audit-2026-09-22.md:1055`); `S0`/`S1`/`S2` are also the swarm audit's severity grades |
| `E1` … `E6` | 2026-11-13 review, enterprise asks (`docs/audits/2026-11-13-adversarial-review/findings.json:203`) | adopter-audit bar-raiser candidates E1-E7 (`docs/adopter-audit-2026-09-22.md:1234`) |
| `G1` | 2026-11-12 review, branch protection | swarm-orchestration gap G1, cancellation cascade (`docs/swarm-orchestration.md:44`) |
| `R1` … `R7`, `R3` | readiness-bar service levels (`docs/readiness-bar.md:40`) | audit register R1-R69 (`docs/compliance/audit-register.md:6`); 2026-11-12 review `R3` (push reservation); swarm-orchestration requirements R1-R7 |
| `F1` … `F16` | readiness-bar failure modes F1-F5 (`docs/readiness-bar.md:65`) | triage decision forks F1-F16 (`docs/audits/2026-11-01-triage/DECISIONS.md:44`); `E-F8` etc. are a third series |
| `C1` … `C5` | readiness-bar resource ceilings | adopter-audit client findings C1-C21 |
| `N1`…`N10` / `N01`…`N16` | adopter audit, found after phase 1 (`docs/adopter-audit-2026-09-22.md:34`) | 2026-11-13 review (`findings.json:3`) |
| `H2`, `H3`, `M6` … | 2026-10-29 audit section H, swarm scale | the 2026-10-24 OpenTelemetry wiring audit, whose report "is not in the repository" (`docs/handoff.md:402`) |
| `T1` … | 2026-11-12 review | adopter-audit types findings |

## 5. Contradictions between documents

| # | What disagrees | Side A | Side B | At HEAD |
|---|---|---|---|---|
| C-01 | Is readiness row F1 measured? | "**F1 is measured**" `docs/readiness-bar.md:181`; register status "one row measured (F1)" `docs/audits/2026-11-11-external-review/findings.json:115` | "R5, R6, R7 and F1 to F5 have not been measured" `docs/readiness-bar.md:88`; machine block `not_measured` holds F1 `docs/readiness-bar.md:288`; `S5` note `docs/audits/2026-11-12-adversarial-review/findings.json:213` | Unresolved (`BL-021`). The machine block is what a release's evidence manifest copies |
| C-02 | Are R1-R4 and C1-C5 measured? | handoff: "records no measurement of its rows R1 to R7, C1 to C5 and F1 to F5" `docs/handoff.md:726`; `docs/readiness-bar.md:193` | measured 2026-11-12, `docs/readiness-bar.md:86` and block `docs/readiness-bar.md:278` | Handoff is stale; line 193 is a dated section superseded by the next one |
| C-03 | Is a spec-added field a minor or a patch addition? | minor: `book/src/reference/upgrading/0.15-to-0.16.md:220` | patch: `STABILITY.md:127`, `README.md:505` | Unresolved; a decision (`BL-004`) |
| C-04 | Does `a2a card` honour `--header` and `--timeout`? | ignores them: `ROADMAP.md:167`, register row `docs/audits/0.16.0-release/findings.json:858` | honours them: `tools/a2a-cli/src/commands.rs:54` | Code is right; ROADMAP and register stale |
| C-05 | Does the gRPC client read `SSL_CERT_FILE`? | ignores it: `ROADMAP.md:118`, register row `findings.json:957` | reads it: `crates/a2a-protocol-client/src/transport/grpc.rs:74` | Code is right; ROADMAP and register stale |
| C-06 | Is `NEW-g-docs-4` open? | status "open (the triage verdict…)" `docs/audits/0.16.0-release/findings.json:876` | same row's evidence: "Closed by this pull request" | Closed (`STABILITY.md:15`) |
| C-07 | Is every register row that needs work in ROADMAP? | "Every row that needs work is in `ROADMAP.md`, section *After 0.16.0*" `docs/audits/0.16.0-release/README.md:67` | 19 register ids are not named in `ROADMAP.md:20-275`: `X-21`, `NEW-a-l-4a`, `B-13`, `B-18`, `B-23`, `B-27`, `B-30`, `NEW-b-2`, `D-22`, `G-18`, `NEW-g-api-1`, `NEW-g-api-4`, `G/D-02`, `G/C-02`, `NEW-g-docs-3`, `-4`, `-6`, `-7`, `H6` | Computed by matching each id (with the two ranges expanded) against those lines. Some may need no work; none says so |
| C-08 | Is a release-signing key committed? | "No key is committed yet, so the next release is blocked" `ROADMAP.md:1117`; "Release tags are not signed" `docs/lf-readiness-review.md:147` | key present `.github/release-signers/allowed_signers:4`; "Git tags after v0.14.1: Signed" `SECURITY.md:140` | ROADMAP and the review are stale |
| C-09 | Any active advisory waiver? | "with no active waiver" `docs/openssf-best-practices.md:106` | two in `deny.toml:79`-`80`; `SECURITY.md:85` | openssf page stale since 2026-11-11 |
| C-10 | Dependency-update automation? | "No Dependabot, no Renovate" `docs/lf-readiness-review.md:125` | `.github/dependabot.yml:1`; `ROADMAP.md:234` | Review stale |
| C-11 | SQLite delegation journal | "A SQLite journal is not built" `docs/swarm-orchestration.md:70` | built: `docs/adr/0030-delegation-resume-and-journal.md:12`; `crates/a2a-protocol-server/src/store/delegation_journal/sqlite.rs:1` | Swarm doc stale |
| C-12 | Is G3 (tenant fail-open) decided? | "G3 — decide fail-closed or not" `docs/swarm-orchestration.md:309` | "Decided 2026-10-02: option 2" `docs/swarm-orchestration.md:83` | Same file disagrees with itself |
| C-13 | Does the single-writer refusal cross replicas? | no: `docs/swarm-scale-findings.md:601`, `docs/adopter-audit-2026-09-22.md:679` | yes: `crates/a2a-protocol-server/tests/multi_replica.rs:579` | Both documents stale (turn claims, 2026-10-28) |
| C-14 | Benchmark harness pin | "pinned `=0.14.1`" `docs/handoff.md:449`, `docs/handoff.md:796` | path dependency `benches/sdk-comparison/harness/agent-a2a-rust/Cargo.toml:24` | Handoff stale |
| C-15 | State of #220 and #222 | "Read CI on #220 … and #222" as next step `docs/handoff.md:86` | HEAD is the merge of #222; pin `5e6f37c` at `.github/workflows/official-tck.yml:58`; PostgreSQL 16/18/19 at `.github/workflows/ci.yml:775` | Handoff "Now" is one session behind |
| C-16 | Is SLIMRPC publishing decided work or a decision? | `[0.16.x]` work: `ROADMAP.md:225` | `[decision]`: `ROADMAP.md:175` | Same file, two labels |
| C-17 | Count of untested TCK MUSTs | "21 of them" at a2a-tck `263b9cf` `tck/untested-musts.toml:7` (22 entries) | 16 not tested, 7 manual, 5 skipped at `5e6f37c` `docs/official-tck-findings.md:2395` | Header is two pins old; §27 says the file "needs no change" |
| C-18 | Where the Known Limitations sit | register cites `CHANGELOG.md:3201`-`3261` (`docs/audits/0.16.0-release/findings.json`, KL rows) | entries are at `CHANGELOG.md:3222`-`3282` | The register's own antipattern 1 (`ROADMAP.md:259`) |
| C-19 | Which wave-2 live findings are unfixed | `W2-LIVE-3`, `-4`, `-5`, `-10` "not fixed" `docs/audits/2026-11-07-campaign-wave2/live/README.md:25`-`32` | bounds built `docs/handoff.md:480`; `crates/a2a-protocol-server/src/tool_loop/openai_parse.rs:49` | README stale; `KL-06` (`CHANGELOG.md:3247`) is the current list |
| C-20 | Fork table: HttpPool, Skills, Steering | "not built" `docs/audits/2026-11-01-triage/DECISIONS.md:25`, `:36` | built in 0.16.0: `docs/adr/0032-http-pool.md:7`, `docs/adr/0035-agent-skills.md:7`, `docs/adr/0036-steering-a-running-task.md:7` | Table dated 2026-11-02, never marked superseded |
| C-21 | ADR 0030 status | "Proposed … open to change while 0.16.0 is unreleased" `docs/adr/0030-delegation-resume-and-journal.md:7` | 0.16.0 released 2026-11-19 `ROADMAP.md:6` | Status line not settled after release |
| C-22 | `handler/helpers.rs` length and the over-500 count | "612 lines", "77 of 310" `ROADMAP.md:1077`-`1082` | 961 lines; 82 baseline entries (`.file-length-baseline`) | ROADMAP figure is from 2026-08-10 |

## 6. Waiting on the maintainer: the decision list

"Default" is what a document recommends; where none is recorded it says so. "Blocks" lists backlog ids that cannot start, or cannot finish, without the answer.

| # | Decision | Options recorded | Default the docs recommend | Blocks | Source |
|---|---|---|---|---|---|
| 1 | Protect `main` with reviews (`G1`) | enable a rule requiring pull requests or checks, with `github-actions` as a bypass actor (the benchmark workflow pushes to `main`); Code-Review needs a second approver | none recorded; "A repository setting, the maintainer's" | `E6` (Scorecard #93, #126) | `ROADMAP.md:124`, `docs/handoff.md:591` |
| 2 | A `v*` tag ruleset (`BL-033`) | set it in repository settings | set it (the README "says why") | closes the gap `release.yml` cannot stop | `ROADMAP.md:1119`, `docs/handoff.md:577` |
| 3 | A2A v1.1.0-rc (`BL-005`) | (a) diff the rc proto and specification against the ledger now, no code; (b) wait for a release | none recorded | any v1.1 work: "nothing is built against a release candidate without that decision" | `ROADMAP.md:54`, `docs/handoff.md:91` |
| 4 | Phase 2 target sheet and plan (`BL-064`) | approve targets (scale, failure detection and recovery, latency, memory, cold start, telemetry cardinality) and the plan | start from `docs/readiness-bar.md`; say what hardware each target needs | `S5`, `BL-006`, `E-F18`, `H5`'s measurement | `docs/handoff.md:88` |
| 5 | Spec-added field: minor or patch (`BL-004`) | minor (upgrade guide) or patch (`STABILITY.md`, README) | none recorded | correcting whichever two documents lose | `ROADMAP.md:50` |
| 6 | Send the upstream drafts (`BL-058`, `H19`) | per draft: send, hold, drop. 13 not sent, 3 held | "Nothing is filed upstream without asking"; the agntcy error-details draft is "ready for the maintainer to file" | `BL-001` rows §12.4 and §5.7; `BL-034`; H19's cross-SDK half | `ROADMAP.md:53`, `docs/handoff.md:600`, `docs/upstream/README.md:18` |
| 7 | File GHSA-hr9h-6jvf-wvg6 with RustSec (`BL-057`) | file as drafted; the draft "leaves three decisions" to the maintainer | file (drafted) | CRA rows CRA-AI-I-2a, CRA-AI-II-2-4-8, CRA-14 | `docs/handoff.md:554` |
| 8 | Publish the SLIMRPC binding (`X-16`) | publish 0.7.0 (first publish) or keep it out | none recorded | `BL-026` (the book page's four ignored blocks and path dependency); the MSRV build of the binding | `ROADMAP.md:175`, `ROADMAP.md:225`, `docs/handoff.md:635` |
| 9 | Distribute the CLI (`X-17`) | set `vars.PUBLISH_CLI_BINARIES`; publish to crates.io; submit Homebrew and winget drafts | none; "theirs to turn on" (2026-10-08) | `G/D-04`'s packaging text | `ROADMAP.md:1383`, `docs/handoff.md:636` |
| 10 | MSRV 1.94 against 1.85 (`X-13`) | stay at 1.94; or lower and hold `time`, `serde_with`, `darling` at older releases | stay ("TRADE-OFF, documented") | STABILITY §7's six-month MSRV clock | `ROADMAP.md:175`, `ROADMAP.md:1402` |
| 11 | Pin `A2A_ITK_REVISION` in `itk.yml` (`X-22`) | pin as `acts.yml` does, or keep floating `main` | none recorded | recording this repository's own ACTS counts per binding (`ROADMAP.md:45`) | `ROADMAP.md:46`, `docs/handoff.md:460` |
| 12 | The evidence gate against the nightly job (`BL-014`) | (a) `ci_evidence.py` skips jobs `ci.yml` marks `continue-on-error`; (b) the nightly job stops being able to fail | none recorded | the next tag whenever nightly is red | `ROADMAP.md:196` |
| 13 | Pin the lint toolchain (`BL-018`) | pin a version for sessions and CI, or keep floating `stable` | none recorded | every fresh session's clippy agreeing with CI | `ROADMAP.md:231` |
| 14 | Absolute coverage floor in `codecov.yml` (`BL-017`) | set one or not | none recorded | RELEASING.md "Path to 1.0.0" | `ROADMAP.md:227` |
| 15 | Size of a release's changelog section (`G/C-01`) | options are in the triage row; fork F11 already gave a Highlights block and moved Internal out | none beyond F11 | `BL-012` (the Known Limitations page) is independent and can start | `ROADMAP.md:190`, `docs/audits/2026-11-01-triage/DECISIONS.md:109` |
| 16 | Fleet-wide inference quota (`S3`) | build with a declared overshoot bound, or keep per-replica limits documented | none recorded | the quota half of `KL-13` | `ROADMAP.md:95` |
| 17 | `Profile::RegulatedData` checks the step journal (`E1`) | let the builder see the `ToolLoop`'s journal and check it, or keep it declared | none recorded | `E1`; part of `X-19` | `docs/handoff.md:208`, `ROADMAP.md:109` |
| 18 | Push SSRF boundary behind a proxy, and `*.localhost` (fork F16: `SEC-13`, `NEW-f-3`) | (a) skip the local lookup when a proxy carries the delivery, keep static checks; (b) leave and document `allow_private_urls`; (c) resolve through the proxy (no API). For `*.localhost`: drop the suffix, or keep and document | the author's position: (a), and drop the suffix. The maintainer: "not chosen 2026-11-03: stays deferred" | `SEC-13`, `D-13`, `NEW-f-3`, `NEW-f-4`, `SEC-17` | `docs/audits/2026-11-01-triage/DECISIONS.md:155`, `:33` |
| 19 | §7.6.1 executor mailbox (in `BL-001`) | an executor mailbox so a waiting executor can take the next message, or leave the row PARTIAL | none; "closing it means an executor mailbox, an ADR and a decision" | one of the 4 PARTIAL spec rows | `docs/handoff.md:373` |
| 20 | ADR 0028, Tokio on wasm32 (`BL-051`) | accept (land the runtime seam as phase 1, with a wasm32 `cargo check` in CI) or decline | the ADR recommends an option; "If you decline, the Worker-in-front-of-a-native-agent deployment needs no change here" | phases 2 to 4 | `docs/handoff.md:544` |
| 21 | ADR 0029, publish the discovery crate and its name (`BL-052`) | publish or not; name | none recorded | — | `docs/adr/0029-local-network-discovery.md:7` |
| 22 | OAuth2 token endpoint 429/5xx classing (`AA22-N2`) | a new error variant; `UnexpectedStatus` was rejected because a token-endpoint 401 would read as the agent's | none recorded | the fix | `docs/adopter-audit-2026-09-22.md:41` |
| 23 | SLIMRPC refusals before the span (M7 of `BL-041`) | make `record_unrouted` and `RpcSystem` public | none; "an API decision" | M7 only | `docs/handoff.md:418` |
| 24 | CLI stdin message and stream idle bound (`WT-D-11`, `WT-D-12`) | (a) leave and document; (b) add `a2a send URL -` and `--stream-idle-timeout` | (b), "small, and not for 0.16.0" | the two rows | `docs/audits/2026-11-07-adopter-walkthrough/README.md:74` |
| 25 | PGP key (`BL-031`), domain `a2a-rust.dev` (`BL-032`) | publish a key; register the domain | none recorded | restoring `security@` and `conduct@` | `ROADMAP.md:1122`, `ROADMAP.md:1140` |
| 26 | Delete two redundant remote branches (`BL-059`) | delete from a clone with the right | delete (checked safe 2026-11-06) | nothing | `docs/handoff.md:487` |
| 27 | External threads (`BL-062`) | port the adk-rust patch; post the rig #391 comment; follow up AutoAgents #52 | none recorded | nothing here | `docs/handoff.md:639` |

**Vetoes still open, not decisions that block work.** The handoff lists choices "made for you, each open to a veto" (`docs/handoff.md:467`-`485`, `:503`-`538`): YAML for Skills read in-tree, steering defaults, parked-task expiry by deletion, the NUL refusal, the three tool-loop bounds, default keep-alives, stricter verifiers, new public names. No work waits on them; a veto would create work.

**Settled since the handoff was written** (listed under "Waiting on the maintainer" but moot at HEAD): the `claude/nifty-gauss-acarbw` pull request and "before the tag" questions (`docs/handoff.md:459`), pull request #203 (`:463`), the turn-claims release number (`:572`).

## 7. Counts

Open work excludes `stale-claim`, `disclosed-accepted` and `not-verifiable` rows. Run from the reports directory:

```bash
jq -r '[.[]|select(.status|IN("stale-claim","disclosed-accepted","not-verifiable")|not)]
  | group_by(.area)[]
  | "\(.[0].area)\t\(length)\t" + (group_by(.severity|ascii_downcase|split(" ")[0])
      | map("\(.[0].severity|ascii_downcase|split(" ")[0])=\(length)") | join(" "))' 06-open-items.json
```

| Area | Open rows | By severity (first word of the source's grade, lower-cased) |
|---|---:|---|
| devex | 38 | low 26, medium 5, polish 3, unstated 4 |
| docs-book | 16 | low 7, medium 3, polish 2, unstated 4 |
| fleet-correctness | 28 | high 1, low 10, medium 9, s1 1, scale 1, unstated 6 |
| governance | 11 | governance 1, release 1, unstated 9 |
| observability | 8 | low 3, medium 2, operational 1, unstated 2 |
| performance | 5 | medium 1, s1 2, unstated 2 |
| release-eng | 10 | governance 1, unstated 9 |
| security-compliance | 37 | high 1, medium 5, low 15, info 2, s1 1, operational 1, product 1, developer 1, unstated 10 |
| spec-conformance | 9 | medium 1, unstated 8 |
| testing-verification | 16 | high 1, medium 2, low 3, evidence 1, unstated 9 |
| tool-loop | 6 | low 1, polish 2, product 1, unstated 2 |
| upstream | 6 | medium 1, s1 1, unstated 4 |
| **Total** | **190** | high 3, medium 29, low 65, polish 7, info 2, s1 5, other source labels 9, unstated 69 (+1 `AA22-N2` "Low" is inside low) |

The 2026-11-12 and 2026-11-13 registers grade by kind, not level ("Operational", "Product gap", "Scale", "Evidence", "Governance", "Release criterion", "Developer experience / release criterion"); those are the "other source labels". `S1` is the swarm audit's scale grade.

```bash
jq -r 'group_by(.status)[]|"\(.[0].status)\t\(length)"' 06-open-items.json
# decision 13, disclosed-accepted 3, maintainer-action 7, not-verifiable 1, open 78, partial 86, stale-claim 12, waiting-upstream 6   (206)
jq '[.[]|select(.evidence_checked)]|length' 06-open-items.json   # 67
jq '[.[]|select(.needs_decision)]|length' 06-open-items.json     # 33
```

Counts taken from the repository at HEAD:

| What | Count | Command |
|---|---:|---|
| Release register rows | 130 | `jq length docs/audits/0.16.0-release/findings.json` |
| …of which `fixed (part)` / `open` triage rows / Known Limitations | 76 / 5 / 22 | `jq -r 'group_by(.status)…'` on the same file |
| 2026-11-13 register not FIXED | 4 of 31 (3 PARTIAL, 1 OPEN) | `jq` on its `findings.json` |
| 2026-11-12 register not FIXED | 9 of 27 (5 PARTIAL, 3 OPEN, 1 MAINTAINER) | same |
| 2026-11-11 register not closed | 13 of 22 | same |
| Spec ledger | 510 rows: 455 TESTED, 32 APPLICATION, 19 NOT_APPLICABLE, 4 PARTIAL | `python3 scripts/check_spec_ledger.py` |
| RFC ledger | 152 rows: 21 TESTED, 45 IMPLEMENTED_UNTESTED (4 high), 3 GAP, 8 PARTIAL, 1 DEVIATION, 7 APPLICATION, 45 NOT_APPLICABLE, 16 NOT_SUPPORTED, 6 SUPPORTED | `python3 scripts/check_rfc_ledger.py`; `jq -r '.documents[].entries[].status' … \| sort \| uniq -c` |
| Official TCK at `5e6f37c` | 87 of 115 MUST pass, 0 fail, 5 skipped, 7 manual, 16 not tested; baseline empty | `docs/official-tck-findings.md:2388`; `tck/conformance-baseline.json` |
| `acts/sut-behaviors.yaml` | 16 behaviours, no open marker | read |
| ADRs not Accepted | 5 of 58 (0028, 0029, 0030, 0031, 0033 Proposed); 0032 "Accepted, built in part" | `grep -n -m1 -i status docs/adr/0*.md` |
| Upstream table | 22 rows: 3 filed, 2 superseded, 13 not sent, 3 held, 1 script | `docs/upstream/README.md:12`-`33` |
| OpenSSF passing criteria | 67 rows: 62 Met, 2 Met (judgement), 3 N/A; none unmet | `awk` over the tables |
| Advisory waivers | root `deny.toml` 2; SLIMRPC `deny.toml` 3; 5 `osv-scanner.toml` files, 7 entries, `ignoreUntil` 2027-02-11 and 2027-02-17 | `grep -n -E '^id\|ignoreUntil\|\{ id'` |
| `.file-length-baseline` | 82 files | `grep -c -v -E '^\s*(#\|$)'` |
| `.book-ignore-baseline` | 1 file (4 blocks) | same |
| `mutants.toml` | not loaded by cargo-mutants; no `.cargo/` directory | `ls .cargo` |
| TODO / FIXME / XXX / HACK | 9 hits, 0 real: 7 are `mktemp …XXXXXX` templates, 2 are `\uXXXX` in doc comments | `git grep -n "TODO\|FIXME\|XXX\|HACK" -- '*.rs' '*.py' '*.sh' '*.toml' '*.yml'` |
| `#[ignore…]` | 300: 212 need a live PostgreSQL, 31 bare, 19 load or measurement, 12 need a llama-server, 12 "needs VAR", 10 need SPIRE, 3 timing or fixture | `git grep -h -o '#\[ignore[^]]*\]' -- '*.rs' \| sort \| uniq -c` |
| `#[allow(deprecated)]` | 45 in 30 files | `git grep -c 'allow(deprecated)' -- '*.rs'` |
| `#[mutants::skip]` | 38 in 25 files | `git grep -c 'mutants::skip' -- '*.rs'` |
| `#[deprecated]` | 34 attributes; 5 since 0.15.0 and 26 since 0.16.0 among those with `since` on a following line | `git grep -n -A6 '#\[deprecated' -- '*.rs'` |

The 31 bare `#[ignore]` carry no reason string; nothing in the sources lists them as debt, so they are counted here and not minted as a row.

## 8. Items that block others

- **Decision 4 (Phase 2 target sheet)** gates every measurement row: `S5`, `BL-006`, `E-F18`, `H5`. Those in turn gate `[0.17]` design work that ROADMAP makes conditional on a measurement (lease groups after `H5`; H12 load balancing in `BL-056`).
- **`G1`** gates `E6` and two Scorecard alerts. **`BL-007`** (the sqlx waiver) gates the third and is upstream.
- **`BL-014`** and **`CI1`** can each refuse the next tag: the evidence gate requires a green `ci.yml` run of the tagged commit.
- **`BL-015`** breaks `check_spec_ledger.py` at every release preparation until the citation is by heading.
- **`BL-019`**: defects in `release.yml`'s post-`ci` jobs are first seen at a tag.
- **`X-16`** gates `BL-026` and the binding's MSRV build.
- **Decision 6 (upstream drafts)** gates two of the four PARTIAL spec rows and the ITK dashboard listing.
- **Fork F16** gates five push-SSRF rows.
- **`BL-010` and `BL-011`** (0.17 removals) are breaks, so 0.17 restarts STABILITY §7's count toward 1.0 (`BL-065`).
- **`X-10` and `E-F25`** are the two `[0.16.x]` correctness fixes ROADMAP puts first in its fleet group; neither waits on a decision.

## 9. Document-system debt

Measured at HEAD (`wc -c`, `wc -l`):

| File | Size | Lines | Role today |
|---|---:|---:|---|
| `CHANGELOG.md` | 1,007,269 bytes | 14,411 | release notes; the 0.16.0 section alone is lines 36-3289; also the only home of the Known Limitations |
| `docs/handoff-history.md` | 466,943 bytes | 6,152 | verbatim history before 2026-10-15 |
| `docs/handoff.md` | 145,881 bytes | 1,519 | "current state only", but holds three dated state blocks, decisions since 2026-10-08, environment recipes, 380 lines of lessons |
| `docs/official-tck-findings.md` | 131,965 bytes | 2,414 | 27 dated sections; only §27 is current |
| `ROADMAP.md` | 91,280 bytes | 1,415 | lines 20-275 are the backlog; lines 276-1100 are closed history kept as record |

What this costs a fresh session:

1. **Reading cost.** The four files a session is told to read first total about 1.38 MB. The live backlog is 256 lines of ROADMAP plus one JSON file; the rest is history interleaved with it.
2. **Six overlapping sources of truth for "what is open":** ROADMAP "After 0.16.0"; the 0.16.0 release register; the three review registers it was built from; section O of the 2026-10-29 README (the only status the 205 triage rows have); the changelog's Known Limitations; handoff "Open engineering, not committed" (which says of itself "None of this is in `ROADMAP.md`"). Section 4 above shows 40 items carried under two to six names.
3. **Status is not countable per id.** Section O gives one status to several ids; the triage JSON has no status field; the release register's 76 `fixed (part)` rows include ids whose part is done (five of nine checked here).
4. **No document is authoritative on closure.** Fixes landed in code without the register or ROADMAP changing (C-04, C-05, C-06); a 2026-09 audit with 41 unmarked rows was never reconciled against 0.14-0.16; campaign READMEs and the fork table keep "not fixed" and "not built" after the fix.
5. **Citations by line number rot at every release** (C-18; `BL-015`). The register's 91 `CHANGELOG.md:<line>` strings were already 21 lines off four commits after the release.
6. **Ten id series collide** (section 4). An engineer told to "fix D-11" or "close S1" has two candidates each.
7. **Dated "present tense" blocks.** `docs/handoff.md` says "delete it rather than annotating it" (`:18`) and carries two "Previous state" blocks and a "Next, in order" list whose items say "Superseded" inline.
8. **Severity is not comparable.** Four vocabularies: low/medium/high; S0-S2; kind labels ("Operational", "Product gap"); none. 69 of 190 open rows have no stated severity.

### Proposal: restructure without losing the evidence trail

The principle: **one machine-readable register holds status; every other document either derives from it or is frozen and dated.** Nothing is deleted; history moves, with its git blame intact, and is linked.

1. **One register: `docs/backlog/register.json`**, seeded from `06-open-items.json`. Fields: `id`, `aliases[]`, `title`, `area`, `severity` (source's words) plus `severity_source`, `status` from a closed set (`open`, `partial`, `decision`, `maintainer-action`, `waiting-upstream`, `accepted`, `fixed`, `stale`), `target` (`0.16.x`, `0.17`, none), `needs_decision`, `blocks[]`, `evidence[]` as `{path, anchor}` where the anchor is a heading, a test name or a symbol, never a line number, `closed_by` (commit and test) and `last_verified` (commit and date). The old registers stay where they are, each with one added top line: "Frozen on <date>; status lives in `docs/backlog/register.json` under the same ids."
2. **A checker, `scripts/check_backlog.py`, in `static-checks`**, on the pattern of `check_spec_ledger.py`: every id unique; every alias resolves; every `evidence.path` exists and its anchor is found; a `fixed` row names a commit and a test that exists and is not ignored; no `open` row's `closed_by` is set; `ROADMAP.md`'s generated block matches the register. This turns C-04 to C-07 into CI failures.
3. **ROADMAP becomes a generated view plus a short hand-written head.** Lines 20-275 are emitted from the register between markers (as `docs/readiness-bar.md` already does with `measured-limits`). Lines 276-1100 move unchanged to `docs/history/roadmap-through-0.15.md`.
4. **Known Limitations get one page**, `book/src/reference/known-limitations.md`, generated from register rows with a `disclosed` flag; the changelog section links to it by id. This is ROADMAP Book item 1 (`BL-012`) and removes the third copy of each limitation.
5. **Split `docs/handoff.md` by rate of change**: `docs/handoff.md` keeps only "Now" and "First, next" (target: one screen, rewritten each session, never appended to); `docs/decisions.md` takes "Decisions the maintainer made" and "Waiting on the maintainer" as a dated log with stable anchors (`release.yml` and `packaging/README.md` cite the heading, so keep the heading text and leave a stub); `docs/environment.md` takes the container and tool recipes; `docs/lessons.md` takes "Lessons" and "Release gotchas". "Previous state" blocks move to `docs/handoff-history.md` as the file's own rule already requires.
6. **Changelog: split by release, keep one index.** `CHANGELOG.md` keeps `[Unreleased]`, the newest release, and for older releases the Highlights block and a link to `docs/changelog/<version>.md` holding the full section verbatim. `check_release_tree.py` and the release-notes extractor read the per-version file. This is the pending decision 15; it needs the maintainer because crates.io and GitHub link to the single file.
7. **Dated audits become frozen snapshots.** Add a front-matter line to each of `docs/adopter-audit-2026-09-22.md`, `docs/lf-readiness-review.md`, `docs/swarm-*.md`, the campaign READMEs and `DECISIONS.md`: "Snapshot of <date>. Not maintained. Open rows were carried into the register as <ids>." Before freezing `docs/adopter-audit-2026-09-22.md`, give each of its 41 unmarked rows a status against HEAD (`AA22-unmarked`): that is the one piece of triage this sweep found owed and did not do.
8. **Namespace ids on entry.** The register rejects a bare id that exists in another series; new audits get a prefix (`WT-`, `AA22-`, as used here). Old ids keep working as aliases.
9. **One severity scale beside the source's.** Keep `severity_source` verbatim and add a normalized `severity` only when the maintainer assigns it; a row without one is visibly `unstated` rather than silently sorted.
10. **`docs/official-tck-findings.md`**: keep §27 and a table of contents as the live page; move §1-§26 to `docs/history/official-tck-findings-through-2026-11.md`. `tck/untested-musts.toml`'s header takes its counts from the pinned run (C-17).

Order that keeps the trail intact: add the register and checker first (no file moves); then generate ROADMAP's block and the limitations page from it; then move history in commits that only move text (`git mv` or a pure cut and paste, so `git log --follow` and blame survive); then add the "frozen" lines. Each step is a `[0.16.x]` documentation change by STABILITY §3.

## 10. What this sweep did not settle

- Each of the 76 `partial` triage rows individually; only nine were opened.
- The 41 unmarked adopter-audit rows beyond six spot checks.
- The handoff's OpenTelemetry wiring list (`BL-041`), which the handoff itself says was not re-derived after 0.16.0.
- Anything that lives in GitHub settings or its alerts API (`G1`, `E6`, `BL-033`, `BL-060`), crates.io (`X-16`), or CI run history (`CI1`, `BL-044`).
- The control map's 29 partial rows (`docs/handoff.md:740`), quoted from the handoff and not re-counted.
- `docs/handoff-history.md` and the changelog below the 0.16.0 Known Limitations were not read.
