<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Support matrix

What release 0.16 supports, how far each part is qualified, and for how long. Each entry names the file it is read from,
so it can be checked again. Where something is outside the guarantee, this page says so; the full policy is in
[`STABILITY.md`](https://github.com/tomtom215/a2a-rust/blob/main/STABILITY.md) and
[`SECURITY.md`](https://github.com/tomtom215/a2a-rust/blob/main/SECURITY.md).

## Products

| Product | Version | Status | Source |
|---|---|---|---|
| `a2a-protocol-types`, `a2a-protocol-client`, `a2a-protocol-server`, `a2a-protocol-sdk` | 0.16.0 | Supported, under `STABILITY.md`'s guarantee | each crate's `Cargo.toml` |
| `a2a-protocol-discovery` (mDNS discovery) | 0.16.0 | **Not published** (`publish = false`); its macOS CI leg is red (register row `CI1`) | `crates/a2a-protocol-discovery/Cargo.toml` |
| `a2a-protocol-slimrpc` (SLIMRPC binding) | 0.7.0 | **Preview.** Versioned on its own and outside the stability guarantee; publishable, never yet published | `bindings/a2a-protocol-slimrpc/Cargo.toml`, `STABILITY.md` §6 |
| `a2a` CLI (`tools/a2a-cli`) | 0.16.0 | **Preview.** Not a published crate; release binaries are built only when the maintainer sets `PUBLISH_CLI_BINARIES` | `tools/a2a-cli/Cargo.toml`, `packaging/README.md` |
| Examples, benchmarks, `tck/`, `itk/`, `book-tests/` | — | Not products | `STABILITY.md` §6 |

## Protocol and bindings

| | Supported | Graded by |
|---|---|---|
| A2A specification | v1.0.1; protocol version `1.0` | [Conformance Statement](./conformance-statement.md) |
| JSON-RPC 2.0 over HTTP (§9) | Yes | Official TCK, ACTS, in-repo `a2a-tck` |
| gRPC (§10), feature `grpc` | Yes | Official TCK, ACTS, in-repo `a2a-tck` |
| HTTP+JSON (§11) | Yes | Official TCK, ACTS, in-repo `a2a-tck` |
| WebSocket (this SDK's custom binding), feature `websocket` | Yes | In-repo `a2a-tck` and cross-binding equivalence only |
| SLIMRPC | Preview (above) | Its own tests |

## Rust

| | Value | Source |
|---|---|---|
| Minimum supported Rust version | 1.94 | workspace `Cargo.toml` (`rust-version`) |
| CI test matrix | `stable` and `1.94`, each on Linux, macOS and Windows | `.github/workflows/ci.yml`, job `test` |
| Edition | 2024 | workspace `Cargo.toml` |

The minimum supported Rust version is raised only in a minor release, for a reason the release notes give
(`STABILITY.md` §5).

## Features of the published crates

Feature flags are additive: enabling one never removes an API (`STABILITY.md` §4). Each published crate's features are
listed in its `Cargo.toml` and in [Feature Flags](../getting-started/installation.md#feature-flags); CI builds and lints every one of
them alone (`ci.yml`, job `features`, cargo-hack `--each-feature`).

| Crate | Default features |
|---|---|
| `a2a-protocol-server` | `tracing`, `otel`, `tls-rustls` |
| `a2a-protocol-sdk` | `tls-rustls`, `tracing`, `otel`, `otlp`, `webhook` |

The tool loop (`tool-loop`) and skills (`skills`) are optional features of the server and the SDK, under the same
guarantee as the rest of the crate.

## Databases

| Store | Supported | Schema | Qualified on |
|---|---|---|---|
| In memory | Yes, for `Profile::LocalDevelopment`; not durable | — | every test run |
| SQLite, feature `sqlite` | Yes, one process per database file | Task store: version 9 in 0.16 (8 in 0.15), recorded in the file's `schema_versions` table when opened with `with_migrations`; the other SQLite stores create or extend their tables in place | `sqlx` 0.9.0 with `libsqlite3-sys` 0.37.0 (`Cargo.lock`) |
| PostgreSQL, feature `postgres` | Yes, shared by replicas | Task store: version 9 in 0.16 (7 in 0.15), recorded in `schema_versions` when opened with `with_migrations` | PostgreSQL 16 in CI (`ci.yml`, service `postgres:16`); other major versions are not tested |

`deny.toml` carries one advisory waiver that concerns the SQLite stores: RUSTSEC-2026-0459 in `libsqlite3-sys`, which
`sqlx` 0.9.0 pins. `SECURITY.md` explains it.

## Sealed formats

These are what an upgrade must keep reading. Each carries its version. They are sealed by a `ContentCipher`: the bundled
one is `Aes256GcmCipher` (AES-256-GCM), and a cipher of your own can stand in its place.

| What is sealed | Stored as | Source |
|---|---|---|
| Task content (`EncryptedTaskStore`) | a `data` part `{"sealedPart": {"v": 1, "c": …}}` with media type `application/vnd.a2a-rust.sealed-part+json`, bound to the context `["a2a-sealed-part/v1", …]` | `store/encrypted/seal.rs` |
| Push-config secrets, push-outbox bodies | `a2a-sealed:v1:<base64>` | `push/cipher.rs`, `push/outbox/body.rs` |
| What audit records say about people (`AuditLog::with_privacy`) | `a2a-sealed:v1:<sid>:…`, one key per person | `audit/store.rs` (`SEALED_VALUE_PREFIX`, `sid_marker`) |
| A tool loop's journal checkpoint (`ToolLoop::with_journal_cipher`) | `a2a-sealed-run:v1:…` | `tool_loop/journal/mod.rs` (`SEALED_CHECKPOINT_PREFIX`) |
| Audit chain hashes (not sealed; the chain's integrity) | `sha256:<hex>` | `a2a-protocol-types/src/audit/record.rs` |

## Upgrades

| From | To | Status | Evidence |
|---|---|---|---|
| 0.15 | 0.16, code | Every API break named in the upgrade guide | [`migration-inventory.md`](https://github.com/tomtom215/a2a-rust/blob/main/docs/audits/2026-11-13-adversarial-review/migration-inventory.md) §1 to §3 |
| 0.15 | 0.16, SQLite data, and back | Rehearsed: task and privacy-sealed audit files written by one version, read and appended by the other, both ways | same, §4 |
| 0.15 | 0.16, PostgreSQL data | Tasks: carried across in the rolling-upgrade run below (a task 0.15 parked continued on 0.16 with its artifacts). Audit, push-config and the other stores: **not rehearsed**. Rollback: **not rehearsed** | [Running More Than One Replica](../deployment/horizontal-scaling.md#upgrading-a-fleet-to-this-release) |
| 0.15 and 0.16 side by side on PostgreSQL (a rolling upgrade) | | Supported: the database keeps old and new replicas off each other's turns (migration 9). Measured against a real 0.15.0 replica on PostgreSQL 17 on 2026-10-28 and again on 2026-11-14 ([Qualification Matrix](./qualification-matrix.md#rolling-upgrade)), and replayed with 0.15's own statements by `tests/turn_claims/upgrade.rs`. Not covered: two 0.15 replicas on one task, as in 0.15; a job halt before every replica runs 0.16 | same |
| 0.15 and 0.16 side by side on one SQLite file | | **Not supported**: replace the processes together | same |
| Older than 0.15 | 0.16 | Through each release's upgrade guide, one at a time | [Upgrading](./upgrading.md) |

## How long a line is supported

A minor line is supported while it is the newest: security fixes ship as a patch of the current line, and the day the
next minor line is published the previous one stops receiving fixes. There is no overlap window and no long-term-support
line. On the day 0.16.0 is published, 0.16.x becomes the supported line and 0.15.x stops; the release workflow refuses a
tag whose `SECURITY.md` does not list the line being released. (`SECURITY.md`, "Supported Versions".)
