<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Triage slice a-l: first-run experience and deployment

Baseline: `main` at `4a8243c66df5ae07a54a69168ab419ac609b06e9`, confirmed with
`git rev-parse HEAD`. Everything here comes from reading the code. Nothing was
built or run: no cargo, docker, kubectl or Prometheus.

Rows: A13, A14, A15, A19, A21, A22, A23, A24, A25, A26, A27, L11 (12 rows).

## Counts by verdict

| Verdict | Count | Rows |
|---|---|---|
| confirmed | 7 | A13, A19, A21, A23, A25, A26, A27 |
| needs_decision | 4 | A14, A22, A24, L11 |
| partly_fixed | 1 | A15 |
| already_fixed | 0 | |
| not_a_defect | 0 | |
| cannot_check | 0 | |
| duplicate | 0 | |

There are 7 new findings: NEW-a-l-1 to -6, with -4 split into 4a and 4b.

## The five most important rows

1. **A13 (confirmed, medium).** I followed the top-level README's commands
   (`README.md:263-267`) one at a time against the repository. They still fail
   twice:
   - The Secret `deploy-agent-db` is never created, so the pods stop at
     `CreateContainerConfigError`.
   - `image: deploy-agent:latest` has no `imagePullPolicy`, so Kubernetes uses
     Always and pulls from Docker Hub. Nothing publishes that image:
     `release.yml` has no container job, and `acts`/`itk-nightly` only `load`
     `itk_service`.

   The deploy-agent README, the book and the manifest header gained warnings
   in 57f034e, but the front page did not change after 2026-10-23. The fix is
   XS.
2. **L11 (needs_decision, low).** All four panels on the client dashboard, and
   its three recording rules, take `rate()` per series. Each CLI process has a
   random `service.instance.id` (`resource.rs:42-47`) and exports once at exit,
   because the export interval is 60 s and a streaming call is recorded when
   its stream is dropped.
   - What a short-lived CLI shows: its job appears in the `$job` dropdown, and
     every panel is empty.
   - What a long-lived client shows: data appears on every panel after about
     2 minutes.

   The docs carry the caveat only under the GenAI dashboard, and the
   walkthrough calls these series "thin", which understates it. Setting a
   stable instance id is not a fix by itself: each run restarts its counter,
   so a series reading 1,1,1 has a rate of 0. My recommendation is a docs fix
   plus a `last_over_time` panel.
3. **A14 (needs_decision; severity lowered from medium to low).** Still true:
   the variable is the generic `DATABASE_URL`, and the tables (`tasks`,
   `push_configs`) are generic, with no option to choose a schema. Two things
   make it less severe than reported:
   - Startup now logs the redacted database URL.
   - A clash with an existing `tasks` table fails closed, because `pg_schema`
     runs everything in one transaction.

   The obvious mitigation, a dedicated schema through `search_path`, is not
   safe yet because of NEW-a-l-2.
4. **A23 (confirmed, low).** The book says to "copy the two files"
   (`containers.md:22`). A copied Dockerfile then needs edits the book does not
   list:
   - Four references to `-p deploy-agent` and the binary path.
   - A committed `Cargo.lock`, because the build uses `--locked`.
   - A `.dockerignore`, which the copy does not bring. Without it the A12 leak
     comes back.

   A copied crate fails on workspace inheritance and on its path dependency.
   On `main` it also uses `with_drain_delay` and `new_with_pool_size`, which no
   published SDK has yet (they were added after 0.15.1, and there is no
   v0.16.0 tag).
5. **A26 (confirmed by reading; whether it causes OOM is still a conjecture).**
   - Tokio follows `available_parallelism()`, which is the node's CPU count
     when no CPU limit is set.
   - jemalloc 5.3.1's `malloc_ncpus()` uses `sched_getaffinity`/`sysconf` and
     never reads cgroup quotas. So the audit's suggested fix "or a CPU limit"
     does not bound jemalloc's arenas. Only `_RJEM_MALLOC_CONF` does.

   Measuring it needs a live probe on a host with many cores.

The other rows:
- **A15 (partly_fixed).**
  - Fixed: `DATABASE_POOL_SIZE` and `new_with_pool_size` (a579fee). I read
    tests that would fail if this were reverted.
  - Still open: readiness checks only the task store's pool, and
    `PushConfigStore` has no health method.
- **A19 (confirmed).** All four details still hold.
- **A21 (confirmed).** After discovery, a connection error names neither host,
  because hyper-util's `ConnectError` prints only "dns error" or "tcp connect
  error". Only the `--header` case is now refused with the card's URL.
- **A22 (needs_decision).** The table of names is in the JSON. The book's
  environment-variable table also omits `SSL_CERT_FILE`/`SSL_CERT_DIR` and the
  wider scope of the proxy variables.
- **A24 (needs_decision).** Publishing a deploy-agent image would also fix A13
  at the root.
- **A25 (confirmed).** deploy-agent has no `otel-prometheus`. Setting
  `OTEL_METRICS_EXPORTER=prometheus` exits with code 2 and a clear message.
- **A27 (confirmed).** The version on the card comes from `version = "0.0.0"`.

## New findings worth acting on

- **NEW-a-l-1:** deploy-agent never calls `purge_expired`. Setting
  `DATABASE_URL` silently changes retention from 10k tasks for 1 h to forever.
  This was raised under A2, which was closed for its readiness half only.
- **NEW-a-l-2:** the schema guards disagree about which schema they look in.
  `to_regclass('tasks')` searches the whole search_path, while the
  `applied_seq` check and the function guard use `current_schema()`. With
  `search_path=a2a,public` and an unrelated `public.tasks`, the store attaches
  itself to the other application's table.
- **NEW-a-l-3:** every Secret recipe includes `sslrootcert=/path/ca.pem`, but
  the manifest has no volume to mount a CA file, and its root filesystem is
  read-only. Without `sslrootcert`, the driver falls back to the Mozilla
  roots, which do not include RDS's or Cloud SQL's server CAs.
- **NEW-a-l-4a/4b:**
  - No CI job builds the deploy-agent Dockerfile or lints the manifest. L18
    says the build stages have never run.
  - Dependabot has no `docker` ecosystem, so the base images pinned by digest
    never receive base-image fixes.
- **NEW-a-l-5:** every example that reads `A2A_BIND_ADDR` advertises
  `http://{bind addr}` on its card. With `0.0.0.0` that is the trap
  deploy-agent exists to prevent, and the ITK image sets exactly
  `0.0.0.0:9104`.
- **NEW-a-l-6:** `README.md:271` still says deploy-agent binds `0.0.0.0`; it
  now binds `[::]` and falls back to `0.0.0.0`.

## Not done, and why

- **Live probes:** none were run, because the brief forbids cargo and docker
  and this sandbox has no cluster. Each row that needs one has the exact steps
  in its `probe` field: A13 (kind), A14 (PostgreSQL with a foreign `tasks`
  table), A21, A23, A26 (a host with many cores and a 256m memory limit), and
  L11 (the observability stack with 50 CLI runs).
- **Tests:** I read the A15 tests but did not run them. The live-Postgres
  `the_pool_is_the_size_asked_for` should run in CI via
  `scripts/run_postgres_suites.py`; I did not run that script's `--plan`.
- **Docker Hub:** I did not check over the network that
  `docker.io/library/deploy-agent` does not exist. The manifest's own comment
  says it does not.
- **sqlx behaviour (NEW-a-l-3):** I did not read the sqlx source for what
  happens when the `sslrootcert` file is missing.
