<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0016: The public types are generated from `a2a.proto`

**Date:** 2026-10-15
**Status:** Accepted
**Author:** Tom F.

---

## Context

A2A v1.0.1 §1.4 (specification line 107): "SDK language bindings, schemas,
and any other derived forms **MUST** be regenerated from the proto (directly
or via code generation) rather than edited manually."

What this SDK had on 2026-10-15:

- The prost message types, behind the `proto` feature, are generated from
  `a2a.proto` by `crates/a2a-protocol-types/build.rs`. They carry the gRPC
  binding.
- The public serde types every binding exposes (`Task`, `Message`,
  `AgentCard` and the rest), about 20,600 lines in `a2a-protocol-types`,
  are hand-written. Golden fixtures, proptests and the proto-to-serde
  conversion tests check them, and `tests/proto_schema_sync.rs` keeps the
  proto copies byte-identical. Nothing checks that the types still say
  what the proto says, and nothing produces them from it.

The proto declares 44 messages and 2 enums. 28 have a Rust type of the same
name; the other 18 exist under another name (`SendMessageRequest` is
`MessageSendParams`, for example). The hand-written types also differ from
a literal translation of the proto in deliberate ways: `Artifact.id` for
`artifactId`, `snake_case` and v0.3 aliases, `Option<Vec<T>>` for an
optional repeated field, newtype IDs (`TaskId`), legacy enum spellings
accepted on input.

Audit register R49 put three options to the maintainer: (a) keep, (b)
generate, (c) keep and add a mechanical check. The maintainer chose (b) on
2026-10-14, and on 2026-10-15 the form of (b) described here.

## Decision

The public types are generated from `proto/a2a_v1/a2a.proto` by a tool in
this repository, the output is checked in, and CI fails when the output
and the proto disagree.

### The generator

`tools/a2a-codegen`, a Rust binary outside the workspace with its own
`Cargo.lock`, as `fuzz/`, `itk/` and `verification/loom-halt/` are. It is
never published, so its dependencies stay out of the published crates'
graph and out of the workspace's `cargo deny` gate; the lockfile and OSV
scans cover its lockfile like the others.

It parses the proto with `protox` (MIT OR Apache-2.0), a protobuf compiler
written in Rust, so no contributor or CI job needs `protoc`. The
`google.api.field_behavior` extension is read through `prost-reflect`'s
descriptor pool, which decodes extension options that plain `prost-types`
drops. Proto comments come from the parsed source info.

### The configuration

`crates/a2a-protocol-types/codegen.toml` says, for each proto message and
enum, where it goes and how it differs from the default translation:

- the Rust name and module, derives and type-level attributes;
- per field: the Rust name, the Rust type (a newtype, `Option`, a boxed
  type), serde attributes (`alias`, `default`, `skip_serializing_if`,
  `with`), and documentation where the proto comment is not what a Rust
  user needs.

Every entry names the proto element it changes, and the generator refuses
to run when one names an element the proto does not have. Every proto
message, enum, field and enum value is either generated or listed as
excluded with a reason. So a field added to, removed from or renamed in the
proto changes the output or stops the generator; neither side can drift
quietly.

The default translation, which most fields need nothing beyond:

| Proto | Rust |
|---|---|
| `string`, `bool`, `int32`, `int64` | `String`, `bool`, `i32`, `i64` |
| `repeated T` | `Vec<T>` |
| a message field | the generated Rust type |
| `google.protobuf.Struct`, `Value` | `serde_json::Value` |
| `google.protobuf.Timestamp` | `String` (RFC 3339, as ProtoJSON writes it) |
| field marked `REQUIRED` | the plain type |
| field not marked `REQUIRED` | `Option<T>`, `skip_serializing_if = "Option::is_none"` |
| JSON name | the proto's `json_name` (lowerCamelCase) |
| enum value | the proto value name (`TASK_STATE_COMPLETED`) |

### The output

`crates/a2a-protocol-types/src/generated/`, one file per Rust module, each
opening with a `@generated` header that names the generator and the proto's
SHA-256. The generator runs `rustfmt` on what it writes, so the repository's
`cargo fmt --check` and its own check agree. The hand-written modules
re-export the generated items, so every public path stays where it is
(`a2a_protocol_types::task::Task`), and keep their `impl` blocks and the
Rust-only types (the ID newtypes, helpers).

### The check

`cargo run --manifest-path tools/a2a-codegen/Cargo.toml -- --check`
regenerates in memory and fails, naming the first file that differs, when
the checked-in output is not byte-identical. It runs in CI's static checks.
After the migration, a second check refuses a hand-written `struct` or
`enum` outside `src/generated/` whose name is a proto message's Rust name.

### The public API

Where today's type already matches the proto, the generated one is the same
type: the same fields, serde behaviour and derives. Each deliberate
difference is a configuration entry with its reason. A difference that
cannot be kept is a breaking change, recorded in the upgrading guide, and
`cargo-semver-checks` continues to gate every release.

### Migration

In phases, each a tested commit, with the golden, proptest and conversion
tests unchanged and passing at every step:

1. The generator, the configuration schema and its validation, the check,
   and the two enums (`TaskState`, `Role`) with a few plain messages.
2. The agent card family.
3. Task, message, artifact and their events.
4. Requests, parameters and responses.
5. Security schemes and the `oneof`s (`Part`, `SecurityScheme`,
   `OAuthFlows`, `StreamResponse`, `SendMessageResponse`).
6. The shadowing check, then the ledger's §1.4 entry and register R49.

## Alternatives considered

### Generate with prost and pbjson, and make those the public types

The proto would be the only input, with no configuration. Rejected:
generated protobuf types carry enums as `i32`, every message field as
`Option`, and none of the ID newtypes or builders. Making them the public
types would break the whole public surface of `a2a-protocol-types` and
every consumer of it, for no conformance gain over this decision.

### Generate in `build.rs`

Rejected. Code generation would run in every downstream build. docs.rs,
IDEs and code review would see only generated code that exists in no file.
And the generator's dependencies would join the published crates' graph.

### Keep the hand-written types and add a mechanical check (option (c))

Rejected by the maintainer. A check detects drift but does not make the
types "regenerated from the proto", which is what §1.4 asks.

## Consequences

### Positive

- The types are derived from the proto, as §1.4 requires. Ledger entry
  §1.4 (line 107) can move from PARTIAL to TESTED once the migration ends.
- A proto change surfaces as a diff to the generated files, or as a refused
  configuration, rather than as a field nobody added.
- No `protoc` is needed anywhere.

### Negative

- The configuration grows with every deliberate difference, and someone
  has to read it. Each entry carries its reason, and an entry for a proto
  element that no longer exists is an error rather than dead text.
- Contributors change a type by changing the proto or the configuration
  and regenerating, never by editing `src/generated/`. CONTRIBUTING says so,
  and the check enforces it.
