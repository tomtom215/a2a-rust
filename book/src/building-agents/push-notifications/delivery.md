<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Delivery and retries

What `HttpPushSender` does with each event: which URLs it will deliver to, how it guards against server-side request forgery, how often it retries, and what each delivery is counted as.

## What `HttpPushSender` checks

The built-in `HttpPushSender` includes:

- **HTTPS delivery** — With the `tls-rustls` feature (enabled by default via the `a2a-protocol-sdk` crate) it delivers to both `http://` and `https://` webhooks. In a build with the feature disabled it is plaintext-HTTP only and fails fast on an `https://` target with a clear error.
- **SSRF protection** — Resolves the host at delivery and rejects a URL that is, or resolves to, a private, loopback, link-local, CGNAT or IPv4-mapped address, a numeric IPv4 spelling such as `0x7f.1`, or an internal name (`url_guard::resolve_public_url`, public for your own senders). Resolving before validating prevents DNS rebinding attacks where a hostname initially resolves to a public IP but later resolves to a private IP. A config is also screened when it is created, without a DNS lookup, so a hostname that resolves privately is stored but never delivered to. For `http://` the validated IP is pinned at connect time; for `https://` the rebinding window is closed by TLS certificate verification instead (so the original hostname is preserved for SNI). `with_allowed_hosts([..])` restricts webhooks to listed hosts (checked at creation and at delivery); `allow_private_urls()` turns the screen off for tests.
- **Header injection prevention** — Validates credentials contain no `\r` or `\n`
- **HTTPS only, if you ask** — `HttpPushSender::new().https_only()` refuses an `http://` webhook (spec §13.2: webhook URLs SHOULD use HTTPS). A push config with one is refused at creation with `InvalidParams`, and a delivery to one is refused before connecting. Off by default.

## Retries and time limits

<figure class="diagram">
<a href="../../images/diagrams/bb-push-delivery.light.svg"><img class="diagram-light" src="../../images/diagrams/bb-push-delivery.light.svg" alt="Flowchart of one delivery: the sender checks the host allowlist and HTTPS rule, resolves the host and refuses a private address, then POSTs; a 2xx is delivered, a non-retryable status fails at once, and a retryable failure is attempted again after 1 s then 2 s up to 3 attempts; push_delivery_timeout bounds the whole delivery and reports timeout or timeout_truncated when it expires first."></a>
<a href="../../images/diagrams/bb-push-delivery.dark.svg"><img class="diagram-dark" src="../../images/diagrams/bb-push-delivery.dark.svg" alt=""></a>
<figcaption>One delivery to one webhook at the defaults, and the outcome label it is counted under.</figcaption>
</figure>

Every config of a task is attempted for every event,
concurrently (`HandlerLimits::push_delivery_budget`, 30 s, sets how many at
once; at the defaults, all). One attempt is bounded by the sender's request
timeout, 10 s (`HttpPushSender::with_timeout`), after a DNS lookup bounded by
5 s (`with_dns_timeout`). `PushRetryPolicy` (`with_retry_policy`) allows 3
attempts with 1 s then 2 s between them (a shorter `backoff` list repeats its
last entry; 0 attempts counts as 1). A transport error, a timeout, `408`,
`429` and any `5xx` are retried with the same body; any other status, a
redirect included (it is not followed), fails the delivery at once, and the
response body is not read. A whole delivery, retries included, is bounded by
`HandlerLimits::push_delivery_timeout` (30 s); a webhook that never answers
needs up to 38 s at the defaults, so the third attempt can be cut short (the
`timeout_truncated` outcome of `a2a.server.push_deliveries`). A failed delivery
is counted (and logged, with the `tracing` feature) and does not fail the task.

## Order, and a webhook slower than the agent

Each task's deliveries go through one lane, on a task of their own: an event
is stored, its delivery is handed to the lane, and the next event is stored
without waiting for any webhook. The lane runs its deliveries one after
another, so a webhook is sent a task's events in the order they were stored.
A blocking `SendMessage` hands over the whole request's events as one
delivery in the same lane, so this holds across turns whichever send path ran
each one. Lanes are per task: a webhook that hangs holds back its own task's
notifications and nothing else, not the task's state, its event log, its
streams, or a continuation sent to another replica. Shutdown waits for the
lanes as it waits for executors, within the same grace period.

A lane holds up to `HandlerLimits::push_queue_capacity` deliveries waiting
behind the one under way (1,024). When it is full, the oldest waiting
delivery that does not end a turn is dropped and counted as `dropped`, so a
webhook that cannot keep up loses intermediate events (artifact chunks,
`working`) and is still sent the terminal or interrupted status that ends
each turn. Spec §4.3.3 allows an agent to stop delivering after a configured
number of failures; this bound is that number, counted in waiting deliveries.

Before 2026-10-30 the streaming path delivered inline: a webhook that did not
answer held each later event out of the store for up to
`push_delivery_timeout`, back-pressured the executor once 1,024 events were
buffered, and held the turn's claim, so a continuation sent to another
replica was refused as "already being processed".

## Outcomes

Each delivery is counted once in `a2a.server.push_deliveries`, labelled with
its outcome:

| `outcome` | Meaning |
|---|---|
| `delivered` | The webhook answered `2xx`. |
| `failed` | The webhook refused it with a status that is not retried, every attempt failed, or the sender refused the delivery (a screened URL, an unsendable credential). |
| `timeout` | `push_delivery_timeout` expired before the sender finished. |
| `dropped` | Never started: the task's lane was full (`push_queue_capacity`), because the webhook is slower than the agent. Counted once per dropped event (or blocking request), not per webhook. |
| `timeout_truncated` | The same, for a sender whose own schedule (`PushSender::max_delivery_duration`) is longer than `push_delivery_timeout`, so its last attempts could never run in full. At the defaults (38 s against 30 s) every timeout is this one; the fix is to lengthen the timeout or shorten the schedule. |

## Security considerations

- **Always use HTTPS** for webhook URLs in production
- The built-in `HttpPushSender` rejects private IP addresses to prevent SSRF attacks
- Webhook credentials are validated for header injection characters
- Consider rate limiting webhook delivery to prevent abuse
