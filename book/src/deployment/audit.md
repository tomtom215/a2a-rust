<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Audit Trail

With the `audit` feature, a server records who did what to which task, in a
hash chain per tenant that shows if anything was changed, removed or
reordered afterwards. It is the record an incident responder reconstructs a
multi-agent run from, and the kind the EU AI Act asks a high-risk AI system
to keep: automatic logs over the system's lifetime (Article 12), kept at
least six months (Articles 19 and 26(6)), including how the system
interacted with other AI systems (Article 72(2)). The design, its threat
model and its measured cost are in
[ADR 0015](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0015-audit-trail.md);
which articles it does and does not help with is in the
[control map](https://github.com/tomtom215/a2a-rust/blob/main/docs/compliance/control-map.md).

## What is recorded

| Record | When | Carries |
|---|---|---|
| `call` | every RPC ends — including ones authentication refused | method, outcome, the caller and how they authenticated, the trace |
| `run.started` | an executor starts working on a task | task, context and message ids, digests of the message and each part, the caller, the trace |
| `task.event` | the agent emits an event and it is stored | its position in the task's event log, the new state, a digest, and which `run.started` it belongs to |
| `task.cancel_requested` | `CancelTask` reaches a running task | task, the caller, the trace |
| `call.started` | a call is admitted, only in [required](#required-mode) mode | method, trace |

Content is recorded as a SHA-256 digest of its canonical JSON, never as
text. A record proves what was said — hash the message you hold and compare —
without being another copy of it.

## Turning it on

```rust,no_run
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
use std::sync::Arc;
use a2a_protocol_server::audit::{AuditLog, SqliteAuditStore};
use a2a_protocol_server::audit::record::{CheckpointSigner, SigningAlg};
use a2a_protocol_server::{BearerTokenAuthInterceptor, RequestHandlerBuilder};
# struct MyAgent;
# a2a_protocol_server::agent_executor!(MyAgent, |_ctx, _q| async { Ok(()) });
# async fn run() -> Result<(), Box<dyn std::error::Error>> {

// The checkpoint key: PKCS#8 DER, made as shown below. Keep it out of the
// database the records go to.
let key = std::fs::read("/run/secrets/audit-ed25519.der")?;
let signer = CheckpointSigner::from_pkcs8(SigningAlg::EdDsa, "audit-2026-10", &key)?;

let store = SqliteAuditStore::new("sqlite:audit.db?mode=rwc").await?;
let log = Arc::new(
    AuditLog::new(Arc::new(store))
        // A signed checkpoint every 1,000 records of a tenant's chain.
        .with_signer(signer, 1_000),
);

let handler = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(card())
    .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([
        ("token-for-billing-agent", "billing-agent"),
    ]))
    .with_audit(Arc::clone(&log))
    .build()?;
# let _ = handler;
# Ok(())
# }
```

Records name the caller only when authentication does: give each credential
a label (`with_labelled_tokens`, `with_labelled_keys`), or use JWT. The
subject is the scheme and the name together: `bearer:<label>`,
`api-key:<label>`, or `jwt:<issuer>:<sub>` (`auth::caller_id` builds each). The recording interceptor is placed first in the
chain whatever order you add interceptors in, so refused calls are recorded
too.

Stores: `InMemoryAuditStore` (tests, or a deployment that ships records
elsewhere), `SqliteAuditStore` with `sqlite`, `PostgresAuditStore` with
`postgres`. Replicas can share one PostgreSQL store; they interleave on a
tenant's chain without forking it.

### Or as one profile

`Profile::Auditable` applies the audit trail and an approval gate in one
call. `build()` then refuses an incomplete configuration and names what is
missing:

- a log that is not required, which would serve calls it cannot record;
- a log that signs no checkpoints, whose chains could lose their end
  without trace;
- no authenticating interceptor, which leaves the records naming no caller;
- tasks isolated by caller, the default with an authenticating interceptor,
  which would keep the gate's approver from the requester's task. Call
  `share_tasks_across_callers()`, or set a gate that allows self-approval
  (see [Human oversight](./oversight.md#the-gate)).

```rust,no_run
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
use std::sync::Arc;
use a2a_protocol_server::audit::{AuditLog, SqliteAuditStore};
use a2a_protocol_server::audit::record::{CheckpointSigner, SigningAlg};
use a2a_protocol_server::profile::Profile;
use a2a_protocol_server::{BearerTokenAuthInterceptor, RequestHandlerBuilder};
# struct MyAgent;
# a2a_protocol_server::agent_executor!(MyAgent, |_ctx, _q| async { Ok(()) });
# async fn run(key: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
let log = AuditLog::new(Arc::new(SqliteAuditStore::new("sqlite:audit.db?mode=rwc").await?))
    .with_signer(CheckpointSigner::from_pkcs8(SigningAlg::EdDsa, "audit-2026-10", key)?, 1_000)
    .require_record(true);
let handler = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(card())
    .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([("t", "billing-agent")]))
    .with_profile(Profile::Auditable(Arc::new(log)))
    .share_tasks_across_callers()
    .build()?;
# let _ = handler;
# Ok(())
# }
```

## The checkpoint key

A hash chain shows that nothing in the middle changed. Only a signed
checkpoint shows that nothing was cut off the end. Make a key with OpenSSL:

```sh
# Ed25519
openssl genpkey -algorithm ed25519 | openssl pkcs8 -topk8 -nocrypt -outform DER -out audit-ed25519.der

# or P-256 (ES256)
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 \
  | openssl pkcs8 -topk8 -nocrypt -outform DER -out audit-p256.der
```

The `pkcs8 -topk8` step matters for P-256: without it OpenSSL writes a SEC1
key, which `CheckpointSigner::from_pkcs8` refuses with that command in the
error. `log.trusted_key()` returns the public half; publish it where your
auditors can get it without trusting the machine that wrote the records.

Whoever holds the private key can rewrite a chain and sign the result, so the
key is what the records' integrity rests on. Keep it in a secret store, not
beside the database.

## Verifying

```rust,no_run
# use a2a_protocol_server::audit::AuditLog;
# async fn check(log: &AuditLog) -> Result<(), Box<dyn std::error::Error>> {
// Cover the tail, then check the store against itself.
log.checkpoint("acme").await?;
let report = log.verify("acme").await?;
assert!(report.is_intact(), "{:?}", report.failure);
println!(
    "{} records, signed through {:?}, {} unsigned at the end",
    report.records, report.signed_through, report.unsigned_tail()
);
# Ok(())
# }
```

That checks the store against the key this process holds. An auditor should
verify an export — `log.export(chain)` and `store.checkpoints(chain)` — on
their own machine with `a2a_protocol_types::audit::verify_chain` and a key
they obtained independently. The `a2a` CLI does the same from files, with no
code: `a2a audit verify records.json --checkpoints checkpoints.json --keys
jwks.json` prints the report and exits 1 unless the chain is intact. It reports the first thing wrong: an edited
record, a gap, a reordering, a record from another chain, a missing start, a
checkpoint beyond the last record (the tail was removed), or a signature that
does not verify.

## Following one task across agents

Every record carries the W3C trace id of the call that caused it, and this
SDK's client propagates `traceparent` on every outbound call. With both
agents audited, the records for one user request are the records with one
`trace.traceId` across the two agents' chains — the interaction Article 72(2)
asks a provider to be able to analyse.

## Retention and legal holds

Nothing is deleted unless you call `purge`, and it never deletes:

- anything younger than the floor — `AuditRetention::six_months()` is 184
  days, the longest six consecutive calendar months; a shorter floor has to be
  asked for by name (`allowing_shorter_than_six_months`);
- a chain's newest record;
- anything in a chain under a legal hold (`log.place_hold(chain, reason)`
  until `log.release_hold(chain)`).

```rust,no_run
# use a2a_protocol_server::audit::{AuditLog, AuditRetention};
# async fn nightly(log: &AuditLog) -> Result<(), Box<dyn std::error::Error>> {
let now_ms = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)?
    .as_millis() as i64;
let report = log.purge(AuditRetention::six_months(), now_ms).await?;
for (chain, deleted, anchor) in &report.purged {
    println!("{chain}: {deleted} records deleted, anchored at {anchor}");
}
# Ok(())
# }
```

Before deleting, purge signs an *anchor* — the hash of the last record it
deletes — so what remains still verifies. It therefore needs the checkpoint
signer, and refuses to run without one.

Six months is the AI Act's floor for high-risk systems. The GDPR asks that
personal data be kept no longer than necessary, and the caller's subject is
personal data in most deployments. Choose the floor for your system; the SDK
only refuses to choose a shorter one by accident.

## Protecting who and what, and erasing a person

The records above name each caller, and a digest of a short message can be
confirmed by anyone who reads the store and guesses. With the `encryption`
feature, `AuditLog::with_privacy` protects both before each record is
hashed into its chain:

- each person named (the caller, an approval's requester) is sealed under a
  key of their own, kept by the audit store, wrapped by your key-encryption
  key, and named in records only by a random handle;
- each digest becomes an HMAC under your key, bound to its record, so it
  confirms no guess without the key.

The chain still verifies with no key. Whoever holds the keys reveals a
record and checks content against it. Erasing a person destroys their key:
their name stops opening in every record, and the chain still verifies.

```rust,no_run
use std::sync::Arc;
use a2a_protocol_server::audit::{AuditLog, AuditPrivacy, HMAC_KEY_LEN, InMemoryAuditStore};
use a2a_protocol_server::store::encrypted::{AES_256_KEY_LEN, Aes256GcmCipher};

# fn kms_key(_: &str) -> [u8; 32] { [0; 32] }
# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let kek = Arc::new(Aes256GcmCipher::new("kek-2026-10", kms_key("kek"))?);
let privacy = AuditPrivacy::new(kek, "hmac-2026-10", kms_key("hmac"))?;
let log = AuditLog::new(Arc::new(InMemoryAuditStore::new())).with_privacy(privacy)?;

// An auditor reads a record back, with the keys.
let records = log.export("acme").await?;
if let Some(first) = records.first() {
    let shown = log.reveal(first).await?;
    println!("{:?} by {:?}", shown.record.kind, shown.record.actor);
}

// A person asks to be erased (GDPR Art. 17), outside any legal hold.
let done = log.erase_subject("acme", "bearer:alice@example.test", "operator:dpo").await?;
if !done.in_clear.is_empty() {
    // Records from before protection was on still name them; report these.
    println!("still named in records {:?}", done.in_clear);
}
# let _ = (AES_256_KEY_LEN, HMAC_KEY_LEN);
# Ok(())
# }
```

What erasure does not do, stated in full in ADR 0015's amendment:

- **Unlink.** The person's records keep one common handle, so they stay
  linkable to each other; times and ids in them may identify the person
  together with other data. Pseudonymised, not anonymised.
- **Reach copies.** Backups, replicas, a database's write-ahead log, and
  PostgreSQL's dead tuples until `VACUUM a2a_audit_subject_keys` keep the
  key until they are overwritten; your key-encryption key unwraps such a
  copy.
- **Reach records from before protection.** `erase_subject` reports them in
  `in_clear`; they cannot be changed without breaking the chain.
- **Work with an HMAC key you dropped.** Keep each old HMAC key in the ring
  (`with_hmac_key`) for as long as people's keys made under it may exist.

It also costs: an append took 17.6 µs plain and 47.2 µs protected on the
in-memory store, 296 µs and 521 µs on a SQLite file (ADR 0015 gives the
method). Retention drops the keys of people no remaining record names.

## Required mode

By default a record that cannot be written is logged, counted in
`log.failures()` and reported to your metrics as a persistence error on
`audit_append`, and the call is served anyway. If serving an unrecorded call
is worse than refusing it:

```rust,no_run
# use std::sync::Arc;
# use a2a_protocol_server::audit::{AuditLog, InMemoryAuditStore};
let log = AuditLog::new(Arc::new(InMemoryAuditStore::new())).require_record(true);
# let _ = log;
```

Each call then writes `call.started` before the handler runs, and is refused
if that write fails. That record is written before authentication, so it
names no caller; the `call` record after it does.

## Limits

- Record times come from the server's clock, not a trusted time source.
- A run's link from its events to its caller is kept in memory; a restart
  while a task runs leaves that task's later events without a `runSeq`.
- Appends to one tenant's chain are serialised. ADR 0015 gives the measured
  cost: about 9 µs per record in memory and 270 µs on a SQLite file on the
  machine measured.
