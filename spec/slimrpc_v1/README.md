# Vendored SLIMRPC specification

Upstream source of truth for the `a2a-protocol-slimrpc` binding.

| | |
|---|---|
| Upstream | [`a2aproject/experimental-cpb-slimrpc`](https://github.com/a2aproject/experimental-cpb-slimrpc) |
| Branch | `main` |
| Vendored | 2026-09-30, at upstream `1328426` (first vendored 2026-08-16) |
| Files | every `spec/**/*.md` on upstream `main`: `spec/v1/slimrpc.md`, `spec/v1/slimrpc-multicast.md`, `spec/v1/a2a-collaborative-task.md`, `spec/v1/a2a-shared-task.md`, `spec/v1/slimrpc-collaborative-task.md` |
| Implemented | the A2A 1.0 surface of `slimrpc.md` and `slimrpc-multicast.md`; see below |

```
3b62aae218d3f5aacc5748ef7833b478454a055c330bf3b40073209561fab1b7  a2a-collaborative-task.md
09624ccafb7bba97e3a2dd729d3b717e56d996f4f9b38f4b592531e6b4feefb1  a2a-shared-task.md
ef95099c806b9f4856364720dd3936a289ec53e12d4fd3b5d654df23061745f2  slimrpc-collaborative-task.md
09842ae391d4265ddfadab303b063844516bf37e3fc8c0810a0f98830b2a195d  slimrpc-multicast.md
ad52a877c965675a24552be118e4f81a6a9cf581c2c755232c22a8d90efec03f  slimrpc.md
```

Every file upstream carries on `main` is vendored, because
`scripts/check_slimrpc_spec.sh` takes its inventory from upstream rather than
from a list kept here. Vendored is therefore not the same as implemented: the
three collaborative-task documents are here so that a change to them fails CI,
not because this binding follows them.

## Why these are here

`bindings/a2a-protocol-slimrpc` claims to implement every A2A 1.0 method in
this specification's inventory. Until this directory existed, that claim referenced a
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

## The 2026-09-30 re-vendor, and why the binding did not follow

The nightly `Official TCK` run of 2026-09-30 failed on this check. Upstream had
merged its collaborative-task work to `main` the day before as `1328426`
(upstream PR #5, 2026-09-29), which added three files to `main` and changed
both files vendored at the time. The whole diff was read. The decision is that
`bindings/a2a-protocol-slimrpc` does not change, because everything the merge
added to the implemented documents is A2A 1.1, and no A2A 1.1 exists yet: the
newest tag in `a2aproject/A2A` is `v1.0.1`, and the `v1.1.0` specification URL
the new text links to returned 404 that day.

- **`slimrpc.md`** gains `SendLiveMessage`, a bidirectional streaming method
  marked "1.1+", and an "A2A Version" column marking the other eleven "1.0+";
  their names, request types and response types are unchanged. It also reserves
  one metadata key, `slimrpc-context-map`, used only on `SendLiveMessage`, where
  it previously reserved none. The document already says that implementations
  targeting an older version of A2A **MUST** use that version's method names,
  which is what the binding does.
- **`slimrpc-multicast.md`** gains a new §8, multicast `SendLiveMessage`
  (requiring A2A 1.1 of every participant); the former §8 "Error Handling"
  becomes §9, with one added sentence extending its failure isolation to §8.
  Sections 1–7 and 9 are what this binding implements, and their A2A 1.0 text is
  unchanged.
- **The three new files** are the collaborative-task documents triaged below.
  Their merged text is byte-identical to the branch tip `36b03a7` that the triage
  was written against, so the reasons for not following them stand; only the
  statement that they had never reached `main` stopped being true.

`scripts/check_method_denominator.py --slimrpc-spec` still reports 11 methods
against the new `slimrpc.md`: it matches the eleven A2A 1.0 names, so the
twelfth neither counts toward nor breaks the denominator.

## Vendored, not implemented

| file (on upstream `main` since 2026-09-29, `1328426`) | status |
|---|---|
| `spec/v1/a2a-collaborative-task.md` (since 2026-09-11, `cb245fc`, where it replaced `spec/v1/a2a-broadcast-live.md`: the transport-independent half, reframed around a relay that joins each agent's task to its peers) | **not implemented here.** It defers every transport question to the SLIMRPC profile below, so it is not this binding's to implement; and its §4.3 makes appending peer messages to A2A 1.1's `timeline` as `TimelineEntry(Message)` a MUST, while the `StreamRequest` vocabulary of its §5.1 translation table — `TaskMessageUpdateEvent` included — is absent from `proto/a2a_v1/a2a.proto`. (Since 2026-09-29 `slimrpc.md` and `slimrpc-multicast.md` name `StreamRequest`, but only as the request type of A2A 1.1's `SendLiveMessage`; neither names `TaskMessageUpdateEvent`.) Narrower than the objection to its predecessor: §2.1's basic relay tier does claim A2A 1.0+, so §4.3 is what blocks it, not the method surface. Re-triage when A2A 1.1 ships. Triaged in `scripts/check_slimrpc_spec.sh` while it was branch-only, and moved here when upstream merged it. |
| `spec/v1/a2a-shared-task.md` (since 2026-09-10, `40c2360`: the shared-task extension the collaborative-task design builds on) | **not implemented here.** Transport-independent — an extension URI (`https://a2a-protocol.org/extensions/shared-task/v1`), a `message-sender` metadata key (renamed from `task-sender` and namespaced under that URI at `1679a1b`, 2026-09-10) and multi-sender rules, nothing SLIMRPC-specific — so it is not this binding's to implement; its §3.3 preserves the sender on 1.1's `TimelineEntry`, which no released A2A specification defines. Re-triage with `a2a-collaborative-task.md` when A2A 1.1 ships. Triaged in `scripts/check_slimrpc_spec.sh` while it was branch-only, and moved here when upstream merged it. |
| `spec/v1/slimrpc-collaborative-task.md` (since 2026-09-11, `cb245fc`, a 54%-similar rename of `spec/v1/slimrpc-broadcast-live.md`, itself the 2026-09-02 `daddfb2` rename of `spec/v1/slimrpc-collaborative-channel.md` — earlier rows here dated that move 2026-09-03 and credited `0c38776`, which touches only `examples/` and was merely the branch tip this shallow-cloning check could see; neither predecessor exists on any upstream branch now) | **not implemented here** — for reasons that have changed, so the previous ones are not being reused. Its §§3.1 and 4 now activate a session with `SendLiveMessage` **or** 1.0's `SendStreamingMessage`, so it no longer requires an A2A 1.1 method. What blocks it now: it is the profile of `a2a-collaborative-task.md` above, whose §4.3 `TimelineEntry` MUST is 1.1-only; its native mode needs SLIM shared-responses group channels (§3.1, `Server.new_with_shared_responses_and_connection`), which this binding does not have — it implements `slimrpc-multicast.md` from `main`, a different operation; and until 2026-09-29 the document had never reached `main`, its text having changed seven further times on 2026-09-11 alone, after the reframe, through the tip `36b03a7` — the text upstream then merged unchanged. That last point was a reason while it held; the others still stand. The official `a2a-slimrpc` crate (v0.2.7) still implements the withdrawn Collaborate design — `experimental.slimrpc.collaborative_channel.v1.CollaborativeChannelService`, `Collaborate`, and `slim-src` sender attribution are all present in its source. Triaged in `scripts/check_slimrpc_spec.sh` while it was branch-only, and moved here when upstream merged it. |

## Upstream branches, and what is on them

Verified 2026-08-26 by cloning all branch tips, and re-verified 2026-09-12 the
same way after the nightly `Official TCK` run of that morning failed on two
untriaged files, and again on 2026-09-30 after upstream's merge.
`feat/slimrpc-collaborative-channel` still exists, but every spec file on it is
now on `main` too, so it carries nothing branch-only. One other branch carries a
specification that has never been merged. `feat/a2a-collaborate-plugin`, new
since the last survey, changes `examples/` only and no spec file.

| branch | file | status |
|---|---|---|
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
