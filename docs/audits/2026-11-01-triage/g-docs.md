# Triage: g-docs (G/D-01 .. G/D-11, G/C-01 .. G/C-03)

Baseline: `main` at `4a8243c66df5ae07a54a69168ab419ac609b06e9` (confirmed with `git rev-parse HEAD`).
Shallow clone: history goes back to 2026-09-18. Everything here comes from reading the files and running read-only `rg`/`git`/`python3 -I` scripts. Nothing was compiled or run. No `cargo` was run.

## Counts by verdict (14 rows)

| Verdict | Rows |
|---|---|
| confirmed | 10: D-02, D-03, D-04, D-06, D-07, D-08, D-09, D-10, D-11, C-03 |
| partly_fixed | 1: D-05 |
| already_fixed | 1: D-01 (57f034e; test `tests/private_ca.rs:113`) |
| needs_decision | 2: C-01, C-02 |

The README's `Status` column shows all 14 as `open`. D-01 has been fixed since 57f034e (2026-10-29), which landed before the README's last edit. D-05 is half fixed by the same commit.

## The five rows that matter most

1. **G/C-02 (medium, needs_decision).** No gate stops a breaking *behaviour* change from shipping in a patch release.
   - `cargo-semver-checks` (`ci.yml:986-1000`) only sees the API.
   - The release gate, `check_release_tree.py:185,203`, fails a patch only when its section contains the literal heading `### Breaking Changes`. 0.15.1 filed an `.init()`-now-panics change and a proxy reroute under `### Behaviour Changes`, so it passed.
   - `STABILITY.md` §2 does not classify a change like that, and §1 still says 0.15.0 is the newest release.
   - Options and a recommendation are in the JSON.
2. **G/D-02 (medium).** The server crate README (its crates.io page, `:61-63`, `:128`) and the **root** `README.md:63`, which the report did not cite, still say `serve` uses "hyper's defaults ... no idle timeout" and has "no limits".
   - In the code, `serve` runs `Server::bind`, which uses `ServeConfig::default()`: a 30 s header-read timeout, a 75 s idle timeout and the loopback Host check (`serve/mod.rs:182-193`, `graceful/config.rs:168-183`).
3. **G/D-05 (partly_fixed).**
   - Fixed: the README's exit-code table and `--header` row are now correct (57f034e).
   - Still open:
     - The CLI's own rustdoc exit table (`main.rs:29`) leaves out the `OTEL_*` exit-2 path, which runs *before* clap.
     - The header rule is case-sensitive, and the JSON-RPC transport appends headers, so names that differ only in case are both sent.
     - **`a2a card <URL>` ignores `--header` and `--timeout` entirely** (`main.rs:61`, `commands.rs:48-55`). That is the "`--header` on `card`" item the handoff names.
4. **G/D-07 (low, wider than reported).** It is not only the Axum page. Six hidden cards on four dispatcher pages advertise only JSON-RPC while the page serves another binding:
   - `axum.md:23,49`
   - `rest.md:15`
   - `grpc.md:21,78`
   - `websocket.md:71`

   A `RestDispatcher` that serves under `ServeConfig::production()` refuses a card that does not declare its binding.
5. **G/C-03 (low).** The book's ignore ratchet (`scripts/check_book_code.sh`) only matches fences at column 0.
   - `book/src/deployment/audit.md:361`, an indented `rust,ignore` block added 2026-10-29, is compiled by nothing and is missing from `.book-ignore-baseline`.
   - The slimrpc README's 5 `rust,no_run` blocks are also compiled by nothing (the report missed this).
   - So are 14 rustdoc `ignore` blocks, among them the main `EventEmitter` example.

## Other confirmed rows (all XS doc edits)

| Row | What is wrong | Where |
|---|---|---|
| D-03 | Rustdoc predicts fields that already exist | `graceful/config.rs:260-265` |
| D-04 | Says no signing key is committed; one is, and 0.15.0 and 0.15.1 passed the gate | `packaging/README.md:20-22,26` |
| D-06 | gRPC returns `io::Result<ServeReport>`, not `ServeReport` | `graceful-shutdown.md:94-96` |
| D-08 | Quick start step 3 says "wire into your hyper server"; it also says "ten `on_*`" methods, and there are eleven | `server/src/lib.rs:15`, `:19` |
| D-09 | Rustdoc says "transport errors" are retryable, but `Transport` is not; the variant's own doc still says "connection error" | `retry.rs:151`, `error/mod.rs:33` |
| D-10 | "One screen, no feature flags", but the example is 106 lines above its tests (52 of them code) and depends on the default `otlp` feature | hello-agent |
| D-11 | The rustdoc list leaves out 22 of the prelude's 63 exports; the SDK README leaves out 40 | `sdk/src/lib.rs:67-79`, SDK `README.md:54-58` |

The exact old text and replacement text for each are in `g-docs.json` under `remaining`.

## New findings (8, all read only)

- **NEW-1:** `without_tls()` "plain HTTP only" is not true for `build_grpc()`, which never reads `config.tls`.
- **NEW-2:** `a2a card` drops `--header`, `--timeout` and `--tenant`.
- **NEW-3:** `tck/README.md:122-125` says BIND-EQUIV-004 is graded "structurally only", but CI runs the enforcement half against a secured SUT (`tck.yml:197-218`).
- **NEW-4:**
  - `STABILITY.md` §1 is stale: it says 0.15.0 is the newest release, but 0.15.1 shipped and the crates are now at 0.16.0.
  - `sign_agent_card` is documented as "removed in 0.16.0" but still exists in the 0.16.0 tree.
- **NEW-5:** The book ignore ratchet does not see indented fences.
- **NEW-6:** The CLI README says TLS verification uses "its bundled roots". `SSL_CERT_FILE`/`SSL_CERT_DIR` are honoured on the HTTP and WebSocket bindings, but not on gRPC.
- **NEW-7:** When a release section exceeds 120,000 characters, `release.yml:589-595` drops `### Behaviour Changes` from the GitHub release notes.
- **NEW-8 (info):** README and book snippets on main say `"0.16"`, which is not yet on crates.io.

## Not checked, and why

- **Live probes:** none were needed. Every row can be settled by reading. The D-05 `a2a card` behaviour is clear from the code; a request-logging proxy would show it live, but I did not run one.
- **Tests:** I did not run any. Where I name a test (D-01's `without_tls_refuses_an_https_endpoint`, D-09's `timeout_is_retryable_transport_is_not`), I read its assertions and judged whether it would fail if the code were reverted.
- **Book build:** I did not build the book or run `scripts/check_book_code.sh`. The ratchet gap comes from reading the script's regexes.
- **README "Status" column:** not updated, because this triage is read-only.
