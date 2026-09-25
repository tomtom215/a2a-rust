# Idempotent Sends

A send whose connection drops *after* the request bytes are on the wire is
ambiguous: the task may exist, or it may not. The client will not retry it,
and that refusal is correct — `SendMessage` creates server-side state, so a
blind re-send can start a second task.

That leaves a caller with no good move. The only recovery is to list the tasks
on the context and pattern-match message content to guess whether the send
landed, which is heuristic, racy, and gets worse the more concurrent work you
delegate.

An idempotency key removes the guess. You generate one, attach it to the
message, and the server deduplicates on it: a retry returns the task the first
attempt created, in whatever state it has reached, and the agent runs once.

This is an extension, not part of A2A v1.0. It is identified by
`https://a2a-rust.com/extensions/idempotency/v1`, so a server that implements
it stays conformant and the TCK is unaffected.

## Attaching a key

```rust
use a2a_protocol_types::idempotency::{IDEMPOTENCY_EXTENSION_URI, key_of, set_key};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};

let mut message = Message {
    id: MessageId::new("msg-1"),
    role: MessageRole::User,
    parts: vec![Part::text("summarise the incident")],
    context_id: None,
    task_id: None,
    reference_task_ids: None,
    extensions: None,
    metadata: None,
};

set_key(&mut message, "8f14e45fceea167a5a36dedd4bea2543").expect("a valid key");

// The key travels in `metadata`; `extensions` declares the URI alongside it.
assert_eq!(
    key_of(&message).expect("a well-formed key"),
    Some("8f14e45fceea167a5a36dedd4bea2543"),
);
assert!(
    message
        .extensions
        .as_ref()
        .expect("set_key declares the extension")
        .iter()
        .any(|uri| uri == IDEMPOTENCY_EXTENSION_URI)
);
```

`Message::extensions` is a list of extension *URIs* and cannot carry a value,
which is why the key itself lives in `Message::metadata` under
`a2a-rust.com/idempotency-key`. On the wire:

```json
{
  "extensions": ["https://a2a-rust.com/extensions/idempotency/v1"],
  "metadata": { "a2a-rust.com/idempotency-key": "8f14e45fceea167a..." }
}
```

## Choosing a key

**A key must be unguessable.** Within a tenant it is the handle to a task:
anyone who can present it can reach the task it created. Generate it the way
you would a session token — at least 16 bytes from a CSPRNG, rendered as hex or
URL-safe base64 — and never derive it from message content, which would let one
caller collide with another's task by sending the same text.

```rust
use a2a_protocol_types::idempotency::{KeyError, MAX_KEY_LEN, MIN_KEY_LEN, validate_key};

// Long enough to be a key rather than a counter or a word.
assert_eq!(validate_key("retry"), Err(KeyError::TooShort));
assert!(validate_key(&"a".repeat(MIN_KEY_LEN)).is_ok());
assert_eq!(validate_key(&"a".repeat(MAX_KEY_LEN + 1)), Err(KeyError::TooLong));

// ASCII alphanumerics and `-`, `_`, `.`, `:` only. A key reaches store keys
// and log lines, and a newline in either is where log injection starts.
assert_eq!(
    validate_key("aaaaaaaaaaaaaaaa\n"),
    Err(KeyError::InvalidCharacter),
);
assert!(validate_key("tenant-a:8f14e45fceea167a").is_ok());
```

`MIN_KEY_LEN` rejects the obvious mistakes. It cannot detect a long key with
little entropy, so the CSPRNG rule is yours to keep.

A key is scoped to the **tenant**, not to the context. A retry of a send whose
`context_id` the server assigned would otherwise land in a fresh context and
miss the deduplication entirely — which is the very case this exists for.

## What the client does with it

Attaching a key changes the retry decision. `SendMessage` and
`SendStreamingMessage` are normally not retried after an ambiguous failure; a
keyed send is, because the retry cannot duplicate work.

**That holds only against a peer known to honour the key**, and the client
will not assume it. `ClientBuilder::from_card` reads the advertisement off
the agent card and sets it; a client built for a bare endpoint can assert it
with `ClientBuilder::with_peer_honouring_idempotency(true)`. Without that
evidence a keyed send stays exactly as retryable as an unkeyed one, which is
to say not at all after an ambiguous failure.

This paragraph used to say the property held even against a server without
the extension, "because such a server refuses a keyed send outright". That is
true of *this* SDK's server, which refuses a keyed send when its store cannot
honour one — and of nothing else. The key travels in `Message.metadata`,
which A2A defines as free-form, and the extension is deliberately not part of
A2A v1.0: a conformant Python, Java, Go or JavaScript server has never heard
of the URI, ignores the metadata, and runs the send. Retrying against one
starts a second task. Nor is there a handshake that would fix it — A2A's
`A2A-Extensions` header is the server reporting what it activated, not a
demand a server must reject — so the agent card is the evidence, and the
client requires it.

## Knowing whether a server honours it

A server advertises the extension on its agent card exactly when its configured
store can honour a key:

```rust
use a2a_protocol_types::agent_card::AgentCapabilities;
use a2a_protocol_types::extensions::AgentExtension;
use a2a_protocol_types::idempotency::IDEMPOTENCY_EXTENSION_URI;

let mut capabilities = AgentCapabilities::none();
capabilities.extensions = Some(vec![AgentExtension::new(IDEMPOTENCY_EXTENSION_URI)]);

let honours_keys = capabilities
    .extensions
    .as_ref()
    .is_some_and(|exts| exts.iter().any(|e| e.uri == IDEMPOTENCY_EXTENSION_URI));
assert!(honours_keys);
```

The advertisement is derived from the store rather than configured by hand, so
the card and the behaviour cannot drift: swapping in a store without the index
removes the advertisement in the same change.

## The three outcomes

| What arrives | What happens |
|---|---|
| A key nobody holds | The send proceeds and the key is claimed. |
| The **same message** again | The task the first send created is returned. Nothing executes. |
| A **different message**, same key | `InvalidParams`. The send is refused. |

The third is the one worth understanding. A genuine retry resends the identical
message — you kept it in order to resend it — so a different message id means
the key was reused across two distinct sends. Returning the first task there
would hand you a result for a message you did not just send, and you would act
on it. A silent wrong answer is the failure you can least detect, so it is
reported instead.

A key outlives its task on purpose. If a retention sweep removed the task, a
retry reports `TaskNotFound` rather than finding the key free and running the
send a second time.

## Server support

Every bundled store honours keys: in-memory, `SQLite`, `PostgreSQL`, and the
tenant-aware variants of each. Nothing needs enabling — a server built with any
of them advertises the extension and deduplicates.

```rust
use a2a_protocol_server::store::{InMemoryTaskStore, TaskStore};

assert!(InMemoryTaskStore::new().supports_idempotency());
```

A **custom** store does not, until it implements
`TaskStore::claim_idempotency_key` and `TaskStore::release_idempotency_key` and
overrides `supports_idempotency` to return `true`. That default is deliberate:
a store that has not implemented the index produces a missing advertisement and
a refused keyed send, never a send that quietly runs twice.

Two obligations if you write one. The claim must be **atomic** — two racing
claims of one key must resolve to exactly one winner, or the send executes
twice and the key is worse than useless. And the key must **not** be deleted
with its task; a foreign key with `ON DELETE CASCADE` would free it when a
sweep removed the task, which re-opens the double-execution it was presented to
prevent.
