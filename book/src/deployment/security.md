<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Security

What this SDK guarantees, what it leaves to you, and what it does not do at all.
[Production Hardening](./production.md) has the knobs; this page is the posture
behind them, including the parts that are gaps rather than features.

## What holds by construction

| Property | How |
|---|---|
| No memory-unsafety in the published crates | All four set `#![forbid(unsafe_code)]`. There is no `unsafe` block to review, because one cannot be added without removing that line in a diff |
| No panic paths in library code | Zero `.unwrap()` in runtime library code, and zero `panic!`/`todo!`/`unimplemented!`. A CI ratchet fails the build when that changes |
| Undocumented public API cannot ship | `#![deny(missing_docs)]` |
| Dependencies are audited | `cargo-deny` runs in CI over the workspace, and separately over the SLIMRPC binding's much larger tree |
| Releases are attested | SLSA build provenance and a per-crate CycloneDX SBOM are produced by the release workflow |

## Authentication

Nothing is authenticated unless you say so. An agent built with no interceptor
serves every caller.

| Interceptor | Feature | Use when |
|---|---|---|
| `BearerTokenAuthInterceptor` | none | you validate an opaque token yourself |
| `ApiKeyAuthInterceptor` | none | a shared secret in a header is genuinely enough |
| `JwtAuthInterceptor` | `auth-jwt` | HS256/RS256/ES256, static or remote JWKS with OIDC discovery |

`JwtAuthInterceptor` is the one to reach for when the caller is another
organisation: it verifies signatures against a key set you did not have to
distribute by hand, and it is the only one of the three that can tell you
*who* the caller is rather than only that they knew a secret.

Pair it with a `BearerTokenTenantResolver` if you are multi-tenant — see
[Multi-Tenancy](./multi-tenancy.md), where the failure mode of getting this
wrong is spelled out.

## Transport

**The server does not terminate inbound TLS.** Put it behind a reverse proxy.
This is a deliberate scope decision, not an omission: certificate lifecycle,
SNI, ALPN and renewal are solved better by nginx, Caddy or a load balancer than
by a protocol library.

```text
Client ──HTTPS──→ [nginx / Caddy / LB] ──HTTP──→ [a2a-rust agent]
```

Outbound is different. The client and the push sender do speak TLS, via
`tls-rustls` — on by default in `a2a-protocol-sdk` and `a2a-protocol-client`,
off in a direct dependency on `a2a-protocol-server`.

## Outbound requests are the dangerous ones

An agent that accepts a webhook URL from a caller is an SSRF primitive unless
something stops it. The bundled `HttpPushSender` does:

* private and loopback addresses are rejected **at config creation and again at
  delivery time** — defence in depth, because the two moments can disagree;
* IPv4-in-IPv6 smuggling is rejected;
* the validated IP is pinned, so DNS rebinding between check and connect does
  not help an attacker;
* credentials are checked for `\r` and `\n` before they reach a header;
* every delivery is capped at 30 seconds.

If you replace the sender, you inherit all of that as your responsibility. The
validation helpers are public and reusable — use them rather than reimplementing
the list above.

## Input handling

* The REST dispatcher rejects `..`, `%2E%2E` and `%2e%2e` in path segments, and
  paths that escape the route hierarchy.
* Request bodies are capped at 4 MiB and query strings at 4 KiB on REST; SSE
  events at 16 MiB, configurable.
* A tenant at `max_concurrent_tasks` is refused rather than queued, so a
  declared bound stays a bound under load.

## Agent card signing, and the part people miss

`signing` on `a2a-protocol-server` is a **forwarding feature**. It makes the
signing types available. **The server never signs the card it serves.**

Sign the card yourself with `sign_agent_card` and hand the signed card to
`RequestHandlerBuilder`. An unsigned card is served happily and verifies as
unsigned — the server has done what it was asked, so there is no error to look
for. `examples/incident-response` does this correctly and is worth copying.

## Known gaps

Stated because a security page that lists only strengths is not one.

* **No PGP key for vulnerability reports.** Email to the maintainer cannot be
  encrypted end-to-end. Use [GitHub Security
  Advisories](https://github.com/tomtom215/a2a-rust/security/advisories/new)
  instead, which keeps a report private without needing a key.
* **Release tags are not signed.** Since `v0.8.0` they are annotated — they
  carry a tagger and a date, and the release workflow refuses a lightweight tag
  — but nothing is GPG/SSH-signed, so `git tag -v` cannot verify a release. For
  a cryptographic link from a published version to this repository, use the
  build provenance attestations described in `PROVENANCE.md`, not the tag.
* **No rate limiting is applied unless you wire it.** `rate_limit_rps` is
  counted in an interceptor; setting the field without installing
  `RateLimitInterceptor` does nothing.
* **The SLIMRPC binding is out of scope for the published crates' audit
  surface.** It pulls 379 transitive dependencies including a native C crypto
  build, which is why it lives outside the workspace with its own lockfile and
  its own `cargo-deny` run.

## Reporting a vulnerability

Do not open a public issue. Open a [draft security
advisory](https://github.com/tomtom215/a2a-rust/security/advisories/new), or
email the address in this project's copyright headers. `SECURITY.md` carries the
full policy, including the 90-day coordinated-disclosure timeline and what to
include in a report.
