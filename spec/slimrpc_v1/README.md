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
- **`scripts/check_slimrpc_spec.sh`** clones upstream and takes its file
  inventory from `main` rather than from a list kept here, so a spec file that
  upstream *adds* fails CI as loudly as one it changes. It also surveys the
  other branches: a spec file that exists only on a branch must be named in
  that script with a one-line disposition, so an untriaged one fails.

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

## Upstream branches, and what is on them

Verified 2026-08-26 by cloning all branch tips. `main` carries the two files
vendored here. Two other branches carry specifications that have never been
merged, and the first of them is the reason this survey exists at all:

| branch | file | status |
|---|---|---|
| `feat/slimrpc-collaborative-channel` | `spec/v1/slimrpc-collaborative-channel.md` | **not implemented here.** The official `a2a-slimrpc` crate (v0.2.6) implements it — `experimental.slimrpc.collaborative_channel.v1.CollaborativeChannelService`, `Collaborate`, and `slim-src` sender attribution are all present in its source. |
| `feat/slimrpc-channel-moderator` | `spec/v1/slimrpc-channel-moderator.md` | not implemented by this binding or by the official crate. |

Three further branches (`feat/slimrpc-multicast-spec`, `feat/spec-versioning`,
`fix/slimrpc-spec-myorg-to-mydomain`) carry only the pre-versioning `spec/`
layout that `spec/v1/` on `main` superseded.

Collaborative channels are **not** what this binding's multicast support does,
and the two are not substitutes. `slimrpc-multicast.md`, which is on `main` and
is implemented here, fans one request out to N agents and returns per-agent
outcomes to the originating client. Collaborate is many-to-many: members see
each other's traffic, attributed by `slim-src`. The official crate's only use of
the word "multicast" is SLIM's `multicast_stream_stream` transport primitive,
which is how it carries Collaborate — it does not implement the multicast
specification. So the two implementations diverge in both directions.

## Re-vendoring

```sh
./scripts/check_slimrpc_spec.sh --update
sha256sum spec/slimrpc_v1/*.md      # update the table above
```

These files are **verbatim upstream copies**. Do not edit them — anything this
project has to say about the binding belongs in the crate's own README, the
book chapter, or this file.
