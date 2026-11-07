<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0040: First-run configuration: one table of names, a database variable of the SDK's own, a schema of its own, and the image as the ready-to-run server

**Date:** 2026-11-04
**Status:** Accepted for 0.16.0 (the maintainer's instruction of 2026-11-04:
every open plan item is in 0.16.0, at the best engineering option, with a veto
in review). Built in the same change as this record.
**Author:** Tom F.

---

## Context

Three audit rows of plan group G were left as "needs decision" by the triage of
2026-11-01 (`docs/audits/2026-11-01-triage/DECISIONS.md`, rows A14, A22, A24):

* **A14.** `deploy-agent` reads the generic `DATABASE_URL`, the variable
  sqlx-cli, Diesel, Rails and Django users already export, and the PostgreSQL
  stores create generic table names (`tasks`, `push_configs`,
  `idempotency_keys`, `task_events`, `schema_versions`) in whatever schema the
  connection's `search_path` names, with no option and no documentation.
  Tables added since 0.12 are prefixed (`a2a_task_claims`, `a2a_delegations`),
  so the naming is not even consistent. A clash with another application's
  `tasks` fails closed at start (one rolled-back transaction), and the start
  logs the redacted URL, so the damage is a refused start, not silent
  corruption. The obvious mitigation, a dedicated schema through
  `search_path`, did not work: the schema guards disagreed about which schema
  they looked in (audit NEW-a-l-2), so with `search_path=a2a,public` and an
  unrelated `public.tasks` the store failed at start ("foreign key constraint
  `task_events_task_id_fkey` cannot be implemented", reproduced 2026-11-04 by
  `pg_schema::search_path_tests` before the fix).
* **A22.** The binaries spell the same setting differently: four ways to say
  where to listen (`PORT`, `A2A_BIND_ADDR`, `SUT_HOST`, hard-coded), two for
  the extra listeners, two meanings of `*AGENT_URL` (deploy-agent's own public
  URL; mcp-bridge's remote agent), several model and database variables, and
  no table that lists them. The book's environment table missed
  `SSL_CERT_FILE`/`SSL_CERT_DIR` and the proxy variables' wider scope.
* **A24.** Every deployment means writing and building a Rust crate: there is
  no env-configured server binary.

Since the triage, fork F8 was decided ("signed image to GHCR from
`release.yml`") and built (`636eaf08`): from 0.16.0 the release workflow
publishes `ghcr.io/tomtom215/a2a-rust/deploy-agent:<version>` with a Sigstore
build-provenance attestation, and the manifest names it.

## Decision

### 1. The database variable (A14): `A2A_DATABASE_URL`, then `DATABASE_URL` with a warning

`deploy-agent` reads `A2A_DATABASE_URL`. When that is unset it reads
`DATABASE_URL`, as before, and logs at `WARN` that it did, naming both
variables, so a developer whose shell exports `DATABASE_URL` for another
application sees at once which database the agent chose and how to point it
elsewhere. The manifest sets `A2A_DATABASE_URL` from the same Secret.
`DATABASE_URL` stays accepted because platforms inject it (Heroku, Render,
Railway, Fly's attached PostgreSQL), and a deployment there should work without
a rename; a warning, not a refusal, is the price of that.

### 2. A schema of its own (A14, NEW-a-l-2): `search_path`, made correct

The table and index guards in `store::pg_schema` now ask whether the relation
exists in `current_schema()` (`to_regclass(format('%I.%I', current_schema(),
name))`), as the column and function guards already did. `current_schema()` is
the first schema on the path that exists, which is where an unqualified
`CREATE TABLE` puts the table and where every unqualified statement of the
stores then finds it first. With that, an operator puts the A2A tables in a
schema of their own with nothing but the connection URL:

    CREATE SCHEMA a2a AUTHORIZATION agent;
    postgres://agent:...@db/app?options=-c%20search_path%3Da2a

and `search_path=a2a,public` works too, beside another application's
`public.tasks`. Two live tests pin both (`pg_schema::search_path_tests`); the
book's horizontal-scaling chapter carries the recipe.

### 3. Names (A22): document every variable in one table, rename nothing, and add no new spelling

One table in `book/src/reference/configuration.md` lists every variable the
SDK reads and every variable each shipped binary reads, with what it means. No
existing variable is renamed in 0.16.0: a rename breaks every script, manifest
and CI job that sets the old name (the ITK image, `go_sdk_interop.sh`,
`scripts/adversarial`, the TCK workflow set `A2A_BIND_ADDR`) for no change in
behaviour, and aliases that log deprecations double the surface to test. The
rule for anything new:

* a setting a platform injects is read under the platform's name (`PORT`;
  `DATABASE_URL` as the fallback above);
* everything else that is the SDK's or an example's own carries the `A2A_`
  prefix, as `A2A_BIND_ADDR`, `A2A_PROXY_LOOPBACK` and `A2A_DATABASE_URL` do;
* a variable that names a URL says whose URL it is: the examples that serve
  on `A2A_BIND_ADDR` gain `A2A_PUBLIC_URL`, the URL their card advertises
  (audit NEW-a-l-5), which is deploy-agent's `AGENT_URL` under the examples'
  prefix. The table says so on both rows.

### 4. The ready-to-run server (A24): the published image, and no gateway binary

The image the release workflow publishes is the ready-to-run server: pull it,
set `AGENT_URL` and the database, apply the manifest. It runs a trivial agent
on purpose; it is the deployment wrapper (probes, drain, retention, telemetry,
the bind and the card) that is ready to run, and an agent of one's own is
built from the same Dockerfile (section 5). The README front page says this
next to the deploy commands.

### 5. Copying it out (A23, A19): the Dockerfile takes the package as an argument, and carries its own ignore file

The Dockerfile reads the package and binary name from build arguments
(`PACKAGE`, default `deploy-agent`), so a project that copies it builds its own
crate without editing the file. Its ignore rules move into
`examples/deploy-agent/Dockerfile.dockerignore`, which BuildKit reads in place
of the root `.dockerignore` for this Dockerfile and which therefore travels
with it when copied (until now a copied Dockerfile lost the A12 protection
against `.env`, keys and `.git` in the build context). The same file also
leaves out what no build of this image reads (the book, the docs, the audit
evidence, the other languages' agents), so editing them no longer invalidates
the last build stage (A19). The book lists the remaining edits a copy needs.

## Alternatives not taken

* **A `with_schema(name)` option on every PostgreSQL store** (A14 option 2).
  Every SQL constant of four stores, the migration runner and the upgrade
  guard functions would become schema-qualified, a large test surface for what
  `search_path` now does correctly. Revisit if someone needs two A2A
  deployments with different schemas over one connection pool.
* **Renaming the tables to `a2a_*`** (A14 option 3). Removes the clash for
  good, but is a storage break for every existing deployment and for the
  rolling-upgrade guards that keep an older replica off a newer one's tasks
  (`store::claim::upgrade` installs its triggers on `tasks` and `task_events`
  by name, and a 0.15 replica writes those names). Not worth a migration
  when a schema of its own now works.
* **Converging every example on one set of names with deprecated aliases**
  (A22 option 2). See section 3.
* **An `a2a-gateway` binary**: an OpenAI-compatible model behind
  `ToolLoopExecutor`, configured by environment, published as an image (A24
  option 3). A new supported surface with its own security review (a model
  key in a public image's environment, a tool policy), which no user has asked
  for; the tool loop's own example already shows the shape.

## Consequences

* `deploy-agent` started with only `DATABASE_URL` logs one `WARN` line it did
  not log before. Nothing else about it changes for existing users.
* Stores started with a `search_path` whose first schema already held an A2A
  install elsewhere on the path now create their own tables in the first
  schema, where before they reused (or failed on) the later schema's. A
  deployment that relied on that reuse, by putting a new empty schema first on
  an existing install's path, gets a fresh empty store; nothing in the
  repository or its documentation set up such a path.
* `A2A_PUBLIC_URL` is a new variable of five examples; their cards stop
  advertising an unspecified bind address (`0.0.0.0`, `[::]`).
