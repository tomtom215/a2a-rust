<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Slice f: security rows SEC-08, SEC-09, SEC-11 to SEC-18

Baseline: `main` at `4a8243c` (confirmed with `git rev-parse HEAD`). I only read the code. I ran no cargo commands and no tests. Every test named below was judged by reading its assertions, and none was run.

## Counts by verdict (10 rows)

| Verdict | Rows |
|---|---|
| already_fixed | 2: SEC-11, SEC-12 |
| partly_fixed | 4: SEC-08, SEC-09, SEC-14, SEC-18 |
| confirmed | 2: SEC-15, SEC-16 |
| needs_decision | 2: SEC-13, SEC-17 |

There are also 6 new findings: NEW-f-1 to NEW-f-6. Two are medium, two are low and two are info.

## The five most important

1. **SEC-09 and NEW-f-1 (medium): the CLI's new `--header` host check can be bypassed.**
   - `resolve_target` (`tools/a2a-cli/src/connect.rs:152`) checks only the card's first interface.
   - `builder.build()` (`connect.rs:216`) then falls back to the card's later interfaces whenever the first cannot be built (`transport_factory.rs:67-69, 133-168`). An unknown binding name or a malformed URL is enough to make the first fail.
   - So a card listing `{NOPE, https://partner.example/}` followed by `{JSONRPC, https://attacker.example/}` passes the check, and the bearer token then goes to the attacker on every call.
   - The e2e test cannot catch this: its card has both interfaces on one host.
   - Still open from the row:
     - `--header` can only come from argv, so tokens show in `ps`.
     - The library's `from_card` has no origin guard at all.
2. **SEC-08 (medium): only the plaintext half is fixed.**
   - Fixed in 779ff31 with good tests:
     - `http://` token endpoints, exchange endpoints and discovery URLs to remote hosts are refused.
     - The discovery document must name the issuer that was asked for.
   - Still open: `from_agent_card` sends the client secret to any `https://` `tokenUrl` the card names (`token_provider.rs:418-419`). That was the report's first exploit. The book page still recommends this constructor with no warning.
3. **SEC-14 and NEW-f-2: push amplification is bounded, but one caller can now take the bound.**
   - ee0a4c4 caps webhook deliveries at 512 in flight overall and 64 per host:port. Unit and handler tests cover this, and it holds in front of custom senders too.
   - Still missing: a per-caller quota (the 100k config table is shared), a per-destination rate, URL verification, and keying on the resolved address rather than the host name.
   - The gate has no fairness between callers, and time spent waiting counts against `push_delivery_timeout`. One caller pointing at a tarpit on 8 ports can hold all 512 permits. Every other tenant's webhooks then time out without being attempted.
4. **NEW-f-3 (medium): the proxy fix and the SSRF check do not fit together.**
   - Since c4404e8, push delivery goes through `HTTPS_PROXY`.
   - The SSRF pre-flight still resolves the webhook host locally and refuses delivery if that lookup fails. Where DNS answers only internal names, every SSRF-checked webhook fails. The only switch that delivers is `allow_private_urls()`, which turns off all SSRF checks.
   - For `https` the proxy resolves the name again, which widens SEC-13.
   - The proxy test uses `allow_private_urls()`, so it never exercises this case.
5. **SEC-12: already fixed, with a caveat (NEW-f-4, low).**
   - 8bdf383 refuses every URL and range the report lists. The tests check each range at both edges, and reverting the fix would fail them.
   - `%`-encoded hosts are not a bypass: the `http` crate refuses `%` in a host (I read its parser).
   - What remains is a design limit. A deny-list cannot know Kubernetes search-domain names such as `kubernetes.default`, or a cluster that uses public address space internally. One example, from memory and not verified: GKE Autopilot's default Service range is 34.118.224.0/20.

## The other rows, briefly

- **SEC-11 (already_fixed):** the default tenant is now `sha256:` plus 16 bytes of the token's hash (5b45b97). The known-answer test would fail if this were reverted.
  - Note: the `with_mapper` rustdoc example still builds the tenant from the raw token (NEW-f-6).
- **SEC-13 (needs_decision):** `https` webhooks are still not pinned to the checked address. The options and the proxy question are in the JSON.
- **SEC-15 and SEC-16 (confirmed):** both examples are unchanged since the audit.
- **SEC-17 (needs_decision):**
  - The book tells custom senders to screen URLs with `resolve_public_url`, and has done so since before the audit.
  - The trait's rustdoc says nothing about this, and nothing anywhere says a sender must not follow redirects.
  - The handler adds no check at delivery time.
- **SEC-18 (partly_fixed):**
  - `crit` is now refused, and the test has a positive control.
  - `typ` is still ignored.
  - `constant_time_eq` is still a plain loop, both in the server and in the client's webhook-token copy.

## GDPR and EU AI Act surface

- **SEC-11:** the stored and exported tenant is now a hash of the token, not the credential. If one token belongs to one person, that hash is still pseudonymous personal data and is exported to trace backends.
- **Webhook spans:** they record `url.full` with the path removed (`rpc_span/webhook.rs:267-280`). Delivery logs record only the origin. I found no new leak of personal data in these rows.

## Not done or not checked

- No test was run, so "would fail if reverted" is my reading of each test, not a measurement.
- These were reasoned from the code and not observed:
  - the CLI fallback bypass;
  - the gate starvation;
  - the proxy and DNS failure.
  
  Each has a probe described in `f.json`.
- The GKE Service range is from memory.
- Whether a given terminal acts on UTF-8-encoded C1 control characters (NEW-f-5) depends on the terminal; I did not test any.
- N-SEC-1, N-SEC-3, N-SEC-4 and N-SEC-6 are not in my slice. I looked at them only where they overlap my rows.
