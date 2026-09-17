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

Verified 2026-08-26 by cloning all branch tips, and re-verified 2026-09-12 the
same way after the nightly `Official TCK` run of that morning failed on two
untriaged files. `main` carries the two files vendored here. Two other branches
carry specifications that have never been merged, and the first of them is the
reason this survey exists at all:

| branch | file | status |
|---|---|---|
| `feat/slimrpc-collaborative-channel` | `spec/v1/a2a-collaborative-task.md` (since 2026-09-11, `cb245fc`, where it replaced `spec/v1/a2a-broadcast-live.md`: the transport-independent half, reframed around a relay that joins each agent's task to its peers) | **not implemented here.** It defers every transport question to the SLIMRPC profile below, so it is not this binding's to implement; and its §4.3 makes appending peer messages to A2A 1.1's `timeline` as `TimelineEntry(Message)` a MUST, while the `StreamRequest` vocabulary of its §5.1 translation table — `TaskMessageUpdateEvent` included — is absent from `proto/a2a_v1/a2a.proto` and from both specs vendored here. Narrower than the objection to its predecessor: §2.1's basic relay tier does claim A2A 1.0+, so §4.3 is what blocks it, not the method surface. Re-triage when A2A 1.1 ships. Triaged in `scripts/check_slimrpc_spec.sh`. |
| `feat/slimrpc-collaborative-channel` | `spec/v1/a2a-shared-task.md` (since 2026-09-10, `40c2360`: the shared-task extension the collaborative-task design builds on) | **not implemented here.** Transport-independent — an extension URI (`https://a2a-protocol.org/extensions/shared-task/v1`), a `message-sender` metadata key (renamed from `task-sender` and namespaced under that URI at `1679a1b`, 2026-09-10) and multi-sender rules, nothing SLIMRPC-specific — so it is not this binding's to implement; its §3.3 preserves the sender on 1.1's `TimelineEntry`, which no released A2A specification defines. Re-triage with `a2a-collaborative-task.md` when A2A 1.1 ships. Triaged in `scripts/check_slimrpc_spec.sh`. |
| `feat/slimrpc-collaborative-channel` | `spec/v1/slimrpc-collaborative-task.md` (since 2026-09-11, `cb245fc`, a 54%-similar rename of `spec/v1/slimrpc-broadcast-live.md`, itself the 2026-09-02 `daddfb2` rename of `spec/v1/slimrpc-collaborative-channel.md` — earlier rows here dated that move 2026-09-03 and credited `0c38776`, which touches only `examples/` and was merely the branch tip this shallow-cloning check could see; neither predecessor exists on any upstream branch now) | **not implemented here** — for reasons that have changed, so the previous ones are not being reused. Its §§3.1 and 4 now activate a session with `SendLiveMessage` **or** 1.0's `SendStreamingMessage`, so it no longer requires an A2A 1.1 method. What blocks it now: it is the profile of `a2a-collaborative-task.md` above, whose §4.3 `TimelineEntry` MUST is 1.1-only; its native mode needs SLIM shared-responses group channels (§3.1, `Server.new_with_shared_responses_and_connection`), which this binding does not have — it implements `slimrpc-multicast.md` from `main`, a different operation; and the document has never reached `main`, its text having changed seven further times on 2026-09-11 alone, after the reframe, through the tip `36b03a7`. The official `a2a-slimrpc` crate (v0.2.7) still implements the withdrawn Collaborate design — `experimental.slimrpc.collaborative_channel.v1.CollaborativeChannelService`, `Collaborate`, and `slim-src` sender attribution are all present in its source. Triaged in `scripts/check_slimrpc_spec.sh`. |
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
