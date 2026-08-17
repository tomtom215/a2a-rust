# Vendored SLIMRPC specification

Upstream source of truth for the `a2a-protocol-slimrpc` binding.

| | |
|---|---|
| Upstream | [`a2aproject/experimental-cpb-slimrpc`](https://github.com/a2aproject/experimental-cpb-slimrpc) |
| Branch | `main` |
| Vendored | 2026-08-16 |
| Files | `spec/v1/slimrpc.md`, `spec/v1/slimrpc-multicast.md` |

```
768c2a08e26b9f8b1d4a384572ecd01f23a94b6d437b24645713b9a02532a1c7  slimrpc.md
5f227bdda9d5b64b07b25a804036a9e5893383af213eeef24b7e32f4f650b36b  slimrpc-multicast.md
```

## Why these are here

`bindings/a2a-protocol-slimrpc` claims to implement every method in this
specification's inventory. Until this directory existed, that claim referenced a
URL: nothing in the repository could check it, and nothing would notice if
upstream added a method, renamed one, or changed the wire format underneath the
binding.

That was an asymmetry rather than a considered decision. The gRPC binding's
governing artifact — `proto/a2a_v1/a2a.proto` — *is* vendored, and
`scripts/check_proto_copies.sh` asserts every copy of it stays byte-identical.
The SLIMRPC binding had no equivalent, so its conformance claim was the one
claim in this repository that could only be verified by opening a browser.

Two checks close that:

- **`scripts/check_method_denominator.py --slimrpc-spec`** holds the binding's
  method inventory to the inventory named here, so a method the binding stops
  serving fails CI. Previously this was checked against the A2A proto, which is
  a reasonable proxy but is not the document the binding claims to implement.
- **`scripts/check_slimrpc_spec.sh`** re-fetches upstream and compares hashes,
  so a change to the specification is surfaced as a CI failure rather than
  discovered by a user.

## Upstream status

**Community-contributed and experimental.** The upstream README describes
itself as "not part of the core A2A specification", and the ratified A2A v1.0
specification contains no occurrence of "slim" or "agntcy". Nothing here is
required for A2A conformance — see
[the book chapter](https://a2a-rust.com/bindings/slimrpc.html).

Because it is experimental, upstream may change without ceremony. A drift
failure is therefore *information*, not necessarily a defect: read the diff,
decide whether the binding must follow, then re-vendor and update the hashes
above in the same commit that records the decision.

## Re-vendoring

```sh
./scripts/check_slimrpc_spec.sh --update
sha256sum spec/slimrpc_v1/*.md      # update the table above
```

These files are **verbatim upstream copies**. Do not edit them — anything this
project has to say about the binding belongs in the crate's own README, the
book chapter, or this file.
