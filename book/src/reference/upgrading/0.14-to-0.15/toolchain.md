<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Toolchain and dependencies

Part of [Upgrading from 0.14 to 0.15](../0.14-to-0.15.md). Each section names what breaks and what to change; the [upgrade guide](../../upgrading.md) has the policy and how to check your own code.

## The MSRV is 1.94, and the SQL stores use `sqlx` 0.9

The minimum supported Rust version goes from 1.88 to 1.94 in all four crates
and in the SLIMRPC binding, so a toolchain older than 1.94 no longer builds
them. STABILITY.md §5 lets a minor release raise it to adopt a new major
version of a direct dependency, and this is that case: `sqlx` 0.9 needs 1.94,
and its pool types are in the SQL stores' public constructors
(`SqliteTaskStore::from_pool` takes a `SqlitePool`,
`PostgresTaskStore::from_pool` a `PgPool`, and so do the other SQL stores). A
crate that builds one of those pools itself moves to `sqlx` 0.9 with this
release; with two `sqlx` majors in the build, its pool is a different type
from the one the store names, and the call stops compiling. A store that opens
its own pool from a URL needs only the toolchain.

```text
# 0.14
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }
```

```toml
# 0.15
sqlx = { version = "0.9", features = ["runtime-tokio", "sqlite"] }
```

`sqlx` 0.9 also refuses a statement built at run time unless it is marked
`sqlx::AssertSqlSafe`; the library's own such statements are marked and built
from static text, so only a store of your own that formats SQL needs the same.
The two helpers the changelog lists as now taking `&'static str`
(`pg_schema::apply`, the SQLite `add_applied_seq`) are crate-internal, so
nothing you call changed.

## OpenTelemetry 0.33 for the `otel` and `otlp` features

`opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp` and the rest of the
family move from 0.32 to 0.33.1, and `tracing-opentelemetry` from 0.33 to
0.34.0. Types from them appear in `Telemetry`'s API, so move your own
dependencies on these crates to the same versions. Export now retries a
failed batch (four attempts by default), which is `opentelemetry-otlp`'s new
default, and `shutdown` reports only exports that were lost after retrying,
on gRPC as well as OTLP/HTTP.

## SLIMRPC is built on `agntcy-slim-rpc` 3.4

`a2a-protocol-slimrpc` goes from 0.6.0 to 0.7.0. It requires `0.15` of the
server, client and types crates, and moves its SLIM requirements to the line
`agntcy-slim-rpc` 3.4.1 needs: `agntcy-slim-rpc` 2.3 → 3.4, `agntcy-slim-auth`
0.15 → 0.17, `agntcy-slim-datapath` 0.18 → 0.19, `agntcy-slim-service`
0.12 → 0.13 and `agntcy-slim-config` 0.16 → 0.17; `agntcy-slim-session` stays
at 0.7. They are public dependencies: `SlimRpcServer::from_app`,
`SlimRpcTransport::from_app`, `SlimRpcMulticast::from_app` and the builders'
`with_identity` take SLIM types, so pass types from the same SLIM versions.
The binding's own code needed no change. Over the wire, SLIM 3.x sessions are
MLS and verify the peer's identity token on the request that sets one up; a
peer on the 2.x line was not tried against this binding. The binding is
outside the root workspace and versioned on its own, as in
[0.12 → 0.13](../0.12-to-0.13.md#also-in-013-not-breaking).

```text
# 0.14, bindings/a2a-protocol-slimrpc 0.6.0
a2a-protocol-slimrpc = "0.6"
agntcy-slim-rpc = "2.3"
```

```toml
# 0.15, bindings/a2a-protocol-slimrpc 0.7.0
a2a-protocol-slimrpc = "0.7"
agntcy-slim-rpc = "3.4"
```
