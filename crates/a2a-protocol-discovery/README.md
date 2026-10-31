<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# a2a-protocol-discovery

A2A agents announced and found on the local network with multicast DNS
service discovery (RFC 6762, RFC 6763): service type `_a2a._tcp.local.`,
TXT `path=` and `scheme=` naming where the agent card is served.

```text
cargo run -p a2a-protocol-discovery --example browse
```

mDNS is unauthenticated: an announcement says where a card claims to be.
Require a signed card (`CardFetchOptions::with_signature_keys`) when what is
found decides what an agent may do. One link only; not a registry.

Not published yet. Design and evidence:
[ADR 0029](../../docs/adr/0029-local-network-discovery.md); guide:
[the book](../../book/src/concepts/agent-cards/local-discovery.md).
