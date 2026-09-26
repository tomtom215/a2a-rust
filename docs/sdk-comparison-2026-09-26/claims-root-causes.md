<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

> Appendix to [`deep-dive.md`](deep-dive.md) §2. Paths under `/opt/bench/` refer to the machine the investigation ran on. The repro crate and scratch clone were not committed; the patches are in [`patches/`](patches/). **F3 is withheld** (security; see `SECURITY.md`).

# a2a-rust 0.14.0: root-cause investigation of six defects (F1–F4, P1, P2)

**Subject:** crates.io `a2a-protocol-{types,client,server,sdk} = 0.14.0` (sources at `/opt/bench/crates/a2a-protocol-*-0.14.0/`; byte-identical to `crates/*/src` at `10f3435`, which is tag `v0.14.0`).
**Date:** 2026-09-26. **Author:** investigation agent (independent of the earlier `claims-audit.md` author).

**Evidence labels:**
- **VALIDATED**: I ran it; the command and an output excerpt are shown.
- **SOURCE-CONFIRMED**: I read the code or the git history.
- **CONJECTURED**: my inference, not checked.

**Artifacts** (none under `/home/user/a2a-rust`; that tree was not modified):

| What | Path |
|---|---|
| Independent repro crate (depends only on crates.io `=0.14.0`; ports 7700–7759) | `/opt/bench/deep-claims/deep/` (`src/lib.rs`, `tests/{f1_card_caching,f2_ready,f3_path_tenant,f4_verify_key,p1_private_ca,p2_rate_limit,other_instances}.rs`) |
| Raw repro output | `/opt/bench/deep-claims/{f1,f2,f3,f4,p1,p2,other}.out` |
| Scratch clone at `10f3435`, with the patches applied (uncommitted) | `/opt/bench/deep-claims/repo` |
| Patches, one per item plus the combined patch | `patches/{F1,F2,F4,P1,P2,TESTS-public}.diff` (F3 and the combined diffs are withheld). Each applies alone to `10f3435` (`git apply --check`, VALIDATED). |
| Regression-test output before and after the patches | `/opt/bench/deep-claims/regress-*.out` |
| Copy of the prior claims suite (built, not re-run; I reproduced each item independently instead) | `/opt/bench/deep-claims/suite/` |

**Commands:**
- Repro commands used `CARGO_TARGET_DIR=/opt/bench/deep-claims/target cargo test -j 2 --offline --test <name> -- --nocapture` from `/opt/bench/deep-claims/deep`.
- Patch verification used `CARGO_TARGET_DIR=/opt/bench/deep-claims/target-repo cargo test -j 2 --offline -p <crate> ...` from `/opt/bench/deep-claims/repo`.
- The final after-patch run also set `CARGO_PROFILE_DEV_DEBUG=0`.
- Both target dirs were deleted when I finished, to free disk.

---

## Summary table

| Item | Confirmed? | Root cause (0.14.0) | Introduced (first tag) | Claim introduced / ever true? | Severity | Patch verified |
|---|---|---|---|---|---|---|
| F1 | Yes, plus a detail the audit missed | `axum_adapter.rs:518-523` serves `axum::Json(card)` and bypasses `StaticAgentCardHandler`. `dynamic_handler.rs:86,127` use `now()`. | Router: `406aff3b` 2026-03-18 (v0.3.0). Dynamic handler: `bb01727f` 2026-03-15 (v0.2.0). | README caching claim: `bb01727f` (v0.2.0). True for REST in v0.2.0 and for JSON-RPC from v0.3.0. **Never true for `A2aRouter`.** | Low | Yes: 2 regression tests fail before and pass after |
| F2 | Yes; one wording refined | `rest/mod.rs:120-126` returns the constant `health_response()` for `/ready`. JSON-RPC routes every GET to the JSON-RPC parser (`jsonrpc/mod.rs:98-197`). | REST `/ready` constant: `4aab29eb` 2026-03-15 (v0.2.0). Real probe added **only** to `A2aRouter`: `c86aec31` 2026-08-16 (v0.9.0). | "probes the task store" README: `823dc21e` 2026-08-16 (v0.9.0). **Never true for `RestDispatcher`.** | Medium | Yes: fails before, passes after |
| F3 | Yes, and more serious than reported | Withheld (security; see F3 section) | Resolver: `88b0aae1` 2026-03-17 (v0.3.0) | README "(header, bearer, path)": `2c1fac36` (v0.3.0). Never true over HTTP. | Medium (security-relevant) | Yes (held privately) |
| F4 | Yes | `signing.rs:450` documents SPKI DER. `signing.rs:480-481` passes the bytes to ring `ECDSA_P256_SHA256_FIXED`, which takes the raw point. | `bb01727f` 2026-03-15 (v0.2.0) | The same commit wrote the doc. **Never true.** | Medium | Yes: fails before, passes after |
| P1 | Yes, **wider than reported** | Transports hard-code `default_tls_config()` (`transport/jsonrpc.rs:140-143`, `transport/rest/mod.rs:166-169`). So do `discovery.rs:350` and `token_provider.rs:694-696`. | TLS module and doc: `785bd082` 2026-03-15 (v0.2.0). No wiring in any release. | `tls.rs:15-16` "pass it to the client builder" since v0.2.0. **Never true.** | Medium | Yes: fails to compile before (no API), passes after |
| P2 | Yes; the behaviour is spec-permitted | `rate_limit/window.rs:70,157` return `A2aError::internal`. `identity.rs:45` falls back to `"anonymous"`. Interceptors cannot return `ServerError::Overloaded`. | `e1f51bdb` 2026-03-16 (v0.3.0). Its own doc then promised `-32029`. | README "per-caller": `e1f51bdb` (v0.3.0). True only for authenticated or XFF-trusted callers. | Medium | Yes: fails before, passes after |

**Regression output:**
- Before (`/opt/bench/deep-claims/regress-server-before.out`): `test result: FAILED. 0 passed; 5 failed`.
- After (`regress-final-after.out`):
  - `deep_regressions`: `5 passed`.
  - `signing_tests`: `31 passed`.
  - `deep_private_ca`: `1 passed`.
  - Affected lib unit modules: `tenant_resolver` 23, `rate_limit` 41, `agent_card` 68, `error` 100, all passed.
- `cargo clippy -p a2a-protocol-{types,server,client} --lib --tests` (workspace lints) is clean after two small lint fixes.

**Not run:** the upstream integration test binaries that bind `127.0.0.1:0` (for example `dispatch_edge_tests`, `axum_adapter_tests`), because their ephemeral ports fall outside the permitted 7500–7999 range. The one-line assertion change in `dispatch_edge_tests.rs` (F2) is therefore **compiled (clippy --tests) but not executed**.

---

## F1: `A2aRouter` agent card has no ETag / Last-Modified / 304; `DynamicAgentCardHandler` Last-Modified = now()

### 1. Reproduction (VALIDATED)

`cargo test --test f1_card_caching -- --nocapture --test-threads 1` (`/opt/bench/deep-claims/f1.out`):

```
F1 JsonRpcDispatcher: GET -> 200 etag=Some("W/\"16654ae07521d610\"") last-modified=Some("Sat, 26 Sep 2026 12:17:08 GMT") cache-control=Some("public, max-age=3600")
F1 JsonRpcDispatcher: If-None-Match W/"16654ae07521d610" -> 304 body_len=0
F1 RestDispatcher: GET -> 200 etag=Some(...) last-modified=Some(...) cache-control=Some("public, max-age=3600")
F1 RestDispatcher: If-None-Match W/"16654ae07521d610" -> 304 body_len=0
F1 A2aRouter: GET -> 200 etag=None last-modified=None cache-control=None
F1 A2aRouter: If-None-Match W/"16654ae07521d610" -> 200 body_len=648
F1 dynamic: first lm=Sat, 26 Sep 2026 12:17:07 GMT etag=W/"44ec61a8512ff804"; IMS=<first lm> 1.1s later -> 200 OK lm=Sat, 26 Sep 2026 12:17:08 GMT etag=W/"44ec61a8512ff804"
F1 static: IMS=own lm -> 304 Not Modified; IMS=Fri, 31 Dec 2100 23:59:59 GMT (after lm) -> 200 OK
```

**The prior finding is confirmed.** Two details the audit did not state:

- **(a)** The `A2aRouter` card response also lacks `access-control-allow-origin`, which the static handler sets (`other.out`: `A2aRouter GET card -> 200 access-control-allow-origin=None` vs `RestDispatcher ... Some("*")`). Browser clients cannot fetch the card cross-origin from `A2aRouter`.
- **(b)** All handlers compare `If-Modified-Since` by exact string (`caching.rs:130`, `dynamic_handler.rs:157`). A later date that should still give 304 gets 200 (last line above).

### 2. Root cause (SOURCE-CONFIRMED)

- `server/src/dispatch/axum_adapter.rs:166` routes the card to `handle_agent_card`. At `:518-523` that handler returns `axum::Json(card).into_response()`.
- It never uses `StaticAgentCardHandler`. The JSON-RPC and REST dispatchers construct that handler in their constructors (`jsonrpc/mod.rs:63-66`, `rest/mod.rs:62-65`) and call it at `jsonrpc/mod.rs:112-116` and `rest/mod.rs:156-162`.
- `server/src/agent_card/dynamic_handler.rs:86` and `:127` compute `format_http_date(SystemTime::now())` per request. So the `Last-Modified` of an unchanged card advances every second.
- `is_not_modified` (`:156-157`) then compares `If-Modified-Since` by string equality against that fresh value. It can only match within the same wall-clock second.

### 3. History (SOURCE-CONFIRMED)

| Event | Commit |
|---|---|
| Caching machinery, `DynamicAgentCardHandler` with `now()`, and the README claim ("HTTP caching (ETag, Last-Modified, 304 Not Modified) ... are implemented") | `bb01727f` 2026-03-15 "Phase 8: HTTP caching, agent card signing …" (first tag **v0.2.0**) |
| Current README wording "…for agent card endpoints" | `2c1fac36` 2026-03-17 (v0.3.0) |
| At v0.2.0 only the REST dispatcher served the card (`card_handler` in `dispatch/rest.rs`) | — |
| JSON-RPC got the card | `69bfd1ca` 2026-03-16 (v0.3.0) |
| `A2aRouter` created, serving `axum::Json(card)` from day one (verified with `git show 406aff3b`) | `406aff3b` 2026-03-18 "feat: add TCK wire format conformance tests and Axum framework integration" (**v0.3.0**) |
| Last touch of that line (response content-type work; the handler was left as-is) | `dfc69ed2` 2026-09-25 |

**Was the claim ever true?**
- For the dispatchers: yes (REST since v0.2.0, JSON-RPC since v0.3.0).
- For `A2aRouter`: **never**, from v0.3.0 through v0.14.0.
- For the dynamic handler: Last-Modified revalidation never worked across a second boundary. ETag revalidation always worked.

### 4. Why tests missed it (SOURCE-CONFIRMED)

- `server/tests/axum_adapter_tests.rs:202-210` (`axum_agent_card_discovery`) asserts status 200 and body fields only. It never checks a header.
- The caching tests are unit tests of the handlers themselves:
  - `static_handler.rs:109+`
  - `caching.rs:288-368`
  - `server/tests/dynamic_handler_tests.rs` (22 tests)
- No test drives any router or dispatcher with a conditional request. `git grep -i if-none-match 10f3435 -- crates/a2a-protocol-server/tests` matches only `dynamic_handler_tests.rs`.
- `dynamic_handler_tests.rs:257-275` (`if_modified_since_matching_returns_304`) issues two requests back-to-back. It passes only because both land in the same HTTP-date second, so it is **timing-dependent and masks the bug**. A request pair that straddles a second boundary would fail it (my regression test sleeps 1.1 s).

### 5. Blast radius

- **Who is affected:** users serving via `A2aRouter` (the `axum` feature) and users of `DynamicAgentCardHandler` / `HotReloadAgentCardHandler`.
- **Effect:** every card fetch transfers the full body. Clients/CDNs cannot revalidate, and `A2aRouter` has no `Cache-Control`, so a caching client falls back to heuristics or its own default.
- **Severity: Low.** The impact is bandwidth and latency only. No correctness or security impact (CONJECTURED: nothing security-relevant depends on the caching headers). The missing CORS header on `A2aRouter` is a functional issue for browser clients, also Low.

### 6. Spec relevance

- A2A spec §8.6 (`specification.md:2191-2203`):
  - Card endpoints "**SHOULD** include a `Cache-Control` … `max-age`" and "**SHOULD** include an `ETag`".
  - `Last-Modified` is **MAY**.
- `A2aRouter` therefore **violates two A2A SHOULDs**. This is not purely our own promise.
- RFC 9110 (§8.8.2 Last-Modified, §13.1.3 If-Modified-Since, cited from memory; no local copy on this machine):
  - Last-Modified should be when the representation was last modified, not the response time.
  - If-Modified-Since is a date comparison (unmodified since the given date means 304), not string equality.
  - Neither is a MUST violation: sending 200 is always safe. The effect is lost revalidation, not wrong data.

### 7. Fix (patch VALIDATED)

- `A2aRouter` builds one `StaticAgentCardHandler` in `into_router` and serves through it. This gives the same headers, the same 304 logic and the CORS header as the dispatchers.
- `DynamicAgentCardHandler` remembers `(etag, last_modified)` and advances `Last-Modified` only when the ETag changes.
- **Not fixed:**
  - Date-based IMS comparison.
  - Per-replica Last-Modified divergence (each replica stamps its own construction time; SOURCE-CONFIRMED at `static_handler.rs:47`).

```diff
diff --git a/crates/a2a-protocol-server/src/agent_card/dynamic_handler.rs b/crates/a2a-protocol-server/src/agent_card/dynamic_handler.rs
index 446f25c9..e54bbfc0 100644
--- a/crates/a2a-protocol-server/src/agent_card/dynamic_handler.rs
+++ b/crates/a2a-protocol-server/src/agent_card/dynamic_handler.rs
@@ -38,6 +38,9 @@ pub trait AgentCardProducer: Send + Sync + 'static {
 pub struct DynamicAgentCardHandler<P> {
     producer: P,
     cache_config: CacheConfig,
+    /// `(etag, last_modified)` of the last card served: `Last-Modified`
+    /// advances only when the card's content (its `ETag`) changes.
+    last_seen: std::sync::Mutex<(String, String)>,
 }
 
 impl<P: AgentCardProducer> DynamicAgentCardHandler<P> {
@@ -47,6 +50,7 @@ impl<P: AgentCardProducer> DynamicAgentCardHandler<P> {
         Self {
             producer,
             cache_config: CacheConfig::default(),
+            last_seen: std::sync::Mutex::new((String::new(), String::new())),
         }
     }
 
@@ -57,6 +61,19 @@ impl<P: AgentCardProducer> DynamicAgentCardHandler<P> {
         self
     }
 
+    /// The `Last-Modified` value for a card whose `ETag` is `etag`: the time
+    /// this handler first served that content, not the time of this request.
+    fn last_modified_for(&self, etag: &str) -> String {
+        let mut seen = self
+            .last_seen
+            .lock()
+            .unwrap_or_else(std::sync::PoisonError::into_inner);
+        if seen.0 != etag {
+            *seen = (etag.to_owned(), format_http_date(std::time::SystemTime::now()));
+        }
+        seen.1.clone()
+    }
+
     /// Handles an agent card request with conditional caching support.
     ///
     /// Serializes the produced card, computes an `ETag`, and checks
@@ -83,7 +100,7 @@ impl<P: AgentCardProducer> DynamicAgentCardHandler<P> {
             Ok(card) => match serde_json::to_vec(&card) {
                 Ok(json) => {
                     let etag = make_etag(&json);
-                    let last_modified = format_http_date(std::time::SystemTime::now());
+                    let last_modified = self.last_modified_for(&etag);
 
                     let not_modified = is_not_modified(
                         if_none_match.as_deref(),
@@ -124,7 +141,7 @@ impl<P: AgentCardProducer> DynamicAgentCardHandler<P> {
             Ok(card) => match serde_json::to_vec(&card) {
                 Ok(json) => {
                     let etag = make_etag(&json);
-                    let last_modified = format_http_date(std::time::SystemTime::now());
+                    let last_modified = self.last_modified_for(&etag);
                     hyper::Response::builder()
                         .status(200)
                         .header("content-type", "application/json")
diff --git a/crates/a2a-protocol-server/src/dispatch/axum_adapter.rs b/crates/a2a-protocol-server/src/dispatch/axum_adapter.rs
index 75ad2387..3fe51187 100644
--- a/crates/a2a-protocol-server/src/dispatch/axum_adapter.rs
+++ b/crates/a2a-protocol-server/src/dispatch/axum_adapter.rs
@@ -144,6 +144,14 @@ impl A2aRouter {
         // (2 MiB) and silently ignores `max_request_body_size`, so the knob that
         // works on the JSON-RPC/REST dispatchers would be a no-op here.
         let max_body = self.config.max_request_body_size;
+        // Same handler the JSON-RPC and REST dispatchers use, so the card gets
+        // ETag / Last-Modified / Cache-Control and conditional 304s here too.
+        let card_handler = Arc::new(
+            self.handler
+                .agent_card
+                .as_ref()
+                .and_then(|card| crate::agent_card::StaticAgentCardHandler::new(card).ok()),
+        );
         let state = A2aState {
             handler: self.handler,
             config: Arc::new(self.config),
@@ -163,7 +171,13 @@ impl A2aRouter {
             // Extended card
             .route("/extendedAgentCard", get(handle_extended_card))
             // Agent card discovery
-            .route("/.well-known/agent-card.json", get(handle_agent_card))
+            .route(
+                "/.well-known/agent-card.json",
+                get(move |req: axum::extract::Request| {
+                    let card_handler = Arc::clone(&card_handler);
+                    async move { handle_agent_card(card_handler.as_ref().as_ref(), &req) }
+                }),
+            )
             // Health check
             .route("/health", get(handle_health))
             .route("/ready", get(handle_ready))
@@ -515,10 +529,13 @@ async fn handle_extended_card(
     .await
 }
 
-async fn handle_agent_card(State(state): State<A2aState>) -> axum::response::Response {
-    state.handler.agent_card.as_ref().map_or_else(
+fn handle_agent_card(
+    card_handler: Option<&crate::agent_card::StaticAgentCardHandler>,
+    req: &axum::extract::Request,
+) -> axum::response::Response {
+    card_handler.map_or_else(
         || plain_error(404, "agent card not configured"),
-        |card| axum::Json(card).into_response(),
+        |h| h.handle(req).map(Body::new),
     )
 }
 
```

**Regression tests** (in `TESTS.diff`, file `crates/a2a-protocol-server/tests/deep_regressions.rs`):
- `f1_a2a_router_card_supports_conditional_get`
- `f1_dynamic_handler_last_modified_is_stable_for_unchanged_card`

Before: both FAILED (`panicked at deep_regressions.rs:114:31` "A2aRouter card must carry an ETag"; `:142:5` "unchanged card must revalidate via If-Modified-Since"). After: both ok.

---

## F2: `RestDispatcher /ready` is a constant; JSON-RPC answers GET probes with HTTP 200

### 1. Reproduction (VALIDATED)

`cargo test --test f2_ready -- --nocapture` uses a task store whose `count()` fails (`/opt/bench/deep-claims/f2.out`):

```
F2 JsonRpcDispatcher GET /health (dead store) -> 200 {"jsonrpc":"2.0","id":null,"error":{"code":-32009,"message":"A2A version '0.3' is not supported ..."}}
F2 JsonRpcDispatcher GET /ready (dead store) -> 200 {"jsonrpc":"2.0","id":null,"error":{"code":-32009,...}}
F2 JsonRpcDispatcher GET /ready with A2A-Version:1.0 -> 200 {"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error: EOF while parsing a value at line 1 column 0"}}
F2 JsonRpcDispatcher GET /no/such/path with A2A-Version:1.0 -> 200 {"jsonrpc":"2.0","id":null,"error":{"code":-32700,...}}
F2 RestDispatcher GET /health (dead store) -> 200 {"status":"ok"}
F2 RestDispatcher GET /ready (dead store) -> 200 {"status":"ok"}
F2 A2aRouter GET /health (dead store) -> 200 {"status":"ok"}
F2 A2aRouter GET /ready (dead store) -> 503 {"reason":"internal_error","status":"not_ready"}
```

**The prior finding is confirmed.** One refinement to the audit's wording: `-32009` appears only because a probe sends no `A2A-Version` header. The underlying fact is that `JsonRpcDispatcher` ignores the method and path of every non-card request. **Any** GET on **any** path returns HTTP 200, with `-32009` or `-32700`. A kubelet `httpGet` probe (success = 200–399) therefore always passes.

### 2. Root cause (SOURCE-CONFIRMED)

- `server/src/dispatch/rest/mod.rs:120-126`: `if method == "GET" && (path == "/health" || path == "/ready") { let mut resp = health_response(); … }`.
  - `health_response()` (`rest/response.rs:46-49`) is the constant `{"status":"ok"}`.
- `A2aRouter` has a separate `handle_ready` (`axum_adapter.rs:552`) that calls `RequestHandler::task_store_health()`, which is `pub`, in `handler/introspection.rs:80`.
- `jsonrpc/mod.rs:98-197`: after the OPTIONS and card checks, every request goes to Content-Type validation, version validation, then body parse. None of these checks method or path. JSON-RPC errors use HTTP 200 (`jsonrpc/response.rs:167-171`).

### 3. History (SOURCE-CONFIRMED)

| Event | Commit |
|---|---|
| REST `/health` + `/ready` constant (`path == "/health" \|\| path == "/ready"`) | `4aab29eb` 2026-03-15 "Audit fixes: DX improvements…" (**v0.2.0**) |
| Real readiness probe added, **to `A2aRouter` only** | `c86aec31` 2026-08-16 (v0.9.0) |
| README "split liveness (`/health`) / readiness (`/ready`, probes the task store) endpoints" | `823dc21e` 2026-08-16 (**v0.9.0**) |

- The `c86aec31` commit message and doc comment say "`/health` alone was the whole health surface for this SDK's life". That was false when written: `RestDispatcher` had answered `/ready` with a constant since v0.2.0. The author fixed the surface they were looking at and did not notice the REST dispatcher's pre-existing `/ready`.
- **Was the claim ever true?** Never for `RestDispatcher`. True for `A2aRouter` since v0.9.0.
- `book/src/deployment/production.md:299` states the JSON-RPC dispatcher has no health endpoints. It does not warn that probes against it return 200.

### 4. Why tests missed it (SOURCE-CONFIRMED)

- `server/tests/dispatch_edge_tests.rs:129-134` (`rest_ready_check`) asserts `200` and `body.contains("ok")` with a healthy in-memory store. It *enshrines* the constant.
- `server/tests/dispatch_tests/hardening.rs:67-79` (`rest_ready_endpoint_returns_ok`) asserts status 200 only.
- The failing-store tests (`axum_adapter.rs:1172+` `readiness_tests`, `UnreachableStore`) call `A2aRouter`'s `handle_ready` directly. Their doc comment says, ironically, that "an implementation where `/ready` also returned a constant would pass any test that only checked the happy path". That is exactly what the REST tests do.
- No test sends a GET to `JsonRpcDispatcher` other than the card.

### 5. Blast radius

- **Who is affected:**
  - Anyone using `RestDispatcher` (directly or via `serve*`) with a Kubernetes/ELB readiness probe on `/ready`.
  - Anyone probing a `JsonRpcDispatcher` port over HTTP at all.
- **Effect:** when the task store (Postgres/SQLite) is unreachable, the replica stays "ready", keeps receiving traffic, and fails requests. A probe on the JSON-RPC port can never fail while the process accepts TCP connections.
- **Severity: Medium.** The impact is availability and operability during partial outages. No security impact. Liveness is intentionally constant, which is correct.

### 6. Spec relevance

- The A2A spec defines no health endpoints (`grep -i health specification.md` finds none).
- JSON-RPC 2.0 defines no HTTP mapping.
- This is **purely our own promise** (README:87).

### 7. Fix (patch VALIDATED)

- REST `/ready` calls `task_store_health()` and answers `200 {"status":"ready"}` or `503 {"status":"not_ready","reason":<label>}`, the same as `A2aRouter`.
- `JsonRpcDispatcher` answers GET `/health` and `/ready` the same way.
- The existing `rest_ready_check` assertion changes `"ok"` → `"ready"`, because the body now matches `A2aRouter`'s. This is a small observable change.
- **Residual:** other non-POST requests to the JSON-RPC dispatcher still get 200 + a JSON-RPC error. A `405 Allow: POST` for non-POST would close that, but it goes beyond the minimal fix.

```diff
diff --git a/crates/a2a-protocol-server/src/dispatch/jsonrpc/mod.rs b/crates/a2a-protocol-server/src/dispatch/jsonrpc/mod.rs
index 076443fb..a7b41b77 100644
--- a/crates/a2a-protocol-server/src/dispatch/jsonrpc/mod.rs
+++ b/crates/a2a-protocol-server/src/dispatch/jsonrpc/mod.rs
@@ -120,6 +120,22 @@ impl JsonRpcDispatcher {
             return resp;
         }
 
+        // Liveness / readiness, answered as the REST dispatcher answers them:
+        // without this a GET probe reached the JSON-RPC parser and got
+        // HTTP 200 with a JSON-RPC error body, which an HTTP probe reads as
+        // healthy.
+        if req.method() == "GET" && matches!(req.uri().path(), "/health" | "/ready") {
+            let mut resp = if req.uri().path() == "/ready" {
+                crate::dispatch::rest::ready_response(&self.handler).await
+            } else {
+                crate::dispatch::rest::liveness_response()
+            };
+            if let Some(ref cors) = self.cors {
+                cors.apply_headers(&mut resp);
+            }
+            return resp;
+        }
+
         // Capture the raw A2A-Extensions request header before the request is
         // consumed, so the activated set can be echoed on the response
         // (official-SDK convention; lets clients see which requested
diff --git a/crates/a2a-protocol-server/src/dispatch/rest/mod.rs b/crates/a2a-protocol-server/src/dispatch/rest/mod.rs
index cf6c6321..3b060218 100644
--- a/crates/a2a-protocol-server/src/dispatch/rest/mod.rs
+++ b/crates/a2a-protocol-server/src/dispatch/rest/mod.rs
@@ -16,7 +16,7 @@ mod response;
 // The axum adapter answers errors through these, so the two HTTP+JSON
 // dispatchers send one error shape (audit N37).
 pub(crate) use error_response::{error_json_response, server_error_to_response};
-pub(crate) use response::json_ok_response;
+pub(crate) use response::{health_response as liveness_response, json_ok_response, ready_response};
 
 use std::collections::HashMap;
 use std::convert::Infallible;
@@ -118,7 +118,11 @@ impl RestDispatcher {
 
         // Health check endpoint.
         if method == "GET" && (path == "/health" || path == "/ready") {
-            let mut resp = health_response();
+            let mut resp = if path == "/ready" {
+                ready_response(&self.handler).await
+            } else {
+                health_response()
+            };
             if let Some(ref cors) = self.cors {
                 cors.apply_headers(&mut resp);
             }
diff --git a/crates/a2a-protocol-server/src/dispatch/rest/response.rs b/crates/a2a-protocol-server/src/dispatch/rest/response.rs
index b2581c31..ee057556 100644
--- a/crates/a2a-protocol-server/src/dispatch/rest/response.rs
+++ b/crates/a2a-protocol-server/src/dispatch/rest/response.rs
@@ -43,11 +43,27 @@ pub(super) fn internal_error_response() -> hyper::Response<BoxBody<Bytes, Infall
 }
 
 /// Returns a health check response.
-pub(super) fn health_response() -> hyper::Response<BoxBody<Bytes, Infallible>> {
+pub fn health_response() -> hyper::Response<BoxBody<Bytes, Infallible>> {
     let body = br#"{"status":"ok"}"#;
     build_json_response(200, body.to_vec())
 }
 
+/// Readiness: probes the task store, as `A2aRouter`'s `/ready` does.
+/// `200 {"status":"ready"}` or `503 {"status":"not_ready","reason":<label>}`.
+pub async fn ready_response(
+    handler: &crate::handler::RequestHandler,
+) -> hyper::Response<BoxBody<Bytes, Infallible>> {
+    match handler.task_store_health().await {
+        Ok(()) => build_json_response(200, br#"{"status":"ready"}"#.to_vec()),
+        Err(e) => build_json_response(
+            503,
+            serde_json::json!({"status": "not_ready", "reason": e.metric_label()})
+                .to_string()
+                .into_bytes(),
+        ),
+    }
+}
+
 /// Builds a JSON HTTP response with the given status and body.
 ///
 /// `application/json`, a deliberate deviation from §11.1, which says
diff --git a/crates/a2a-protocol-server/tests/dispatch_edge_tests.rs b/crates/a2a-protocol-server/tests/dispatch_edge_tests.rs
index 9d704a74..d4f41012 100644
--- a/crates/a2a-protocol-server/tests/dispatch_edge_tests.rs
+++ b/crates/a2a-protocol-server/tests/dispatch_edge_tests.rs
@@ -130,7 +130,7 @@ async fn rest_ready_check() {
     let addr = start_rest_server(make_handler()).await;
     let (status, body) = http_request(addr, "GET", "/ready", None, None).await;
     assert_eq!(status, 200);
-    assert!(body.contains("ok"));
+    assert!(body.contains("ready"));
 }
 
 #[tokio::test]
```

**Regression test:** `f2_ready_probes_the_store_on_rest_and_jsonrpc`. Before: FAILED `readiness must fail when the store is down: {"status":"ok"}` (REST). After: ok for both dispatchers.

---

## F3: `PathSegmentTenantResolver` does not work over HTTP

> **Withheld.** F3 is confirmed and is more serious than the earlier audit reported: it is security-relevant. Following `SECURITY.md`, its details, reproduction and patch are held for a private GitHub Security Advisory and are not published here.

## F4: `verify_agent_card` documents a DER SubjectPublicKeyInfo but requires the raw SEC1 point

### 1. Reproduction (VALIDATED)

`cargo test --test f4_verify_key -- --nocapture`. Keys come from **rcgen**, not ring, so the test does not share the SDK's assumptions (`/opt/bench/deep-claims/f4.out`):

```
F4 spki.len=91 raw.len=65 raw[0]=0x04 spki ends with raw: true
F4 verify(card, sig, SPKI DER)   -> Err(A2aError { code: InternalError, message: "signature verification failed", ... })
F4 verify(card, sig, raw point)  -> Ok(())
F4 verify(tampered, sig, raw)    -> is_err=true
F4 verify(card, sig, other raw)  -> is_err=true
F4 verify(card, sig, spki[26..]) -> Ok(())
```

**The prior finding is confirmed in full.**

### 2. Root cause (SOURCE-CONFIRMED)

- `types/src/signing.rs:450-451` documents `public_key_der` as "DER-encoded public key (`SubjectPublicKeyInfo`)".
- `:480-481` passes the bytes to `ring::signature::UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, public_key_der)`. ring's ECDSA verifier takes the uncompressed point `0x04||X||Y`, not an SPKI.
- The key-format failure surfaces as the same `"signature verification failed"` (`InternalError`) as a bad signature, so the caller gets no hint.

### 3. History (SOURCE-CONFIRMED)

- Introduced in `bb01727f` 2026-03-15 (**v0.2.0**). That commit wrote both the doc (`crates/a2a-types/src/signing.rs:197` at that commit) and the `UnparsedPublicKey` call (`:235`). Its tests used `key_pair.public_key().as_ref()` (`:333`, `:358`).
- Wording unchanged through v0.14.0.
- **Was the claim ever true?** Never.

### 4. Why tests missed it (SOURCE-CONFIRMED)

Every test and example obtains the key from ring's own `KeyPair::public_key()`, which is the raw point. No test uses an externally produced key or an SPKI:
- `types/tests/signing_tests.rs:60-72` (`generate_es256_keypair`)
- `signing.rs:635`, `:660` (unit tests)
- `examples/agent-team/src/tests/coverage_gaps/feature_gated.rs:70`
- `examples/incident-response/src/hardening/trust.rs:35`

The test oracle shares the implementation's assumption.

### 5. Blast radius and security

**Who is affected:** any user verifying cards with a key in the documented form. That includes keys from `openssl pkey -pubout -outform DER`, a PEM `PUBLIC KEY` decoded to DER, or most KMS/HSM export formats. A key taken from a JWKS (`x`/`y`) must be assembled into `0x04||x||y`, which is undocumented.

**Security:**
- It is fail-closed. There is no way to make a forged or tampered card verify; tampered and wrong-key cases still fail (VALIDATED).
- The risk is indirect. A user who follows the docs sees every card fail, may conclude verification is broken, and ships with verification disabled. Nothing in the SDK verifies automatically (the client `signing` feature is a forwarder; `installation.md:66`), so skipping it is one deleted line.
- A user who strips the 26-byte prefix gets correct verification. This is safe for P-256 only; the function hard-codes ES256.

**Severity: Medium.** A security feature is unusable as documented. Integrity itself is not compromised.

### 6. Spec relevance

- A2A §8.4.3 (`specification.md:2092-2101`) mandates the verification steps but not a key encoding.
- The key encoding is **purely our own rustdoc contract**.

### 7. Fix (patch VALIDATED)

- Accept both forms: strip the fixed 26-byte P-256 SPKI header when present, otherwise pass the bytes through (backward compatible).
- Correct the rustdoc to name both forms.
- **Regression test:** uses a fixture generated by **OpenSSL 3.0.13**, independent of ring: `openssl genpkey … P-256`, `openssl pkcs8 -topk8`, `openssl pkey -pubout -outform DER`. It signs with the OpenSSL PKCS#8 key and verifies with the OpenSSL SPKI.

```diff
diff --git a/crates/a2a-protocol-types/src/signing.rs b/crates/a2a-protocol-types/src/signing.rs
index f39e2842..21a390b1 100644
--- a/crates/a2a-protocol-types/src/signing.rs
+++ b/crates/a2a-protocol-types/src/signing.rs
@@ -396,6 +396,13 @@ pub fn signature_header(sig: &AgentCardSignature) -> A2aResult<SignatureHeader>
 
 // ── JWS Verification ────────────────────────────────────────────────────────
 
+/// DER header of a P-256 `SubjectPublicKeyInfo` (`id-ecPublicKey`,
+/// `prime256v1`, 66-byte BIT STRING); the uncompressed point follows it.
+const P256_SPKI_PREFIX: [u8; 26] = [
+    0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a,
+    0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
+];
+
 /// Verifies an [`AgentCardSignature`] against an [`AgentCard`] using the
 /// given public key.
 ///
@@ -447,8 +454,11 @@ pub fn signature_header(sig: &AgentCardSignature) -> A2aResult<SignatureHeader>
 ///
 /// * `card` — The agent card that was signed.
 /// * `sig` — The signature to verify.
-/// * `public_key_der` — DER-encoded public key (`SubjectPublicKeyInfo`),
-///   already established by the caller to be currently valid.
+/// * `public_key_der` — the P-256 public key, either DER-encoded
+///   (`SubjectPublicKeyInfo`, e.g. `openssl pkey -pubout -outform DER`) or
+///   as the raw 65-byte uncompressed point `0x04 || X || Y` (what ring's
+///   `KeyPair::public_key()` and a JWK's `x`/`y` give), already established
+///   by the caller to be currently valid.
 ///
 /// # Errors
 ///
@@ -476,9 +486,11 @@ pub fn verify_agent_card(
         return Err(A2aError::internal(format!("unsupported algorithm: {alg}")));
     }
 
-    // Verify with ES256.
-    let public_key =
-        signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, public_key_der);
+    // Verify with ES256. ring takes the raw point, so unwrap a P-256 SPKI.
+    let point = public_key_der
+        .strip_prefix(&P256_SPKI_PREFIX[..])
+        .unwrap_or(public_key_der);
+    let public_key = signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, point);
     public_key
         .verify(signing_input.as_bytes(), &sig_bytes)
         .map_err(|_| A2aError::internal("signature verification failed"))
diff --git a/crates/a2a-protocol-types/tests/signing_tests.rs b/crates/a2a-protocol-types/tests/signing_tests.rs
index 92bece17..f6bd2f47 100644
--- a/crates/a2a-protocol-types/tests/signing_tests.rs
+++ b/crates/a2a-protocol-types/tests/signing_tests.rs
@@ -470,3 +470,29 @@ fn verify_rejects_modified_protected_header() {
         "verification must fail when protected header is modified"
     );
 }
+
+// ── deep-claims F4: the documented key format (DER SubjectPublicKeyInfo) ────
+//
+// Fixture made with OpenSSL 3.0.13, independently of ring:
+//   openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -outform DER -out k.der
+//   openssl pkcs8 -topk8 -nocrypt -inform DER -in k.der -outform DER   # PKCS#8
+//   openssl pkey -inform DER -in k.der -pubout -outform DER           # SPKI
+const F4_PKCS8_HEX: &str = "308187020100301306072a8648ce3d020106082a8648ce3d030107046d306b02010104208b6e12af3b4d5b9db27aba7cd6376dd2fa7826f37479fa69fa044166887109f1a14403420004f274f10f617575549e7b5579f8479bfcdf001bacc53e97478efe8952ebd7c7ae2de28d2136a730a5fe9d9b9eab692f11cd93c39809e18fc2f3e3aaeeb8554808";
+const F4_SPKI_HEX: &str = "3059301306072a8648ce3d020106082a8648ce3d03010703420004f274f10f617575549e7b5579f8479bfcdf001bacc53e97478efe8952ebd7c7ae2de28d2136a730a5fe9d9b9eab692f11cd93c39809e18fc2f3e3aaeeb8554808";
+
+fn f4_hex(s: &str) -> Vec<u8> {
+    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
+}
+
+#[test]
+fn verify_accepts_the_documented_spki_der_key() {
+    let card = minimal_card();
+    let spki = f4_hex(F4_SPKI_HEX);
+    let sig = sign_agent_card(&card, &f4_hex(F4_PKCS8_HEX), Some("openssl")).unwrap();
+    verify_agent_card(&card, &sig, &spki).expect("SPKI DER, as the rustdoc documents, must verify");
+    // The raw point keeps working, and tampering is still caught with SPKI.
+    verify_agent_card(&card, &sig, &spki[26..]).unwrap();
+    let mut tampered = card.clone();
+    tampered.name = "Evil".into();
+    assert!(verify_agent_card(&tampered, &sig, &spki).is_err());
+}
```

- Before: `verify_accepts_the_documented_spki_der_key ... FAILED` (panicked at `signing_tests.rs:492:43`, the `expect("SPKI DER, as the rustdoc documents, must verify")`).
- After: `signing_tests` 31 passed. The existing raw-point tests still pass.

---

## P1: the JSON-RPC and REST client cannot trust a private CA

### 1. Reproduction (VALIDATED)

`cargo test --test p1_private_ca -- --nocapture`. The server is a `JsonRpcDispatcher` over tokio-rustls, with a leaf certificate issued by a private CA made with rcgen (`/opt/bench/deep-claims/p1.out`):

```
P1 (a) a2a tls::build_https_client_with_config(tls_config_with_extra_roots(CA)) GET card -> 200 OK (649 bytes)
P1 (b) ClientBuilder::new(https).build() send_message -> Err("HTTP client error: client error (Connect)")
P1 (b') REST binding send_message -> Err("HTTP client error: client error (Connect)")
P1 (c) with SSL_CERT_FILE=<CA pem> -> Err("HTTP client error: client error (Connect)")
P1 (d) resolve_agent_card(https private CA) -> Err("HTTP client error: client error (Connect)")
```

**The prior finding is confirmed, and it is wider than reported.**
- (a) shows the SDK's own TLS helpers work: the CA and certificate chain are valid, and only the wiring is missing.
- (c) and (d) are new:
  - The OS trust store and `SSL_CERT_FILE` are ignored, because roots are the compiled-in `webpki-roots`.
  - Card discovery (`resolve_agent_card`) and the OAuth token fetcher have the same hard-coding.
- The error text never mentions certificates.

### 2. Root cause (SOURCE-CONFIRMED)

- `client/src/tls.rs:12-16` says to use `tls_config_with_extra_roots` "then pass it to the client builder".
- No such API exists. `ClientBuilder` (`builder/mod.rs`) has `with_grpc_tls_config` for gRPC only.
- The HTTP transports' constructors build the client with a fixed config:
  - `transport/jsonrpc.rs:140-143`: `build_https_client_with_connect_timeout(crate::tls::default_tls_config(), …)`
  - `transport/rest/mod.rs:166-169`: the same.
- `discovery.rs:350` uses `build_https_client()`.
- `token_provider.rs:694-696` uses `default_tls_config()`.
- The only workaround is implementing the whole `Transport` trait and passing it via `with_custom_transport` (`builder/mod.rs:439`).
- Related: `ClientConfig.tls`/`TlsConfig` is never read by any transport. See "Other instances".

### 3. History (SOURCE-CONFIRMED)

- The `tls.rs` module and doc sentence date from `785bd082` 2026-03-15 "Phase 9: Production hardening — tracing, TLS, CI" (**v0.2.0**). At v0.2.0 the transports called `crate::tls::build_https_client()` (`transport/jsonrpc.rs:108`, `transport/rest.rs:216`).
- `4b9f455a` 2026-03-18 (v0.3.0) switched to `build_https_client_with_connect_timeout(default_tls_config(), …)`, still hard-coded.
- `c23f0d6f` (v0.3.0) added `tls_integration_tests.rs`.
- **Was the claim ever true?** Never.

### 4. Why tests missed it (SOURCE-CONFIRMED)

- `client/tests/tls_integration_tests.rs:163-260` exercises `tls::tls_config_with_extra_roots` + `tls::build_https_client_with_config` directly against a raw hyper client. It never goes through `ClientBuilder` or a transport.
- The book lists these tests as covering "TLS/mTLS" (`book/src/deployment/dogfooding-tests.md:211`).
- The documented procedure was never compiled as a test.

### 5. Blast radius and security

**Who is affected:** enterprise and internal-PKI deployments, service meshes with a private CA, and corporate TLS-inspecting proxies whose CA is installed in the OS store, using the JSON-RPC or REST binding over HTTPS. That covers both `send_*` and `resolve_agent_card`.

**Workarounds:** gRPC (`with_grpc_tls_config` works; the prior audit VALIDATED this), a TLS-terminating sidecar with plain `http://` to it, or a hand-written `Transport`.

**Security:**
- Fail-closed. There is no "accept invalid certs" escape hatch, which is good.
- The practical pressure is towards plaintext HTTP inside the network.
- mTLS (client certificates) is equally impossible for JSON-RPC and REST, even though the tests "cover mTLS" at the connector level.

**Severity: Medium.**

### 6. Spec relevance

- A2A §7.2 (`specification.md:1857-1859`): clients "SHOULD verify the A2A Server's identity by validating its TLS certificate against trusted certificate authorities".
- The SDK does verify. It just cannot be told which CAs are trusted. This is not a spec violation; **it is our own rustdoc promise.**

### 7. Fix (patch VALIDATED)

- `ClientBuilder::with_tls_config(rustls::ClientConfig)` stores the config in `ClientConfig.tls_client_config`, a new field; `ClientConfig` is `#[non_exhaustive]`, so the addition is non-breaking.
- `http_transport()` passes it to a new `JsonRpcTransport::with_tls_config` / `RestTransport::with_tls_config`, which rebuild the connector.
- The `tls.rs` doc now names the method.
- Both new setters are `#[cfg(feature = "tls-rustls")]`. `cargo check -p a2a-protocol-client --no-default-features` passes (VALIDATED).
- Discovery and the token provider are **not** changed; the minimal patch covers the documented path only. They need the same parameter.

```diff
diff --git a/crates/a2a-protocol-client/src/builder/mod.rs b/crates/a2a-protocol-client/src/builder/mod.rs
index deac5060..fcb2e85d 100644
--- a/crates/a2a-protocol-client/src/builder/mod.rs
+++ b/crates/a2a-protocol-client/src/builder/mod.rs
@@ -441,6 +441,16 @@ impl ClientBuilder {
         self
     }
 
+    /// Uses `tls` for the JSON-RPC and REST transports' HTTPS connections —
+    /// e.g. [`tls_config_with_extra_roots`](crate::tls::tls_config_with_extra_roots)
+    /// to trust a private CA. (gRPC: see `with_grpc_tls_config`.)
+    #[cfg(feature = "tls-rustls")]
+    #[must_use]
+    pub fn with_tls_config(mut self, tls: rustls::ClientConfig) -> Self {
+        self.config.tls_client_config = Some(tls);
+        self
+    }
+
     /// Disables TLS (plain HTTP only).
     #[must_use]
     pub const fn without_tls(mut self) -> Self {
diff --git a/crates/a2a-protocol-client/src/builder/transport_factory.rs b/crates/a2a-protocol-client/src/builder/transport_factory.rs
index 53e53d59..1f1e4c74 100644
--- a/crates/a2a-protocol-client/src/builder/transport_factory.rs
+++ b/crates/a2a-protocol-client/src/builder/transport_factory.rs
@@ -211,24 +211,36 @@ fn http_transport(
     endpoint: &str,
 ) -> ClientResult<Box<dyn Transport>> {
     match canonical_binding(binding) {
-        Some(BINDING_JSONRPC) => Ok(Box::new(
-            JsonRpcTransport::with_all_timeouts(
+        Some(BINDING_JSONRPC) => {
+            let t = JsonRpcTransport::with_all_timeouts(
                 endpoint,
                 config.request_timeout,
                 config.stream_connect_timeout,
                 config.connection_timeout,
             )?
-            .with_max_response_size(config.max_response_size),
-        )),
-        Some(BINDING_HTTP_JSON) => Ok(Box::new(
-            RestTransport::with_all_timeouts(
+            .with_max_response_size(config.max_response_size);
+            #[cfg(feature = "tls-rustls")]
+            let t = match &config.tls_client_config {
+                Some(tls) => t.with_tls_config(tls.clone(), config.connection_timeout),
+                None => t,
+            };
+            Ok(Box::new(t))
+        }
+        Some(BINDING_HTTP_JSON) => {
+            let t = RestTransport::with_all_timeouts(
                 endpoint,
                 config.request_timeout,
                 config.stream_connect_timeout,
                 config.connection_timeout,
             )?
-            .with_max_response_size(config.max_response_size),
-        )),
+            .with_max_response_size(config.max_response_size);
+            #[cfg(feature = "tls-rustls")]
+            let t = match &config.tls_client_config {
+                Some(tls) => t.with_tls_config(tls.clone(), config.connection_timeout),
+                None => t,
+            };
+            Ok(Box::new(t))
+        }
         Some(BINDING_GRPC) => Err(ClientError::Transport(GRPC_NOT_SYNC.into())),
         _ => Err(ClientError::Transport(format!(
             "unknown protocol binding: {binding}"
diff --git a/crates/a2a-protocol-client/src/config.rs b/crates/a2a-protocol-client/src/config.rs
index db3c08c1..c67a0e2c 100644
--- a/crates/a2a-protocol-client/src/config.rs
+++ b/crates/a2a-protocol-client/src/config.rs
@@ -226,6 +226,12 @@ pub struct ClientConfig {
     /// TLS configuration.
     pub tls: TlsConfig,
 
+    /// A custom rustls configuration for the JSON-RPC and REST transports —
+    /// e.g. [`tls_config_with_extra_roots`](crate::tls::tls_config_with_extra_roots)
+    /// for a private CA. `None` uses the bundled Mozilla roots.
+    #[cfg(feature = "tls-rustls")]
+    pub tls_client_config: Option<rustls::ClientConfig>,
+
     /// Default tenant identifier for multi-tenancy.
     ///
     /// When set, this tenant is included in all requests unless overridden
@@ -252,6 +258,8 @@ impl ClientConfig {
             max_response_size: crate::transport::DEFAULT_MAX_RESPONSE_SIZE,
             max_event_size: crate::streaming::DEFAULT_MAX_EVENT_SIZE,
             tls: TlsConfig::Disabled,
+            #[cfg(feature = "tls-rustls")]
+            tls_client_config: None,
             tenant: None,
         }
     }
@@ -272,6 +280,8 @@ impl Default for ClientConfig {
             max_response_size: crate::transport::DEFAULT_MAX_RESPONSE_SIZE,
             max_event_size: crate::streaming::DEFAULT_MAX_EVENT_SIZE,
             tls: TlsConfig::default(),
+            #[cfg(feature = "tls-rustls")]
+            tls_client_config: None,
             tenant: None,
         }
     }
diff --git a/crates/a2a-protocol-client/src/tls.rs b/crates/a2a-protocol-client/src/tls.rs
index 381c59a2..4ece44cc 100644
--- a/crates/a2a-protocol-client/src/tls.rs
+++ b/crates/a2a-protocol-client/src/tls.rs
@@ -13,7 +13,7 @@
 //!
 //! For enterprise/internal PKI, use [`tls_config_with_extra_roots`] to create
 //! a [`rustls::ClientConfig`] with additional trust anchors, then pass it to
-//! the client builder.
+//! [`ClientBuilder::with_tls_config`](crate::ClientBuilder::with_tls_config).
 
 use std::sync::Arc;
 use std::time::Duration;
diff --git a/crates/a2a-protocol-client/src/transport/jsonrpc.rs b/crates/a2a-protocol-client/src/transport/jsonrpc.rs
index 039ba799..7ab41378 100644
--- a/crates/a2a-protocol-client/src/transport/jsonrpc.rs
+++ b/crates/a2a-protocol-client/src/transport/jsonrpc.rs
@@ -153,11 +153,26 @@ impl JsonRpcTransport {
         })
     }
 
-    /// Sets the maximum size in bytes of a buffered (non-streaming) response
-    /// body. Responses exceeding the cap fail with a non-retryable transport
-    /// error instead of being buffered without bound.
+    /// Replaces the TLS configuration, e.g. with
+    /// [`tls_config_with_extra_roots`](crate::tls::tls_config_with_extra_roots)
+    /// to trust a private CA.
     ///
-    /// Defaults to 32 MiB.
+    /// (`with_max_response_size`: responses exceeding the cap fail with a
+    /// non-retryable transport error; defaults to 32 MiB.)
+    #[cfg(feature = "tls-rustls")]
+    #[must_use]
+    pub fn with_tls_config(
+        mut self,
+        tls: rustls::ClientConfig,
+        connection_timeout: Duration,
+    ) -> Self {
+        Arc::make_mut(&mut self.inner).client =
+            crate::tls::build_https_client_with_connect_timeout(tls, connection_timeout);
+        self
+    }
+
+    /// Sets the maximum size in bytes of a buffered (non-streaming) response
+    /// body; see the note above.
     #[must_use]
     pub fn with_max_response_size(mut self, max_bytes: usize) -> Self {
         Arc::make_mut(&mut self.inner).max_response_size = max_bytes;
diff --git a/crates/a2a-protocol-client/src/transport/rest/mod.rs b/crates/a2a-protocol-client/src/transport/rest/mod.rs
index 1c000526..fdd723b0 100644
--- a/crates/a2a-protocol-client/src/transport/rest/mod.rs
+++ b/crates/a2a-protocol-client/src/transport/rest/mod.rs
@@ -179,11 +179,26 @@ impl RestTransport {
         })
     }
 
-    /// Sets the maximum size in bytes of a buffered (non-streaming) response
-    /// body. Responses exceeding the cap fail with a non-retryable transport
-    /// error instead of being buffered without bound.
+    /// Replaces the TLS configuration, e.g. with
+    /// [`tls_config_with_extra_roots`](crate::tls::tls_config_with_extra_roots)
+    /// to trust a private CA.
     ///
-    /// Defaults to 32 MiB.
+    /// (`with_max_response_size`: responses exceeding the cap fail with a
+    /// non-retryable transport error; defaults to 32 MiB.)
+    #[cfg(feature = "tls-rustls")]
+    #[must_use]
+    pub fn with_tls_config(
+        mut self,
+        tls: rustls::ClientConfig,
+        connection_timeout: Duration,
+    ) -> Self {
+        Arc::make_mut(&mut self.inner).client =
+            crate::tls::build_https_client_with_connect_timeout(tls, connection_timeout);
+        self
+    }
+
+    /// Sets the maximum size in bytes of a buffered (non-streaming) response
+    /// body; see the note above.
     #[must_use]
     pub fn with_max_response_size(mut self, max_bytes: usize) -> Self {
         Arc::make_mut(&mut self.inner).max_response_size = max_bytes;
diff --git a/crates/a2a-protocol-client/tests/deep_private_ca.rs b/crates/a2a-protocol-client/tests/deep_private_ca.rs
new file mode 100644
index 00000000..965d70d6
--- /dev/null
+++ b/crates/a2a-protocol-client/tests/deep_private_ca.rs
@@ -0,0 +1,81 @@
+// deep-claims P1 regression (scratch copy only): the JSON-RPC and REST
+// transports must be able to trust a private CA. Ports 7790-7799.
+#![cfg(feature = "tls-rustls")]
+
+use std::convert::Infallible;
+use std::sync::Arc;
+
+use a2a_protocol_client::ClientBuilder;
+use a2a_protocol_client::tls::tls_config_with_extra_roots;
+use a2a_protocol_server::{JsonRpcDispatcher, RequestHandlerBuilder, RestDispatcher};
+use a2a_protocol_types::error::ErrorCode;
+use a2a_protocol_types::params::TaskQueryParams;
+
+struct Noop;
+a2a_protocol_server::agent_executor!(Noop, |_ctx, _q| async { Ok(()) });
+
+fn port() -> u16 {
+    (7790..7800)
+        .find(|p| std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok())
+        .expect("free port in 7790..7800")
+}
+
+async fn serve_tls(rest: bool) -> (u16, rustls_pki_types::CertificateDer<'static>) {
+    let mut ca_p = rcgen::CertificateParams::new(vec![]).unwrap();
+    ca_p.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
+    let ca_key = rcgen::KeyPair::generate().unwrap();
+    let ca = ca_p.self_signed(&ca_key).unwrap();
+    let issuer = rcgen::Issuer::new(ca_p, ca_key);
+    let leaf_key = rcgen::KeyPair::generate().unwrap();
+    let leaf = rcgen::CertificateParams::new(vec!["localhost".into()]).unwrap().signed_by(&leaf_key, &issuer).unwrap();
+    let cfg = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
+        .with_safe_default_protocol_versions()
+        .unwrap()
+        .with_no_client_auth()
+        .with_single_cert(
+            vec![leaf.der().clone()],
+            rustls_pki_types::PrivateKeyDer::Pkcs8(leaf_key.serialize_der().into()),
+        )
+        .unwrap();
+    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(cfg));
+    let h = Arc::new(RequestHandlerBuilder::new(Noop).build().unwrap());
+    let (j, r) = (Arc::new(JsonRpcDispatcher::new(h.clone())), Arc::new(RestDispatcher::new(h)));
+    let p = port();
+    let l = tokio::net::TcpListener::bind(("127.0.0.1", p)).await.unwrap();
+    tokio::spawn(async move {
+        loop {
+            let (s, _) = l.accept().await.unwrap();
+            let (acc, j, r) = (acceptor.clone(), j.clone(), r.clone());
+            tokio::spawn(async move {
+                let Ok(tls) = acc.accept(s).await else { return };
+                let svc = hyper::service::service_fn(move |req| {
+                    let (j, r) = (j.clone(), r.clone());
+                    async move {
+                        Ok::<_, Infallible>(if rest { r.dispatch(req).await } else { j.dispatch(req).await })
+                    }
+                });
+                let _ = hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
+                    .serve_connection(hyper_util::rt::TokioIo::new(tls), svc)
+                    .await;
+            });
+        }
+    });
+    (p, ca.der().clone())
+}
+
+#[tokio::test]
+async fn jsonrpc_and_rest_clients_trust_a_private_ca() {
+    for (rest, binding) in [(false, "JSONRPC"), (true, "HTTP+JSON")] {
+        let (p, ca) = serve_tls(rest).await;
+        let client = ClientBuilder::new(format!("https://localhost:{p}"))
+            .with_protocol_binding(binding)
+            .with_tls_config(tls_config_with_extra_roots(vec![ca]))
+            .build()
+            .unwrap();
+        // Reaching the server at all proves the TLS handshake succeeded: the
+        // unknown task comes back as the server's TaskNotFound.
+        let err = client.get_task(TaskQueryParams::new("no-such-task")).await.unwrap_err();
+        let a2a: a2a_protocol_types::error::A2aError = err.into();
+        assert_eq!(a2a.code, ErrorCode::TaskNotFound, "{binding}: {a2a:?}");
+    }
+}
```

- Before: does not compile: `error[E0599]: no method named 'with_tls_config' found for struct 'ClientBuilder'`. This is the defect itself, since there is no API.
- After: `jsonrpc_and_rest_clients_trust_a_private_ca ... ok`. Both bindings reach a private-CA server; the unknown task returns the server's `TaskNotFound`, which proves the handshake succeeded.

### 8. Comparison with a2a-rs (SOURCE-CONFIRMED)

a2a-rs does not have this problem:
- `a2a-client-lf-0.2.5/src/jsonrpc.rs:47` `JsonRpcTransport::new(client: reqwest::Client, endpoint)` and `src/rest.rs:44` `RestTransport::new(client, base_url)` take a caller-built `reqwest::Client`, so any TLS setup works, including OS roots, extra roots and client identity.
- `JsonRpcTransportFactory::with_root_certificates_pem(pem)` (`jsonrpc.rs:571-575`) and `RestTransportFactory::with_root_certificates_pem` (`rest.rs:456`) call `default_reqwest_client(Some(pem))` (`lib.rs:43-63`), which uses `tls_certs_merge`.
- Card resolution takes a client too: `agent_card.rs:12` `AgentCardResolver::new(Option<Client>)`.

---

## P2: rate-limit refusals are -32603 / HTTP 500 with no 429 or Retry-After; unauthenticated callers share one bucket

### 1. Reproduction (VALIDATED)

`cargo test --test p2_rate_limit -- --nocapture`, with a limit of 2 per 60 s (`/opt/bench/deep-claims/p2.out`):

```
P2 caller A JSON-RPC #0 -> HTTP 200 retry-after=None code=null msg=null
P2 caller A JSON-RPC #1 -> HTTP 200 retry-after=None code=null msg=null
P2 caller A JSON-RPC #2 -> HTTP 200 retry-after=None code=-32603 msg="rate limit exceeded: 2 requests per 60 seconds"
P2 caller B (first request ever) REST -> HTTP 500 retry-after=None body={"error":{"code":500,"message":"rate limit exceeded: 2 requests per 60 seconds","status":"INTERNAL"}}
P2 SDK client error: Protocol(A2aError { code: InternalError, message: "rate limit exceeded: 2 requests per 60 seconds", ... })
```

- Caller B used a different connection and a different `X-Forwarded-For`, and was refused on its **first** request. This demonstrates the shared `"anonymous"` bucket.
- The SDK client classifies the refusal as `Protocol`, which is not retryable (`client/src/retry.rs:171`). Its retry policy would back off on 429 (`retry.rs:166-167`), but the SDK server never sends one.
- **The prior finding is confirmed in full.**

### 2. Root cause (SOURCE-CONFIRMED)

- `server/src/rate_limit/window.rs:70` and `:157` return `A2aError::internal("rate limit exceeded: …")`. `:132` does the same for bucket-capacity exhaustion.
- Interceptors return `A2aResult` (`interceptor/mod.rs:180-185`), so a refusal becomes `ServerError::Protocol(internal)`.
- `ServerError::http_status` (`error/mod.rs:154-168`) maps that to 500.
- JSON-RPC error responses are HTTP 200 except for auth rejections (`jsonrpc/response.rs:167-171`).
- The codebase **already has** the right concept: `ServerError::Overloaded` goes to REST 503 (`error/mod.rs:165`) and gRPC `RESOURCE_EXHAUSTED` (`dispatch/grpc/helpers.rs:61,85`). An interceptor cannot produce it.
- Identity: `rate_limit/identity.rs:22-46`. The key is `caller_identity()`, else the `X-Forwarded-For` entry when `trusted_proxy_hops > 0` (default 0), else `"anonymous"`.
  - The peer socket address is never available: dispatchers receive a `hyper::Request` without connection info.
  - This is documented in the module docs (`rate_limit/mod.rs:44-71`) and the book (`interceptors.md:328`, `authentication.md:43`). The README ("per-caller") does not mention it.

### 3. History (SOURCE-CONFIRMED)

- `e1f51bdb` 2026-03-16 "feat: perf fixes, rate limiter…" (**v0.3.0**):
  - Returned `A2aError::internal(...)` from the start.
  - Its own module doc claimed "an error with code `-32029`". **The doc and the code disagreed from day one**; the doc was later corrected to "-32603" (`11bdf947`, v0.7.0).
  - It had the `"anonymous"` fallback.
  - It added the README "fixed-window per-caller limiting".
- `113badb4` (v0.10.0) moved the identity code; `545fc34a` (v0.10.0) moved the message.
- **Was the claim ever true?** "Per-caller" has only ever held for authenticated or XFF-trusted callers.

### 4. Why tests missed it (SOURCE-CONFIRMED)

- `rate_limit/tests.rs:704-729` and `shared_tests.rs:67-81` call `limiter.before()` and assert only `is_err()` plus the message text.
- `multi_replica.rs:408+` exercises shared counters at the interceptor level.
- No test drives a dispatcher and asserts the HTTP status or headers of a refusal.
- The -32603 mapping was a documented design choice (`rate_limit/mod.rs:76-81`: "…wrap this in a transport adapter that maps the message… e.g. HTTP 429"). However, the SDK offers no hook for that mapping short of string-matching the message.

### 5. Blast radius, DoS and fairness

**Who is affected:**
- Anyone using `RateLimitInterceptor`.
- Unauthenticated deployments, or deployments where auth is registered after the limiter, or deployments with `trusted_proxy_hops = 0` behind a proxy.

**Effects:**
- (i) Throttling looks like server failure. HTTP 500s trip error-rate alerts and SLOs, and REST clients and proxies cannot distinguish "slow down" from "broken".
- (ii) The SDK's own client neither retries nor backs off.
- (iii) Fairness and DoS: every anonymous caller shares one budget. A single client can spend `requests_per_window` and deny service to *all* other anonymous callers for the rest of the window. Authenticated callers with labelled identities are isolated.
- Bucket-map exhaustion (`max_buckets`) also fails closed with -32603 for *new* callers, the same class of problem.

**Severity: Medium.** It is operational, and trivially exploitable for anonymous-tier DoS. It is not a data-integrity issue.

### 6. Spec relevance

- The current behaviour is **spec-permitted**. A2A §3.3.2 "System Errors" (`specification.md:533-538`) lists "rate limit exceeded" as an example scenario with example codes "HTTP 500 … or 503 …, JSON-RPC -32603", and says servers "MAY include retry guidance (e.g., Retry-After)".
- §13.4 "General Security Best Practices" (`:3176-3180`): agents "SHOULD return appropriate error responses when rate limits are exceeded". That is vague, but arguably met.
- RFC 6585 §4 defines 429 Too Many Requests (Retry-After MAY). It does not mandate 429.
- RFC 9110 §15.6.1 defines 500 as "an unexpected condition", which is semantically inaccurate for throttling.
- **So this is our own promise plus interop quality, not non-conformance.**

### 7. Fix (patch VALIDATED)

This mirrors the existing auth-rejection precedent (N36), where `A2aError` carries a non-wire marker that bindings map to an HTTP status:
- `A2aError::with_retry_after(secs)` / `retry_after()`, stored in a `#[serde(skip)]` field. The struct is `#[non_exhaustive]`.
- The rate limiter sets the marker to `window_secs`. That is an upper bound; the exact time to the window end would be better.
- `ServerError::http_status` gives 429 and `status_name` gives `RESOURCE_EXHAUSTED`.
- REST and JSON-RPC add `Retry-After`, and JSON-RPC uses 429 the way it uses 401/403. The JSON-RPC body code stays -32603, as the spec's example.
- `A2aRouter` inherits the change via `server_error_to_response`.
- **Not in the minimal patch:**
  - gRPC `RESOURCE_EXHAUSTED` for this case.
  - Per-peer-address buckets. Those need the peer `SocketAddr` plumbed from `serve*` into the call context, which is a design change, not a minimal fix.

```diff
diff --git a/crates/a2a-protocol-server/src/dispatch/jsonrpc/response.rs b/crates/a2a-protocol-server/src/dispatch/jsonrpc/response.rs
index 1efa5996..89fa0b05 100644
--- a/crates/a2a-protocol-server/src/dispatch/jsonrpc/response.rs
+++ b/crates/a2a-protocol-server/src/dispatch/jsonrpc/response.rs
@@ -164,7 +164,9 @@ pub(super) fn error_response(
     // A refused credential answers 401/403 over HTTP, the body unchanged
     // (N36): JSON-RPC runs over HTTP, and a client refreshes a token on the
     // status, not on a -32600 it cannot tell from a malformed request.
-    let status = if err.auth_rejection().is_some() {
+    // A throttling refusal likewise answers 429 + Retry-After (RFC 6585 §4),
+    // so HTTP-level clients and proxies can back off.
+    let status = if err.auth_rejection().is_some() || err.retry_after().is_some() {
         err.http_status()
     } else {
         200
@@ -173,6 +175,7 @@ pub(super) fn error_response(
         Ok(body) => {
             let mut resp = json_response(status, body);
             crate::dispatch::add_auth_challenge(resp.headers_mut(), err);
+            crate::dispatch::add_retry_after(resp.headers_mut(), err);
             resp
         }
         Err(e) => internal_serialization_error(id, &e),
diff --git a/crates/a2a-protocol-server/src/dispatch/mod.rs b/crates/a2a-protocol-server/src/dispatch/mod.rs
index 3139c31e..f0f491fa 100644
--- a/crates/a2a-protocol-server/src/dispatch/mod.rs
+++ b/crates/a2a-protocol-server/src/dispatch/mod.rs
@@ -262,6 +262,13 @@ pub(crate) fn validate_version_header(
 /// Adds the `WWW-Authenticate` challenge a `401` for `err` must carry
 /// (RFC 9110 §15.5.2; audit N36). Every HTTP binding answers a refused
 /// credential through this, so none can send a `401` without one.
+/// Adds `Retry-After` (RFC 9110 §10.2.3) to a throttling refusal's response.
+pub(crate) fn add_retry_after(headers: &mut hyper::HeaderMap, err: &crate::error::ServerError) {
+    if let Some(secs) = err.retry_after() {
+        headers.insert(hyper::header::RETRY_AFTER, hyper::header::HeaderValue::from(secs));
+    }
+}
+
 pub(crate) fn add_auth_challenge(headers: &mut hyper::HeaderMap, err: &crate::error::ServerError) {
     let Some(challenge) = err.challenge() else {
         return;
diff --git a/crates/a2a-protocol-server/src/dispatch/rest/error_response.rs b/crates/a2a-protocol-server/src/dispatch/rest/error_response.rs
index 52f346f7..1b29e711 100644
--- a/crates/a2a-protocol-server/src/dispatch/rest/error_response.rs
+++ b/crates/a2a-protocol-server/src/dispatch/rest/error_response.rs
@@ -92,6 +92,7 @@ pub fn server_error_to_response(err: &ServerError) -> hyper::Response<BoxBody<By
         |body| {
             let mut resp = build_json_response(status, body);
             crate::dispatch::add_auth_challenge(resp.headers_mut(), err);
+            crate::dispatch::add_retry_after(resp.headers_mut(), err);
             resp
         },
     )
diff --git a/crates/a2a-protocol-server/src/error/mod.rs b/crates/a2a-protocol-server/src/error/mod.rs
index 29ce0173..b068a146 100644
--- a/crates/a2a-protocol-server/src/error/mod.rs
+++ b/crates/a2a-protocol-server/src/error/mod.rs
@@ -160,6 +160,9 @@ impl ServerError {
                 _ => 403,
             };
         }
+        if self.retry_after().is_some() {
+            return 429;
+        }
         match self {
             Self::PayloadTooLarge(_) => 413,
             Self::Overloaded(_) => 503,
@@ -178,6 +181,16 @@ impl ServerError {
         }
     }
 
+    /// Seconds a throttled caller should wait, when this error is a
+    /// throttling refusal (e.g. from `RateLimitInterceptor`).
+    #[must_use]
+    pub(crate) const fn retry_after(&self) -> Option<u64> {
+        match self {
+            Self::Protocol(e) => e.retry_after(),
+            _ => None,
+        }
+    }
+
     /// The canonical status name (`google.rpc.Code`) the HTTP bindings put in
     /// an AIP-193 error body's `status`, agreeing with
     /// [`http_status`](Self::http_status) and the gRPC dispatcher's code.
@@ -186,6 +199,7 @@ impl ServerError {
         match self.auth_rejection().map(AuthRejection::kind) {
             Some(AuthRejectionKind::Unauthenticated) => "UNAUTHENTICATED",
             Some(_) => "PERMISSION_DENIED",
+            None if self.retry_after().is_some() => "RESOURCE_EXHAUSTED",
             None => self.to_a2a_error().code.grpc_status(),
         }
     }
diff --git a/crates/a2a-protocol-server/src/rate_limit/window.rs b/crates/a2a-protocol-server/src/rate_limit/window.rs
index e8f30464..9f1f0803 100644
--- a/crates/a2a-protocol-server/src/rate_limit/window.rs
+++ b/crates/a2a-protocol-server/src/rate_limit/window.rs
@@ -70,7 +70,8 @@ impl RateLimitInterceptor {
             return Err(A2aError::internal(format!(
                 "rate limit exceeded: {limit} requests per {} seconds",
                 self.config.window_secs
-            )));
+            ))
+            .with_retry_after(self.config.window_secs));
         }
         Ok(())
     }
@@ -157,7 +158,8 @@ impl RateLimitInterceptor {
             return Err(A2aError::internal(format!(
                 "rate limit exceeded: {limit} requests per {} seconds",
                 self.config.window_secs
-            )));
+            ))
+            .with_retry_after(self.config.window_secs));
         }
         Ok(())
     }
diff --git a/crates/a2a-protocol-types/src/error.rs b/crates/a2a-protocol-types/src/error.rs
index 33c31c30..efc3b9ef 100644
--- a/crates/a2a-protocol-types/src/error.rs
+++ b/crates/a2a-protocol-types/src/error.rs
@@ -306,6 +306,11 @@ pub struct A2aError {
     /// binding answers it with its own status instead.
     #[serde(skip)]
     auth_rejection: Option<AuthRejection>,
+    /// Set when this error is a throttling refusal; see
+    /// [`retry_after`](Self::retry_after). Not on the wire: HTTP bindings
+    /// answer it with `429` and a `Retry-After` header instead.
+    #[serde(skip)]
+    retry_after: Option<u64>,
 }
 
 impl A2aError {
@@ -327,6 +332,7 @@ impl A2aError {
             message: message.into(),
             data: None,
             auth_rejection: None,
+            retry_after: None,
         }
     }
 
@@ -338,6 +344,7 @@ impl A2aError {
             message: message.into(),
             data: Some(data),
             auth_rejection: None,
+            retry_after: None,
         }
     }
 
@@ -378,6 +385,21 @@ impl A2aError {
         self.auth_rejection.as_ref()
     }
 
+    /// Marks this error as a throttling refusal the caller may retry after
+    /// `secs` seconds (HTTP `429` + `Retry-After`).
+    #[must_use]
+    pub const fn with_retry_after(mut self, secs: u64) -> Self {
+        self.retry_after = Some(secs);
+        self
+    }
+
+    /// Seconds after which a throttled request may be retried, if this error
+    /// is a throttling refusal. Never read from the wire.
+    #[must_use]
+    pub const fn retry_after(&self) -> Option<u64> {
+        self.retry_after
+    }
+
     /// Creates a "Task not found" error for the given task ID string.
     #[must_use]
     pub fn task_not_found(task_id: impl fmt::Display) -> Self {
```

**Regression test:** `p2_rate_limit_refusal_is_429_with_retry_after`. Before: FAILED `REST: {"error":{"code":500,…"status":"INTERNAL"}}`. After: ok on REST and JSON-RPC.

### 8. Comparison with a2a-rs (SOURCE-CONFIRMED)

- a2a-rs ships **no rate limiter** (`grep -rli "rate.limit|governor|429"` over `/opt/bench/a2a-rs` at `365d056` and over the `-lf` crates finds nothing). It therefore makes no per-caller or 429 claim.
- Its routers are plain `axum::Router`s (`a2a-server-lf-0.4.4/src/jsonrpc.rs:44`, `rest.rs:60`), so a user can add a tower rate-limit layer that answers 429 before A2A handling.
- If a user implemented limiting as an a2a-rs `CallInterceptor` (`middleware.rs:69-80`, returning `A2AError`), they would hit **the same problem**. `A2AError::http_status_code` (`a2a-lf-0.3.1/src/errors.rs:179-199`) has no throttling code and maps `INTERNAL_ERROR` and unknown codes to 500, and `rest_error_response` (`rest.rs:448-450`) uses it.
- Verdict: a2a-rs has the same limitation in its interceptor error model, but does not ship or advertise a limiter.

---

## Other instances of the same bug classes

| # | Class | Location (0.14.0) | Finding | Status |
|---|---|---|---|---|
| O1 | Route missing on `A2aRouter` vs dispatchers | `axum_adapter.rs:152-170` (no tenant routes) and `:472`, `:601`, `:617`, `:633`, `:679`, `:717` (`tenant: None` hard-coded) | `A2aRouter` has no `/tenants/{t}/…` or proto `/{tenant}/…` routes. `POST /tenants/acme/message:send` → **404** on `A2aRouter` vs **200** on `RestDispatcher`, and the same for `/acme/message:send`. URL-path tenancy is impossible on `A2aRouter`. | VALIDATED (`other.out`) |
| O2 | Router vs dispatcher hardening parity | `rest/mod.rs:103-117` vs `axum_adapter.rs` | `RestDispatcher` rejects a query string > `max_query_string_length` with **414**. `A2aRouter` answered **200** for a 9000-byte query. | VALIDATED (`other.out`) |
| O3 | Router vs dispatcher header parity | `axum_adapter.rs:518-523` | `A2aRouter` card has no `access-control-allow-origin` (REST: `*`). Fixed by the F1 patch. | VALIDATED |
| O4 | Pseudo-header readers | `grep` for `":` header names across all 0.14.0 crates | `PathSegmentTenantResolver` is the **only** reader of a pseudo-header, and `websocket.rs:673` the only writer. No other resolver or interceptor reads `:path`, `:authority`, etc. | SOURCE-CONFIRMED (no other instance) |
| O5 | Rustdoc contract vs implementation (client TLS) | `client/src/config.rs:41` and `:43`; `builder/mod.rs:446` | `TlsConfig::Disabled` is documented "Plain HTTP only; HTTPS connections will fail". `TlsConfig::Rustls` is documented "system's default configuration". Neither holds. `config.tls` is **never read** by any transport (`grep` finds only setters), and the roots are compiled-in `webpki-roots`, not the system store. `ClientBuilder::new("https://…").without_tls().build()` returns `Ok` and still dials TLS. | SOURCE-CONFIRMED; `build()` acceptance VALIDATED (`other.out`). The "still dials TLS" part is source-only. |
| O6 | Rustdoc parameter contracts in `signing.rs` | `signing.rs:297-299` `sign_agent_card(pkcs8_key)` | Checked: "PKCS#8 DER" is accurate. An OpenSSL 3 `pkcs8 -topk8` key signs fine (the F4 regression test). Note that `openssl genpkey -outform DER` alone emits SEC1, not PKCS#8, which is a user pitfall but not a doc error. No other mismatch in `signing.rs`. | VALIDATED |
| O7 | Client-caused refusal reported as internal error | `types/src/signing.rs:484` | A bad signature, wrong key or wrong key format → `InternalError` (-32603 class) with no hint. | VALIDATED (F4 output) |
| O8 | Client-caused or throttling refusal → -32603 | `rate_limit/window.rs:132` | Rate-limiter bucket capacity exhausted → `A2aError::internal` → REST 500 / JSON-RPC -32603. | SOURCE-CONFIRMED |
| O9 | Overload → -32603 on JSON-RPC | `error/mod.rs:243`; producers `handler/concurrency.rs:65` (per-tenant in-flight cap), `handler/messaging/admission.rs:173` (stream capacity), `handler/push_config.rs:125` | `ServerError::Overloaded` is REST **503** and gRPC `RESOURCE_EXHAUSTED`, but JSON-RPC **HTTP 200 + -32603** with no `Retry-After` anywhere. This matches the prior audit's #16 observation (`tenant 'A' already has 1 task(s) in flight` → -32603). | SOURCE-CONFIRMED |
| O10 | Hard-coded TLS config beyond the transports | `client/src/discovery.rs:350`, `token_provider.rs:694-696` | Card discovery and OAuth token fetch cannot trust a private CA either. | VALIDATED for discovery (P1 (d)); SOURCE-CONFIRMED for the token provider |
| O11 | Doc citation drift (minor) | `rest/mod.rs:136-140` | This comment says spec §5.4 assigns `ContentTypeNotSupportedError` HTTP **400**. The TCK snapshot here (`specification.md:1188`) says **415**. The comment may be citing a different spec revision; I did not determine which is authoritative. | UNVERIFIED |

---

## Corrections to the prior audit (`claims-audit.md`)

- **#15 / F3, incomplete:** the prior description understates the defect. Details withheld (security; see the F3 section).
- **#35 / F2, imprecise:** the JSON-RPC `-32009` body is an artefact of probes omitting `A2A-Version`. The real defect is that `JsonRpcDispatcher` answers **every** non-card request, on any path and any method, with HTTP 200. With the header set, the body is `-32700`.
- **#25 / P1, incomplete:** discovery (`resolve_agent_card`) and the token provider are also hard-coded, and OS trust-store variables are ignored.
- **#10 / F1, incomplete:**
  - The `A2aRouter` card also lacks the CORS header the dispatchers send.
  - The dynamic handler's own IMS test passes only because both requests fall in the same second.
  - The static handler's IMS comparison is string equality, not a date comparison.
- **F4 and P2:** no errors found in the audit's description.

---

## Appendix: regression tests

The F1, F2 and P2 regression tests are published as `patches/TESTS-public.diff` (the F3 test is withheld with F3). F4 and P1 carry their tests inside `patches/F4.diff` and `patches/P1.diff`.
