<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# 0.15 → 0.16 migration inventory (adversarial review N14)

The CI semver job reported every check *skipped* for the four published crates, because a 0.x minor release may break
compatibility and the tool says so. That is correct of the tool, and it means a green job is no evidence that the upgrade was
reviewed. This file is that review, made on 2026-11-13 against the tree at the commit that adds it, with `v0.15.0`
(`e4fc3bc1`) as the baseline. It is a **migration inventory**, not a semantic-versioning promise: it lists what an adopter of
0.15 meets, and checks that the upgrade guide (`book/src/reference/upgrading/0.15-to-0.16.md`) names each item.

Toolchain: `rustc 1.100.0 (0ef1bc386 2026-11-09)`, `cargo-semver-checks 0.51.0`, `cargo-public-api 0.52.0`, all features.

## 1. Lints that would fail a minor release

`cargo semver-checks check-release --baseline-rev v0.15.0 --release-type minor --all-features` with the four crates. Forcing
`minor` makes the tool run the lints it skips for a 0.x minor bump.

| Crate | Checks | Failed lints | Items |
|---|---|---|---|
| `a2a-protocol-types` | 202: 200 pass, 2 fail, 58 skip | `function_missing`, `struct_marked_non_exhaustive` | `signing::sign_agent_card` removed; 39 structs now `#[non_exhaustive]` |
| `a2a-protocol-client` | 202: 198 pass, 4 fail, 58 skip | `auto_trait_impl_removed`, `enum_discriminants_undefined_non_unit_variant`, `enum_marked_non_exhaustive`, `inherent_method_const_removed` | `CardFetchOptions`, `ClientConfig`, `TlsConfig` no longer `UnwindSafe`/`RefUnwindSafe`; `TlsConfig` gains a data variant and `#[non_exhaustive]`; `ClientBuilder::without_tls`, `ClientConfig::with_tls` no longer `const` |
| `a2a-protocol-server` | 202: 199 pass, 3 fail, 58 skip | `auto_trait_impl_removed`, `inherent_method_const_removed`, `inherent_method_now_returns_unit` | `AuthFailureThrottle` no longer `UnwindSafe`/`RefUnwindSafe`; `Server::with_config`, `JwtAuthInterceptor::new` no longer `const`; `Telemetry::install_subscriber` returns `()` |
| `a2a-protocol-sdk` | 202: 202 pass, 58 skip | none | |

Each item is named in the upgrade guide. For the 39 structs this was checked name by name, against the guide's section
"Protocol structs are `#[non_exhaustive]`", by script: none is missing.

## 2. The whole public surface

`cargo-semver-checks` has no lint for most signature changes, for example a changed parameter or return type, or a changed
trait method. So the public API of each crate was also listed with `cargo public-api -sss diff`. The rustdoc JSON was built by
`RUSTC_BOOTSTRAP=1 cargo rustdoc --all-features --lib -- -Z unstable-options --output-format json` in a `v0.15.0` worktree
and at HEAD.

The JSON `cargo-semver-checks` leaves under `target/semver-checks` documents **private items as well**. A diff of those files
reported 713 lines, most of them crate-private (`handler::messaging::Started`, `RequestHandler::commit_task`). Only the
public build is used here.

| Crate | Removed | Changed | Added |
|---|---|---|---|
| `a2a-protocol-types` | 1 | 78 | 245 |
| `a2a-protocol-client` | 3 | 1 | 271 |
| `a2a-protocol-server` | 6 | 4 | 1,271 |
| `a2a-protocol-sdk` | 0 | 0 | 9 |

These are unique items. An item reachable by two paths (a module path and a root re-export) is printed twice by the tool and
counted once here. What each removed or changed item is:

* **types.** `sign_agent_card` removed (guide: "`sign_agent_card` is removed"). The 78 changed lines are the 39 structs of
  §1, each by two paths, each gaining `#[non_exhaustive]`.
* **client.** The `const` removals and `TlsConfig` of §1. `tls::HttpsClient` was a type alias and is a struct (CHANGELOG,
  "`tls::HttpsClient` is a struct, not a type alias").
* **server.**
  * The `const` and return-type changes of §1.
  * `RequestContext::context_id` and `RequestContext::new` take a `ContextId` (guide: "`RequestContext.context_id` is a
    `ContextId`").
  * `ToolLoop::with_meter` takes `&Meter` (guide, "Telemetry").
  * `BearerTokenTenantResolver::new` becomes `const` (compatible).
  * `impl Default for BearerTokenTenantResolver` and `default()` are listed as removed. **They are not.** The impl is now
    `#[derive(Default)]` (`crates/a2a-protocol-server/src/tenant_resolver.rs:242`), and `-sss` hides derived impls. The
    guide's "The default bearer-token tenant is a digest" covers the behaviour change.

No removed or changed public item is missing from the guide or the Unreleased CHANGELOG.

## 3. Real 0.15 applications compiled against 0.16

Every example the repository shipped at `v0.15.0` was copied over a HEAD worktree. The 16 example packages are the same set
as today's. Each was then checked with `cargo check -p <example> --all-targets --all-features`, with no source change. The
examples take the SDK through the workspace's path dependencies, so no version requirement had to be edited.

* **Unchanged and compiling:** `hello-agent`, `langfuse-agent`, `live-model`, `tool-loop-agent`.
* **Not compiling:** the other 12, in 39 distinct source locations. Many examples share the harness library, so one location
  can fail several packages. Every error is one of two kinds:
  * **E0639** "cannot create non-exhaustive struct using struct expression": a struct literal of one of the 39 structs, in
    `examples/harness`, `deploy-agent`, `mcp-agent`, `mcp-bridge`, `resilient-agent` and `swarm`. The guide gives the
    constructors and `with_*` setters to use instead.
  * **E0277** "`String: From<ContextId>` is not satisfied": `ContextId::new(ctx.context_id.clone())` in `mcp-bridge`, where
    `ctx.context_id` is already a `ContextId`. The guide's `ContextId` section covers it.

No other compile error appeared. A deprecation warning is not an error, and these were not counted.

## 4. Data written by 0.15, read by 0.16, and back (SQLite)

One program was built twice, against `v0.15.0` and against HEAD, each with that tree's `Cargo.lock`. It uses the
`SqliteTaskStore`, and a `SqliteAuditStore` under `AuditLog::with_privacy` with the same key-encryption key. The steps:

1. 0.15 saved a completed task and appended 3 records whose actor is sealed.
2. 0.16 opened both files: the task read back, the chain verified (`records=4`, `failure=None` after its own append), and
   every actor revealed.
3. **Rollback:** 0.15 opened the files 0.16 had written, appended, and verified the chain (`records=5`, `failure=None`); every
   actor revealed, including the one 0.16 sealed.
4. 0.16 read all 5 again.

Schema: 0.16 adds the table `a2a_task_claims` and its index `idx_a2a_task_claims_owner` to the task database, and nothing
else. 0.15 ignores both. The audit database's schema is identical (compared as `sqlite_master`).

## Not covered

* PostgreSQL stores: no 0.15 binary wrote to PostgreSQL here.
* The push-config, tenant-scoped, encrypted-task and halt stores, and the step journal, which is new in 0.16 and has no 0.15
  data.
* Signed checkpoints, legal holds and erasure carried across the upgrade.
* A second process running 0.15 and 0.16 side by side, as in a rolling upgrade, in this rehearsal. For tasks on PostgreSQL that was
  measured earlier, against a real 0.15.0 replica on 2026-10-28 (`book/src/deployment/horizontal-scaling.md`, *Upgrading a fleet*).
* Configuration files: the SDK reads none. Environment-variable changes are in the guide's "The server's outbound HTTP
  follows the proxy and CA variables" and the `Telemetry` section.
