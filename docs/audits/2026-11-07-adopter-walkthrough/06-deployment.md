<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# 6. Deployment artifacts

Scripts: `03-examples-offline.sh` (the agent), `03-card-http-claims.sh`, `06-image-digests.sh`,
`06-manifest-lint.py`. Raw output: `evidence/06-*`, `evidence/03-examples-offline.log`,
`evidence/03-card-http-claims.log`.

## What ran

| Artifact | What was run | Result |
|---|---|---|
| deploy-agent, as a process | `cargo run -p deploy-agent`-equivalent (the built binary): `/health`, `/ready`, the card, `POST /message:send`, `POST /` (JSON-RPC), `SIGTERM` with a 3 s drain, six invalid configurations | **pass** (`03-examples.md`): `/health` and `/ready` 200; during the drain `/ready` 503 and `/health` 200, exit 0, `drained cleanly, exiting`; every bad value refused at startup with exit 1. `GET /metrics` and `/healthz` answer **400** (an unknown path is a 400 on the REST dispatcher, not a 404): this agent has no scrape endpoint, and the deploy-agent README says why (built without `otel-prometheus`; metrics leave through OTLP and a Collector). The health paths are `/health` and `/ready`, not `/healthz` and `/readyz` |
| The agent card endpoint | conditional GETs, HEAD, OPTIONS, a wrong method, a trailing slash, `Accept: text/html`, the extended card without credentials (`03-card-http-claims.sh`) | **pass for the caching claim**: `ETag` (weak), `Last-Modified`, `Cache-Control: public, max-age=3600`; `If-None-Match` with the ETag or `*` is 304 with no body, another tag 200, `If-Modified-Since` at `Last-Modified` 304 and at 1970 200; `HEAD` carries the headers; `POST` is 405 with `Allow: GET, HEAD`; the card carries `Access-Control-Allow-Origin: *` |
| The pinned images | `06-image-digests.sh`: `docker buildx imagetools inspect` of each digest | **pass**: all 8 pins (`rust:1.94-bookworm`, `debian:bookworm-slim`, Collector, Prometheus, Alertmanager, Tempo, Loki, Grafana) still resolve; Docker Hub did not answer 429 |
| The compose files | `docker compose -f deploy/observability/docker-compose.yml config -q`, and with the prod overlay | **pass**: both parse; the overlay refuses to start without `GRAFANA_ADMIN_PASSWORD`, as the README says |
| Kubernetes manifests | `06-manifest-lint.py` (YAML parse and cross-checks against the Dockerfile and the agent) | **parsed and cross-checked only**: five documents, container port 8080 equals the Dockerfile's `PORT`, probe paths are `/health` and `/ready`, the Service selector and the NetworkPolicy's `podSelector` match the pod's label `app: deploy-agent`, the Ingress targets that Service. **`kubectl`, `kubeconform`, `kind` and `promtool` are not installed, so no manifest was validated against a Kubernetes schema and no Prometheus rule was checked by promtool: nothing here claims either.** The sandbox cannot start a pod (handoff: nested `runc`) |
| `docker build -f examples/deploy-agent/Dockerfile` | not run to completion | see "Not run" |
| The observability stack | `docker compose -f deploy/observability/docker-compose.yml up -d` as the README says, then `06-observability.sh` and `06b-observability-more.sh`, `06-screenshots.sh` | **pass, with caveats below.** Six services pulled (digest-pinned), started and answered; two long-lived agents (deploy-agent, hello-agent), the `a2a` CLI, a deploy-agent over a real PostgreSQL and `tool-loop-agent` against a scripted model exported OTLP to the Collector |

## The observability stack

* **Where each signal arrived** (`evidence/06-observability.log`): Prometheus held series for `deploy-agent`,
  `hello-agent` and `a2a-cli` (`count by (job, instance)`), plus the stack's own jobs; the recording rule
  `a2a:rpc_server_calls:rate5m` had 9 series; 42 rules in 6 groups were loaded and none fired. Tempo returned traces for
  all three services. Loki listed `a2a-cli`, `deploy-agent` and `hello-agent` as `service_name` values.
* **The repository's panel checker** (`deploy/observability/scripts/check_dashboards.py --grafana ... --prometheus ...`,
  every query of every panel through Grafana's own query API): after 150 s of traffic the A2A server and client
  dashboards had data in every panel but the ones named below, the **GenAI dashboard was empty** (no model agent had
  run), and Store operations had no data. After `06b` (a scripted model through `tool-loop-agent`, a PostgreSQL-backed
  deploy-agent) and a 30-minute lookback (`evidence/06c-check-dashboards-final.log`) **every GenAI panel and both Task store
  panels had data**. The store panels had been empty at the first check only because the process had exported its
  metrics once (the SDK's default metric export interval, as I understand the OpenTelemetry default of 60 s, was not changed): the panel query itself, run by hand, returns nine series
  (`db_client_operation_duration_seconds_count` by `db_operation_name`). Still empty: *Failed calls (ratio)* (client
  and server), *Failed calls per second*, *Persistence errors* and *Push deliveries*, and the recording rules over
  failures, cancellations, push, store retries and executor failures. **That is what the walkthrough page says to expect**
  ("*Failed calls* stays empty ... *Persistence errors* stays empty"): the traffic produced caller errors
  (task-not-found), no server failure, no failing store and no webhook. Push deliveries were not exercised (the page's
  traffic script sends them to a receiver the agent may reach; deploy-agent refuses private webhook URLs by default).
* **Looked at** (headless Chrome, scale factor 1, `evidence` holds no images; they were for checking): the A2A server
  dashboard has lines in Calls by binding and by method, Caller errors, Server span duration with exemplars, p50/p95/p99,
  Errors by method and kind, Requests without a response, Active event queues, HTTP connections (at zero, drawn),
  Store operations and duration, and Executor runs and duration; the GenAI dashboard has lines in all nine panels (flat,
  because each process was short-lived, and labelled with the example's default model name `qwen3:1.7b` though the model
  was a stub). The *Persistence errors* and *Push deliveries* panels read "No data".
* What differs from the page: it builds `a2a-tck-sut` and drives all four bindings with `scripts/drive.sh`; this run used
  the already-built deploy-agent and hello-agent, so only JSON-RPC and HTTP+JSON appear in *Calls by binding*, and gRPC
  and WebSocket were not observed in the stack. A password in `A2A_DATABASE_URL` is logged as `postgres://postgres:***@`.
* `deploy/observability/scripts/check_stack.py` (static): `check_stack: ok`.
* Torn down with `docker compose down -v`; the images were pruned.

## Findings

* **D-17 (polish).** `OPTIONS` on a REST-dispatcher path with no CORS configured answers `200` with the
  health body `{"status":"ok"}` (`dispatch/rest/mod.rs:134`, `health_response()`), while the
  JSON-RPC dispatcher answers `204` (`dispatch/jsonrpc/mod.rs:107`). Observed on deploy-agent's card path.
  A browser's preflight still fails for want of `Access-Control-Allow-*` headers, which is the correct
  effect; the body and the status disagree between the two bindings.

## Not run

* **`docker build -f examples/deploy-agent/Dockerfile -t deploy-agent .` and running the image.** The disk allowance
  (about 4 GB free after the observability stack was removed) cannot hold the `rust:1.94-bookworm` layers and the
  release build's target directory (several GB each). What was checked instead: BuildKit's own linter
  (`docker buildx build --check`: `Check complete, no warnings found.`, `evidence/06-image-digests.log`'s companion), that both
  base-image digests resolve, and that the process the image would run behaves as its README says. The HEALTHCHECK
  (a `bash` `/dev/tcp` probe) was not run, and neither was the `--secret id=cacert` path.
* Kubernetes: see the table. The `deploy/kubernetes/observability` generated manifests: `check_stack.py` (static) passed;
  not applied anywhere.
