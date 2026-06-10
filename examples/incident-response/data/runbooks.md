# Service Runbooks

## payments-api

Symptoms: 5xx on /v1/charge, latency spikes, "timeout acquiring db connection".

1. Check `payments-db` connection pool saturation (`connections=N/100` in logs).
   Pool exhaustion is almost always downstream of a slow query, not load.
2. Identify slow queries in the db log (`slow query:` lines). A repeated slow
   query holding connections starves the pool for everyone else.
3. Mitigate: kill the offending query, then scale the pool +25% as a buffer.
   Long-term: add the missing index and set a per-query statement timeout.
4. Verify recovery: error rate on /v1/charge back under 0.1% for 10 minutes.

Escalation: page #payments-oncall if errors persist 15 minutes after mitigation.

## checkout

Symptoms: 502 on /checkout/complete, "upstream payments-api unavailable".

1. Checkout is a thin orchestrator — 502s here are almost always an upstream
   incident. Check payments-api first before touching checkout.
2. If payments-api is healthy, inspect checkout's own retry budget; retries
   amplify upstream load during partial outages.
3. Mitigate: enable the static "order received, payment pending" fallback so
   carts are not lost while upstream recovers.

Escalation: #checkout-oncall.

## search

Symptoms: elevated p99, empty result sets.

1. Check index freshness lag (should be < 5 min).
2. Roll the query-parser canary back if lag is normal but errors are up.

Escalation: #search-oncall.
