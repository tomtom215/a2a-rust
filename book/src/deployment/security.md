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

The server terminates TLS on its own listener with `tls-rustls`:
`Server::with_tls_pem` takes a certificate chain and key, and `Server::with_tls`
takes a `rustls::ServerConfig` as built, so a configuration that requires
client certificates gives mutual TLS on the HTTP bindings as it does on gRPC
(`GrpcDispatcher::with_tls`). The client certificate reaches the
interceptors on every binding, and `MutualTlsAuthInterceptor` names the
caller from it ([Authentication](../building-agents/authentication.md#mutual-tls-the-client-certificate-names-the-caller));
until 2026-10-18 the HTTP dispatchers did not see it. See
[Production](./production.md) for the setup. A reverse proxy in front remains
a fine choice when it already manages your certificates.

The client and the push sender speak TLS via `tls-rustls`, which is on by
default in `a2a-protocol-sdk` and `a2a-protocol-client` and off in a direct
dependency on `a2a-protocol-server`.

## Outbound requests are the dangerous ones

An agent that accepts a webhook URL from a caller is an SSRF primitive unless
something stops it. The bundled `HttpPushSender` does:

* private and loopback addresses are rejected **at config creation and again at
  delivery time** — defence in depth, because the two moments can disagree;
* IPv4-in-IPv6 smuggling is rejected (`::ffff:` mapped, NAT64 `64:ff9b::`, and
  the deprecated `::a.b.c.d`);
* the non-canonical numeric IPv4 encodings a C resolver still accepts are
  normalised and rejected — `http://2852039166/`, `http://0xA9FEA9FE/` and
  `http://0251.0376.0251.0376/` all denote `169.254.169.254`;
* the validated IP is pinned, so DNS rebinding between check and connect does
  not help an attacker;
* credentials are checked for `\r` and `\n` before they reach a header;
* every delivery is capped at 30 seconds;
* with `https_only()`, an `http://` webhook is refused at config creation and
  again at delivery, so payloads and credentials never cross the network in
  plaintext (opt-in: spec §13.2 says webhook URLs SHOULD use HTTPS);
* with a `PushConfigCipher` (`with_cipher` on the SQL push-config stores),
  configs are sealed at rest, bound to their row, so the table holds no token
  or credential in the clear (opt-in: spec §13.2 says agents SHOULD store them
  securely).

If you replace the sender, you inherit all of that as your responsibility. The
validation is public in `a2a_protocol_server::url_guard`: `check_public_url`
applies the address checks above to a URL, and `resolve_public_url` also
resolves its host and refuses it if any address is private. Use them rather
than reimplementing the list above.

The same applies to file references in messages (spec §14: they "MUST be
validated to prevent SSRF"). The SDK never fetches a `Part`'s URL, so this
falls to an executor that does: call `resolve_public_url` before fetching and
connect to the address it returns (`None` means the URL already names an
address, which it has checked), so a second lookup cannot answer differently. To refuse such messages before the
executor runs, call `RequestHandlerBuilder::refuse_private_file_urls()`; it
checks the URL's form only, without DNS, so an executor that fetches still
has to resolve.

## What a card discloses

A card is served to whoever asks, so `build()` and every card handler refuse a
card, public or extended, that carries a credential in one of its URLs: user
information (`user:pass@host`) or a query parameter named like a secret
(`token`, `api_key`, `signature` and similar). `agent_card::card_credentials`
lists the offending fields. A URL naming an internal host (a private address,
`.internal`, `.local`) is reported by `agent_card::card_warnings`, which
`build()` logs, but not refused (spec §13.3 and §14: the card SHOULD NOT carry
credentials or internal details).

That "the two moments can disagree" is not hypothetical: the numeric-IPv4 line
above was added after an [adversarial run](./security-testing.md) found the
registration check accepting an integer-encoded metadata URL that delivery still
blocked. The gap is closed and regression-tested in both the unit suite and the
over-the-wire probe.

## Data at rest

With the `encryption` feature, `store::encrypted::EncryptedTaskStore` wraps
any `TaskStore` and seals every part of a task (its history, its status
message, its artifacts) and of every event in its log before the wrapped
store sees it. Each part is sealed whole (content, metadata, filename and
media type) and bound to where it is stored: the tenant partition, the task,
the record or the position in the log, the message or artifact id, and its
index there. A part that is altered, moved to any other such place, sealed
under a key the cipher does not hold, or stored unsealed fails the read.

```rust,no_run
use a2a_protocol_sdk::server::store::InMemoryTaskStore;
use a2a_protocol_sdk::server::store::encrypted::{Aes256GcmCipher, EncryptedTaskStore};

# fn key_from_your_kms(_: &str) -> [u8; 32] { [0; 32] }
let ring = Aes256GcmCipher::new("2026-10", key_from_your_kms("2026-10"))?
    .with_key("2026-09", key_from_your_kms("2026-09"))?; // still opens older values
let store = EncryptedTaskStore::new(InMemoryTaskStore::new(), ring.into_arc());
# Ok::<(), a2a_protocol_sdk::types::error::A2aError>(())
```

`Aes256GcmCipher` is AES-256-GCM through `ring`, with a fresh random 96-bit
nonce per value; one key seals and every key in the ring opens. NIST SP
800-38D §8.3 bounds a key used with random nonces to 2^32 seals, counted
across every process holding the key and every push config sealed with the
ring. Every write seals every part of the task, so an artifact streamed in
`n` chunks costs about `n²/2` seals. `seal_count()` reports this process's
count; rotate the active key well before the total nears the bound. To keep
keys in a key-management service instead, implement `ContentCipher`. The
same ring is a `PushConfigCipher`, so the SQL push-config stores can share
it.

What it does not protect:

* **The structure the stores query**: task, context, message and artifact
  ids, roles, states, timestamps, artifact names and descriptions, message
  `extensions` and `referenceTaskIds`, the `metadata` of tasks, messages,
  artifacts and events, and the number and size of parts. Keep personal data
  out of those fields.
* **The shape of a record**: messages or artifacts reordered or removed,
  parts removed from the end of one, parts exchanged between two messages
  with the same id, and a store rolled back to an earlier record all still
  open.
* **The audit trail**, which keeps a digest of each event as written: a
  reader of the audit store can confirm a guess of a short event's content
  unless the audit log protects its records too (`AuditLog::with_privacy`;
  see [Audit Trail](./audit.md)).

A part that does not open fails the whole read, a `list` page and a log
replay included, so keep every key that sealed a stored value in the ring
for as long as the value is stored. A store holding data from before the
wrapper was installed is refused until `accept_unsealed_parts()` is set;
while it is, an unsealed part planted in the store is read as genuine, so
remove it once every record has been rewritten. Sealing every part on every
write costs time in proportion to the task's size; this has not been
measured.

## Input handling

* The REST dispatcher rejects `..`, `%2E%2E` and `%2e%2e` in path segments, and
  paths that escape the route hierarchy.
* Request bodies are capped at 4 MiB (JSON-RPC and REST) and query strings at
  4 KiB (REST); SSE events at 16 MiB, configurable.
* A tenant at `max_concurrent_tasks` is refused rather than queued, so a
  declared bound stays a bound under load.

## Agent card signing, and the part people miss

`signing` on `a2a-protocol-server` is a **forwarding feature**. It makes the
signing types available. **The server never signs the card it serves.**

Sign the card yourself with `sign_card_es256` and hand the signed card to
`RequestHandlerBuilder`. An unsigned card is served happily and verifies as
unsigned — the server has done what it was asked, so there is no error to look
for. `examples/incident-response` does this correctly and is worth copying.

**Declare the extensions before you sign.** `build()` advertises the
extensions this server can honour by appending to
`capabilities.extensions`, and `canonicalize_card` strips only `signatures`
— so `capabilities.extensions` is inside the bytes a signature covers. A
card signed before those entries exist would be served canonicalizing to
bytes nobody signed, and every client that verifies would fail while this
side reported nothing. So `build()` **refuses** a signed card it would
otherwise have to edit, and names the URIs to add:

```rust
use a2a_protocol_types::agent_card::{AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::extensions::AgentExtension;
use a2a_protocol_types::failure::FAILURE_EXTENSION_URI;
use a2a_protocol_types::idempotency::IDEMPOTENCY_EXTENSION_URI;
use a2a_protocol_types::signing::sign_card_es256;

fn card_to_serve(pkcs8: &[u8]) -> A2aResult<AgentCard> {
    let mut card = AgentCard::new(
        "Triage Agent",
        "1.0.0",
        AgentInterface::jsonrpc("https://agent.example/rpc"),
    )
    .with_description("Triages incoming incidents")
    .with_skill(AgentSkill::new("triage", "Triage", "Triages an incident").with_tags(["triage"]));

    // Declare first. `AgentExtension::new` leaves `required` unset, which is
    // what a non-required extension means on the wire.
    card.capabilities.extensions = Some(vec![
        AgentExtension::new(IDEMPOTENCY_EXTENSION_URI),
        AgentExtension::new(FAILURE_EXTENSION_URI),
    ]);

    // ...then sign the card that results, and hand *that* to the builder.
    let signature = sign_card_es256(&card, pkcs8, "my-key")?;
    card.signatures = Some(vec![signature]);
    Ok(card)
}
```

Declare only the extensions your configuration actually advertises:
`IDEMPOTENCY_EXTENSION_URI` is advertised exactly when the configured
`TaskStore` reports `supports_idempotency()`, and `FAILURE_EXTENSION_URI`
always. Declaring one the server would not have added is allowed — an
operator's own entry is never overwritten — but it advertises a capability
to clients, so declare it only if it holds.

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
  `RateLimitInterceptor` does nothing. Failed credentials are throttled only
  with `AuthFailureThrottle` registered before the authenticator (see
  [Authentication](../building-agents/authentication.md#throttling-guessed-credentials)).
* **The SLIMRPC binding is out of scope for the published crates' audit
  surface.** It pulls 359 transitive dependencies including a native C crypto
  build, which is why it lives outside the workspace with its own lockfile and
  its own `cargo-deny` run.

## Reporting a vulnerability

Do not open a public issue. Open a [draft security
advisory](https://github.com/tomtom215/a2a-rust/security/advisories/new), or
email the address in this project's copyright headers. `SECURITY.md` carries the
full policy, including the 90-day coordinated-disclosure timeline and what to
include in a report.
