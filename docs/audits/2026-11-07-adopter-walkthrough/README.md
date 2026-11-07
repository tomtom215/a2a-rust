<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Adopter walk-through of 0.16.0, 2026-11-07

A stranger's first hours with the release before it is tagged: package the four crates, build a
project outside the repository from the tarballs, type the README and the book as written, run
the examples and the CLI, and talk to a real model. This directory is the log; the scripts that
produced it are in `scripts/live/adopter/` (each says what it needs; none is a CI gate). Raw output
of every run is in `evidence/`. Worktree branch `worktree-agent-aefdbbc4b8f3742b2`, off `main` at `27889e09`.

| File | Area |
|---|---|
| [01-packaging.md](01-packaging.md) | `cargo package`, the unpacked tarballs, what is in them and what is missing |
| [02-readme-and-book.md](02-readme-and-book.md) | the README Quick Start and the book's Getting started, typed as written; every Rust block compiled at default features |
| [03-examples.md](03-examples.md) | the examples, run as their READMEs say |
| [04-cli.md](04-cli.md) | the `a2a` CLI against an agent and against a misbehaving peer |
| [05-live-model.md](05-live-model.md) | llama.cpp b12189 with Qwen3.5-2B and Gemma 4 E2B: the examples, a refusal, a truncated answer, scripted malformed replies, a client that disconnects |
| [06-deployment.md](06-deployment.md) | the deploy-agent process, image digests, compose and Kubernetes files, the observability stack |
| [07-docs-hygiene.md](07-docs-hygiene.md) | the documentation gates, dead links, stale numbers |

## Result

No blocker and no high-severity defect was found in what was run. The README Quick Start, the
Installation, Quick Start and Your First Agent pages, `hello-agent`, `deploy-agent`, `echo-agent`
and the CLI all did what their pages say, from tarballs built outside the repository, on stable
1.99.1 and on rustc 1.94.0 with no lockfile; the observability stack came up, received traces,
metrics and logs and filled its dashboards. Found: one real packaging defect (P-1), one behaviour
that needs a decision (M-1, a truncated or empty model answer is a `COMPLETED` task), and eighteen
documentation, CLI and example rough edges (D-1 to D-18). P-1 and nine of the others are fixed in this
branch, each with a check that fails without the fix; the rest are open, listed below.

### Findings, ranked

| ID | Severity | Where | What a stranger meets | Found by | State |
|---|---|---|---|---|---|
| M-1 | medium | `crates/a2a-protocol-server/src/tool_loop/run.rs:360` | A model answer cut at the token limit completes the task with the cut text; a reasoning model that spends its tokens thinking completes it with an **empty answer**. Real models: Qwen3.5-2B (answer ends mid-sentence), Gemma 4 E2B (`TOOL_LOOP_THINKING=1`, 40 tokens: empty). No marker anywhere; `finish_reason` is kept for telemetry only | `05-model-runs.sh`, `05c-model-stub-runs.sh` | **needs a decision** (not touched: `tool_loop/` is another branch's) |
| P-1 | medium | the four tarballs | No `LICENSE` or `NOTICE` in any `.crate` (the manifests say `Apache-2.0`) | `tarball_scan.py` | **fixed** (`8b5f1c63`) |
| D-5 | low | nine book pages | A block uses `PostgresTaskStore`, `WebSocketDispatcher`, `tool_loop` or `auth::jwt` and the page never names the Cargo feature; default dependencies give `unresolved import` | `03-book-sweep.sh default` | **fixed** (`5fbb3df5`) |
| D-15 | low | tool-loop, SQLite, PostgreSQL pages | The tool-loop page's minimal agent does not build from the page's dependency line (needs `tokio`, `serde_json`) | `05-tool-loop-page.sh` | **fixed** (`fa50a3c5`) |
| D-3 | low | installation.md, README, crate-map diagram | `tls-rustls` said to be a default of client and SDK only; the server's default holds it since 0.16.0 | reading against `Cargo.toml` | **fixed** (`025cbd0b`) |
| D-1 | low | `README.md:23` | The conformance badge says 83/114, 5 failing; the paragraph below it, 85 of 115, 2 failing | reading | **fixed** (`63ea45ae`) |
| D-2 | low | introduction, README, server and SDK READMEs, API reference | `serve()` offered as the way to start a listener; deprecated since 0.16.0 | reading | **fixed** (`63ea45ae`) |
| D-8 | low | CLI README, book CLI page | Exit-code table says one `error:` line then a JSON line; the SDK's own `WARN`/`ERROR` log line for the same failure comes first | `04-cli.sh` | **fixed (docs)** (`4fabb287`); the duplicate line is a decision |
| D-14 | low | hello-agent README and book page | Quotes `invalid params: message must contain at least one part`; the wire says `-32602` and `message must contain at least one part` | `03-hello-claims.sh` | **fixed** (`4fabb287`) |
| D-9 | low | CLI | A URL without a scheme is exit 1, though the table puts "a value that does not parse" under 2 | `04-cli.sh` | open (message is clear) |
| D-10 | low | CLI | A 401 or 429 prints the status and body only: no `WWW-Authenticate`, no `Retry-After`, no "pass `--header-env`" | `04-cli.sh` | open |
| D-11 | low | CLI | A message is one argument, so 128 KiB at most; `a2a send URL -` sends the text `-` | `04b-cli-followup.sh` | open (decision) |
| D-12 | low | CLI | No flag bounds a stream that has started; a silent one holds the command for the library's 300 s | `04b-cli-followup.sh` | open (decision) |
| D-4 | polish | first-agent.md | Typing Steps 1 to 3 as written prints two `unused import` warnings | `02-book-getting-started.sh` | **fixed** (`4fabb287`) |
| D-7 | polish | README "Command line" | `a2a card http://127.0.0.1:3111` with nothing said about what listens there | reading | **fixed** (`4fabb287`) |
| D-6 | polish | server and SDK crate READMEs | Hidden `# ` lines show literally on crates.io; the SDK README's Quick Start has no dependency line and no `fn main` | reading | open |
| D-13 | polish | CLI | After a stream's idle timeout the CLI adds the "the card named another URL" hint, which is wrong for a connected stream | `04b-cli-followup.sh` | open |
| D-16 | polish | `docs/implementation/plan.md` (11 anchors), `spec/slimrpc_v1` (1), the vendored spec copy (10) | Dead anchors and links in design records | `07-links.py` | open |
| D-17 | polish | REST dispatcher | `OPTIONS` with no CORS configured is `200 {"status":"ok"}`; the JSON-RPC dispatcher answers `204` | `03-card-http-claims.sh` | open |
| D-18 | polish | `genai-agent` | Logs three `WARN`s about its own card at startup (a gRPC `host:port` that is "not an absolute URL", a custom binding that "is not a URI") | `05-model-runs.sh genai` | open |

### Needs a decision

1. **M-1**, a truncated or empty model answer. (a) Fail the task with a failure class (`truncated`) when a
   final reply has `finish_reason: "length"` or no text, or (b) complete it but put `finish_reason` in
   the answer artifact's metadata and the status message so callers can see it. Recommend (b) for a partial
   answer and (a) for an empty one: an empty `COMPLETED` task is never what a caller wants, and a partial answer
   is sometimes all that is wanted but must be visible. It is a change to `tool_loop/` and `LoopError`, so
   it belongs with the work in that directory.
2. **`ListTasks` with a malformed page token** answers `{"tasks":[],"nextPageToken":"","pageSize":0,"totalSize":0}`
   with exit 0 where the store holds tasks (an in-memory store comment says the contract is "unknown cursor →
   empty"). (a) Keep; (b) answer `-32602` (`INVALID_ARGUMENT`) for a token the store cannot decode. Recommend
   (b): a client paging with a corrupted or expired cursor reads "no more tasks" as "complete".
3. **The library's log line for an error it returns** (D-8). (a) Leave the CLI's subscriber as is and keep
   the docs fixed in this branch; (b) filter the client's transport targets in the CLI so exit 1 prints
   `error:` and the JSON line only. Recommend (a): the same line is what a service operator wants and the
   CLI's `OTEL_*` handling already says the subscriber prints warnings.
4. **CLI: a message from stdin and a stream idle bound** (D-11, D-12). (a) Leave and document;
   (b) add `a2a send URL -` (read stdin) and `--stream-idle-timeout`. Recommend (b), small, and not for 0.16.0.

### What this branch changes

Eight commits on top of `main` (`git log 27889e09..HEAD`): the licence files and the check that holds
them (`check_package_excludes.py`); the TCK-badge, deprecated-entry and dependency-block checks and their
`--self-test` and 14 hand mutants (`check_doc_versions.py`); the default-feature statements and the
page-names-its-feature check (`check_feature_tables.py`); the nine pages, the three dependency blocks and four
first-hour rough edges; this directory and `scripts/live/adopter/`. No Rust source changed, so there is
nothing for `cargo mutants --in-diff` to mutate: `git diff 27889e09 -- '*.rs'` is empty. Red-then-green
evidence for each fix is in `evidence/fix-*.log`.

## Not verified

* The `docker build` of the deploy-agent image, running it and its HEALTHCHECK, the Kubernetes manifests
  against any schema (`kubectl`, `kubeconform`, `kind` and `promtool` are not installed), a pod running at all.
* `genai-agent` and `rig-agent` end to end with both models, `live-model` with either: the box ran at 0.3 to 1
  tokens/s; see `05-live-model.md` for what finished.
* `incident-response`, `agent-team`, `resilient-agent`, `swarm`, `mcp-agent`, `mcp-bridge`, `multi-lang-team`
  and `langfuse-agent`: neither built nor run (disk and CPU; CI runs seven of them).
* The sweep of every book block with every feature on (the disk could not hold it; `book-tests/` does it in
  CI), and the dependency lines of pages that do name a feature.
* Client, server, binding, authentication and swarm pages beyond compiling their Rust blocks and
  statically checking their commands; PostgreSQL-backed behaviour; TLS (`--cacert`, mTLS); gRPC and WebSocket
  agents driven by the CLI.
* `scripts/prove_workflow_gates_fail.py` (needs `ssh-keygen`, not installed) and `scripts/prove_gates_fail.sh`
  (refuses a dirty tree, and runs for about 15 minutes).
