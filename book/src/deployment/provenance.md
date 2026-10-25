<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Provenance and Signatures

TLS proves who you are connected to. It says nothing about content that has
passed through another agent, or through a proxy that terminated TLS. This
chapter covers what proves where content came from:

- **Signed agent cards**, verified against a JWK Set.
- **Marked and signed messages and artifacts**: the provenance extension,
  including the machine-readable "AI-generated" marker that EU AI Act
  Article 50(2) asks for.

Signing, verifying and key sets need the `signing` feature; marking content
(`mark_ai_generated`) does not.

## Verifying an agent card

```rust,no_run
use a2a_protocol_client::discovery::{CardFetchOptions, resolve_agent_card_with_options};
use a2a_protocol_client::jwks::fetch_jwks;
# async fn run() -> Result<(), Box<dyn std::error::Error>> {
// A key set you trust: your directory's, or one the operator published.
// Not the `jku` in the card's own signature header, which is unverified.
let keys = fetch_jwks("https://keys.example.com/a2a/jwks.json", &CardFetchOptions::default()).await?;

// The fetch verifies any of the card's signatures, by `kid`, as received,
// and refuses a card that carries none.
let options = CardFetchOptions::default().with_signature_keys(keys);
let card = resolve_agent_card_with_options("https://agent.example.com", &options).await?;
# let _ = card;
# Ok(())
# }
```

- **Unsigned cards:** with keys supplied, a card with no signature fails the
  fetch. Anyone able to alter a card on its way can also remove its
  signatures, so a fetch that returned an unsigned card would return the
  altered one unverified. If some of your agents do not sign,
  `.with_require_signature(false)` verifies signatures only when present,
  which is all §14.3 asks (and §8.4.3 asks a client only to SHOULD verify):
  a signed card must still verify, and an unsigned one is returned as it
  is, whoever removed its signatures.

- **As received:** the fetch verifies the card's JSON before reading it into
  an `AgentCard`. The signer signed every field it serves, and `AgentCard`
  drops one this SDK does not model, such as a field a later minor version
  adds, so a correctly signed card would not verify as an `AgentCard`. For a
  card you fetched yourself, `verify_card_json_with_jwks` takes its JSON;
  `verify_card_with_jwks` verifies a card as this SDK models it.

- **Every way in:** a `CachingCardResolver` verifies with the options it is
  given (`CachingCardResolver::new(url)?.with_options(options)`), and never
  caches a card that fails. For a card you hold as JSON,
  `ClientBuilder::from_signed_card(&json, keys)` verifies it as given and
  then builds as `from_card` does. Both since 0.15.

- **Algorithms:** ES256 (P-256) and EdDSA (Ed25519). A key is only ever
  tried under the algorithm its type is for.
- **Revoking a key:** remove it from the set. A verifier that re-fetches the
  set stops accepting signatures made with it; nothing that verifies card or
  content signatures caches a set. (The server's `JwtAuthInterceptor` caches
  the JWKS it uses for JWT login, which is a separate key set.)
- **Expiring a key:** publish it with an `exp` member (seconds since the
  epoch; `Jwk::with_expiry`). RFC 7517 defines none, but some key sets carry
  it, and a key at or past it is never used to verify (spec §8.4.3).
- **Certificates (`x5c`):** a key that carries its X.509 certificate chain
  (RFC 7517 §4.7) is used only while the first certificate is valid
  (`notBefore` through `notAfter`) and only if that certificate holds the
  same key. A chain that is not standard base64 DER certificates leaves
  the key unused. The chain is not validated to a root, and CRL/OCSP are
  not consulted: the key set is already the source you trust, and
  removing a key from it is how it is revoked.
- **A single key:** `verify_agent_card` takes the raw key (the 65-byte P-256
  point, or the 32-byte Ed25519 key) or its `SubjectPublicKeyInfo` DER, as
  `openssl pkey -pubout -outform DER` writes it.
- **Fetching:** `fetch_jwks` needs `https://`, or plain `http://` to a
  loopback host (`localhost`, `127.0.0.1`, `[::1]`) for development. It caps
  the set at 256 KiB, and the same time budget covers the whole fetch.

To sign a card: `sign_card_es256` or `sign_card_ed25519`, with a PKCS#8
key and the key id a verifier will look the key up by. The protected header
carries `alg`, `typ: "JOSE"` and `kid` (spec §8.4.2), and the card is
canonicalized with protobuf field presence first (§8.4.1): a field holding
its default value is left out unless it is `REQUIRED` or `optional`. The
older `sign_agent_card`, which takes the key id as an `Option` and so could
break §8.4.2, is deprecated and refuses `None` since 0.15.0, and is removed
in 0.16.0: call `sign_card_es256` with the key id instead.
`sign_card_es256_with_jku` and `sign_card_ed25519_with_jku` add the `jku`
(the JWK Set URL, which must be `https://`) that §8.4.2 allows. A verifier
still chooses which key sets to trust; this SDK's verification never
follows a card's `jku`.

A card you hand to `RequestHandlerBuilder` must already declare the
extensions the builder would add (the failure and budget extensions always;
idempotency, lease and approval when the handler uses them): `build()`
refuses a signed card it would have to edit. Declare them, then sign
([Security](./security.md#agent-card-signing-and-the-part-people-miss)).

## Marking and signing content

An agent marks what it generated, then signs it. The signature covers the
whole message or artifact, marker included, so the marker cannot be removed
without breaking it.

```rust,no_run
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::message::Part;
use a2a_protocol_types::provenance::{ContentSigner, mark_ai_generated, sign_content};
# fn run(pkcs8: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
let signer = ContentSigner::ed25519("agent-2026-10", pkcs8)?;

let mut report = Artifact::new("summary", vec![Part::text("Q3 revenue rose 4%.")]);
mark_ai_generated(&mut report, Some("analyst-agent/1.4 (model: example-model)"));
sign_content(&mut report, &signer)?;

// Publish signer.public_jwk() in the JWK Set your callers verify against.
# Ok(())
# }
```

The receiving side:

```rust,no_run
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::provenance::verify_content;
use a2a_protocol_types::signing::Jwks;
# fn check(artifact: &Artifact, keys: &Jwks) -> Result<(), Box<dyn std::error::Error>> {
let provenance = verify_content(artifact, keys)?;
if provenance.ai_generated {
    println!("generated by {:?}", provenance.generator);
}
# Ok(())
# }
```

### Signing everything a server sends

`RequestHandlerBuilder::with_content_signer` signs every artifact, status
message and direct reply the server sends (ADR 0027): stream frames, pushes,
and what `SendMessage`, `GetTask`, `ListTasks`, `CancelTask` and
`SubscribeToTask` return. Each is signed as it is sent, so a chunk on a
stream verifies as that chunk and the merged artifact `GetTask` returns
verifies as the whole. Content that already carries a signature, such as
another agent's artifact you forward, keeps it. A task's history is not
signed: it holds the caller's messages as they arrived.

```rust,no_run
use a2a_protocol_server::{RequestHandlerBuilder, agent_executor};
use a2a_protocol_types::provenance::ContentSigner;
# fn run(pkcs8: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
struct Agent;
agent_executor!(Agent, |_ctx, _queue| async { Ok(()) });

let handler = RequestHandlerBuilder::new(Agent)
    .without_agent_card()
    .with_content_signer(ContentSigner::ed25519("agent-2026-10", pkcs8)?)
    .build()?;
# drop(handler); Ok(())
# }
```

An executor's event whose provenance value is malformed is refused when it is
written, so a stored task can always be signed when it is read.

### Verifying what arrives

Both ends take the key set of the agents you trust. On the server,
`with_content_keys` refuses a message whose signature fails before the
executor runs, and tells the executor what it found:

```rust,no_run
use a2a_protocol_server::RequestHandlerBuilder;
use a2a_protocol_server::request_context::ContentCheck;
use a2a_protocol_server::{EventEmitter, agent_executor};
use a2a_protocol_types::signing::Jwks;
use a2a_protocol_types::task::TaskState;
# fn run(trusted: Jwks) -> Result<(), Box<dyn std::error::Error>> {
struct Agent;
agent_executor!(Agent, |ctx, queue| async {
    match ctx.content_check() {
        ContentCheck::Verified(p) => println!("from {:?}", p.generator),
        // Unsigned content is unverified, whoever sent it.
        _ => println!("unverified"),
    }
    EventEmitter::new(ctx, queue).status(TaskState::Completed).await
});

let handler = RequestHandlerBuilder::new(Agent)
    .without_agent_card()
    .with_content_keys(trusted)
    .build()?;
# drop(handler); Ok(())
# }
```

On the client, `ClientBuilder::with_content_keys(keys)` checks every
artifact, status message and direct reply it receives; one whose signature
fails ends the call or the stream with `ClientError::ContentNotVerified`. On either side,
`require_signed_content()` refuses unsigned content as well, for a
deployment that acts only on content from agents it knows. Call it after
`with_content_keys`: before it, or without keys, it requires nothing. A2A has
no way to learn a peer's content keys, so you supply them: the same JWK Set
you trust for its card, or one its operator publishes.

`verify_content` fails in these cases:

- the content was changed after signing, marker included;
- it is unsigned;
- it was signed by a key that is not in the set.

### Limits

- **The agent decides what to mark.** The SDK cannot tell generated content
  from any other.
- **The marker lives in A2A metadata.** It does not travel with a file once
  the file is saved. Marking inside the file itself, with a watermark or an
  embedded C2PA manifest, is the generator's job. A file part's bytes,
  including any embedded C2PA manifest, pass through this SDK unchanged.
- **The signature covers the JSON this SDK's types serialize.** Content that
  another implementation extended with fields these types do not model will
  not verify after a round trip through them.
- **It signs content, not where it sits.** A task's state, an event's
  `append` and `lastChunk`, and the task an artifact belongs to are not
  covered, so a signed artifact can be replayed into another task.
- **A signature says which key signed, not that the content is true.**

The extension is described for other implementations in
[`docs/upstream`](https://github.com/tomtom215/a2a-rust/blob/main/docs/upstream/a2a-oversight-provenance-extensions-draft.md).
