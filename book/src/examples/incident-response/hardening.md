<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Production hardening (Act 5)

Acts 1 to 4 of the [Incident-Response Team](../incident-response.md) cover
what an agent is and what the protocol requires. Neither asks the question an
operator asks first: *what happens when this is exposed to more than one
caller?* Act 5 exercises the SDK capabilities that answer it, over a real
socket, and **asserts** each one rather than narrating it. Use this page to
see which capability each check covers and the wrong answer it rules out.

## Run it

```bash
cargo run -p incident-response -- harden    # Act 5 on its own
```

Every check but one runs with nothing installed. The PostgreSQL check needs a
server the example cannot start:

```bash
A2A_TEST_POSTGRES_URL=postgres://user:pass@localhost:5432/db \
  cargo run -p incident-response -- harden
```

The run ends with a tally, for example
`16 passed, 0 failed, 0 not compiled, 0 not run` from the README's recorded
demo. A failed check exits `3`.

## The sixteen checks

The first column is how each check's printed label begins; the label goes on
to say what is asserted, such as
`Client retry (503 retried, 502 not — no double execution)`.

| Check | Capability | The wrong answer it rules out |
|---|---|---|
| Tenant isolation | `TenantAwareInMemoryTaskStore` + `HeaderTenantResolver` | One tenant reading another's tasks — including by *naming* the other tenant in `params.tenant`, which is the v0.6.0 regression this check was written for |
| Bearer-token auth | `BearerTokenAuthInterceptor` (+ the client's `AuthInterceptor`) | An unauthenticated request succeeding — and, in the other direction, a server that refuses everything |
| API-key auth | `ApiKeyAuthInterceptor` | An interceptor that rejects on the header being *present* rather than on its value |
| JWT auth via JWKS | `JwtAuthInterceptor` against a served JWKS | A forged ES256 signature verifying, or an expired token still working |
| Rate limiting | `RateLimitInterceptor` | A limit that accepts everything, or one that accepts nothing |
| Handler limits | `HandlerLimits` | An unbounded caller-supplied `context_id` the server allocates |
| Client retry | `RetryPolicy` (client) | A retry layer that retries a `502` on `SendMessage` — an ambiguous failure a gateway may already have forwarded, so the work can run twice |
| Agent card signing | `sign_card_es256` / `verify_agent_card` | A rewritten interface URL still verifying, which would let a signed card redirect callers to an impostor |
| SQLite persistence | `SqliteTaskStore` | A task that does not survive being read back through a *different* handler over the same file |
| Tenant-partitioned SQLite | `TenantAwareSqliteTaskStore` | Partitions that look right in memory and share one table on disk, leaking the moment the process restarts |
| PostgreSQL persistence | `PostgresTaskStore` | A second SQL backend assumed correct because the first one is |
| Graceful shutdown | `RequestHandler::shutdown` | A shutdown that hangs instead of draining |
| Metrics hook | The `Metrics` hook | Served requests that reach no recorder |
| OpenTelemetry export | `OtelMetrics` | An instrumented handler that exports no `a2a.server.requests` datapoint — checked by collecting from a real `ManualReader`, because the default global meter provider is a no-op and would make any wiring look correct |
| OTLP pipeline | `init_otlp_pipeline` | A pipeline that builds cleanly and never reaches the collector — checked by pointing it at a socket this example owns and requiring HTTP/2 bytes |
| HTTPS push | `HttpPushSender::with_tls_config` | A sender that speaks HTTPS without verifying certificates — the same self-signed cert must be accepted with a trust store naming it and rejected with the default one |

Every check names the specific wrong answer in its failure message, and every
one was proven able to fail by removing the capability it covers.
`scripts/prove_gates_fail.sh` injects the tenant-resolver removal to keep the
act's CI step honest.

## Nothing passes by not running

A capability compiled out prints `[NOT BUILT]` with the feature it needs; one
whose external service is absent prints `[NOT RUN]` with what to set. All six
features are on by default, so a plain run builds every check:

| Feature | Checks it builds |
|---|---|
| `sqlite` | SQLite persistence, tenant-partitioned SQLite |
| `postgres` | PostgreSQL persistence (still `[NOT RUN]` without `A2A_TEST_POSTGRES_URL`) |
| `signing` | Agent card signing |
| `auth-jwt` | JWT auth via JWKS |
| `tls-rustls` | HTTPS push |
| `otel` | OpenTelemetry export, OTLP pipeline |

Only PostgreSQL is in the second category. Set `INCIDENT_REQUIRE_ALL=1` (as CI
does) to make either state exit `4` instead of passing quietly, so a service
that stops being provisioned fails the run rather than downgrading a check to
a printed line.

## Where next

- [Production Hardening](../../deployment/production.md): the same
  capabilities as deployment guidance.
- [Resilient Agent](../resilient-agent.md): restarts, injected failures and
  two replicas, asserted the same way.
- [Deploy Agent](../deploy-agent.md): the container and Kubernetes side.
