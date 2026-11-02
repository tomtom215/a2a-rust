<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Live results log (this session). One entry per run; newest last.
Environment: main@4a8243c, dev build with CI flags (RUSTFLAGS=-D warnings, CARGO_PROFILE_DEV_DEBUG=0).

## LR-1  SEC-09 bypass (triage NEW-f-1)  -- CONFIRMED LIVE, 2026-11-01
Command: python3 -I probes/probe_sec09.py ./target/debug/a2a <a card saved with: a2a card http://127.0.0.1:3000 > base_card.json> {direct|fallback}
- direct  (card names only http://localhost:B/): exit 1, "a different host ... --header would send your credentials there"; attacker saw nothing.  (PR #185 fix works)
- fallback (card: [JSONRPC ftp://127.0.0.1/, JSONRPC http://localhost:B/]; typed http://127.0.0.1:A): the chosen interface (same host) passes the CLI's check, builder.build() fails on the ftp:// scheme (jsonrpc.rs validate_url), fall_back() moves to localhost:B,
  WARN "chosen interface could not be built; using the next one on the card", attacker listener received "Authorization: Bearer TOP-SECRET-TOKEN".  SECRET LEAKED TO OTHER HOST: True.
- A first attempt with a same-host interface on port 99999 did NOT trigger the fallback (the URL builds; connect refused): the triage agent's own example ("NOPE" binding) was not tested; the mechanism is what is confirmed.
Rows: SEC-09 stays partly fixed; NEW-f-1 confirmed (medium).

## LR-2  loopback traffic goes through HTTP_PROXY (triage NEW-b-2)  -- CONFIRMED LIVE, 2026-11-01
Command: python3 -I probes/probe_proxy_loopback.py ./target/debug/a2a http://127.0.0.1:3000 [NO_PROXY]   (hello-agent on :3000; logging proxy answers 502)
- control, no proxy vars: exit 0, card fetched, proxy saw nothing.
- HTTP_PROXY=http://127.0.0.1:<p>, NO_PROXY unset: the CLI sent "GET http://127.0.0.1:3000/.well-known/agent-card.json" to the proxy; exit 1 "error: unexpected HTTP status 502: proxy probe: refused".  LOOPBACK TRAFFIC WENT TO THE PROXY: True
  The error text never mentions a proxy.
- NO_PROXY=127.0.0.1 and NO_PROXY=localhost (url http://localhost:3000): proxy saw nothing, exit 0.
Rows: NEW-b-2 confirmed (medium). Go's net/http exempts loopback by default; curl does not. Policy decision recorded for the maintainer.
Scope: card fetch only. Other outbound paths (token provider, JWKS, push, OTLP) share the matcher by reading, not run.

## LR-1b  SEC-09 bypass, the triage agent's own variant -- CONFIRMED LIVE, 2026-11-01
Command: python3 -I probes/probe_sec09.py ./target/debug/a2a <a card saved with: a2a card http://127.0.0.1:3000 > base_card.json> unknown-binding
Card: [ {protocolBinding:"NOPE", url:http://127.0.0.1:A/ (the typed host)}, {JSONRPC, http://localhost:B/} ]. builder doc: from_card filters only by protocolVersion major;
build() falls back when the chosen interface cannot be built ("a binding this SDK does not implement, a malformed URL"). WARN "chosen interface could not be built ... chosen=NOPE used=JSONRPC url=http://localhost:B/".
Attacker listener received "Authorization: Bearer TOP-SECRET-TOKEN". SECRET LEAKED TO OTHER HOST: True.
Correction to my earlier remark: I had said the NOPE example might be skipped by from_card; it is not. Both NOPE and ftp:// were run and both leak.
