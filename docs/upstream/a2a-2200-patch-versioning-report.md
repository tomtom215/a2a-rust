<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Issue for `a2aproject/A2A` — §3.6 versioning vs. v1.0.1's transport-mapping changes

**Status: FILED 2026-09-01** as
[a2aproject/A2A#2200](https://github.com/a2aproject/A2A/issues/2200), by the
maintainer. Open, no maintainer response at the time of writing.

This is the second of two upstream threads from the same finding, and it is the
root-cause one. Its sibling,
[a2aproject/a2a-tck#231](https://github.com/a2aproject/a2a-tck/issues/231)
(recorded in `docs/upstream/a2a-tck-231-spec-pin-report.md`), asks the
conformance kit to refresh a superseded snapshot. This one asks the
specification why the refresh was not obviously needed.

An earlier attempt put the *kit's* report on this repository by mistake
([#2199](https://github.com/a2aproject/A2A/issues/2199)); it was closed and
refiled correctly, and this issue was opened fresh rather than reusing it.

## What this report does not say

Recorded first, because both were live risks while drafting and the finished
text avoids them:

* **It does not say the change was silent.** `v1.0.1`'s release notes list it —
  *"fix(spec): recent transcoding-related error changes (#1627)"*, under Bug
  Fixes. Claiming otherwise would have been false and checkable in one click.
* **It does not ask for a revert.** `757f0ec`'s own message is *"Fix error code
  mappings table so that http codes correspond to `google.rpc.Code`-defined
  mappings"*, closing [#1596](https://github.com/a2aproject/A2A/issues/1596).
  The new table is the better one. The report says so explicitly, and asks only
  about how the change interacts with §3.6 and how an implementer is meant to
  notice it.

The narrowing came from reading the commit and the changelog rather than the
diff alone. A first draft of the caveat — carried in
`docs/official-tck-findings.md` §21.1 before this was checked — implied a
wire-affecting change had been slipped into a patch. It was not slipped; it was
deliberate, reasoned and announced. What remains true is only that §3.6 promises
patch releases do not affect compatibility, and this one did.

---

## Title

`[Bug]: §3.6 says patch versions do not affect protocol compatibility, but v1.0.1 changed six §5.4 transport mappings`

## The argument, as submitted

§3.6 (*Versioning*) says patch numbers "do not affect protocol compatibility",
"SHOULD NOT be used in requests, responses and Agent Cards", and "MUST not be
considered when clients and servers negotiate protocol versions". `v1.0.1`
changed six of §5.4's nine rows — HTTP statuses and gRPC statuses, both
observable on the wire.

| Error | v1.0.0 | v1.0.1 |
|---|---|---|
| `TaskNotCancelableError` | `409 Conflict` | `400 Bad Request` |
| `PushNotificationNotSupportedError` | `UNIMPLEMENTED` | `FAILED_PRECONDITION` |
| `UnsupportedOperationError` | `UNIMPLEMENTED` | `FAILED_PRECONDITION` |
| `ContentTypeNotSupportedError` | `415 Unsupported Media Type` | `400 Bad Request` |
| `InvalidAgentResponseError` | `502 Bad Gateway` | `500 Internal Server Error` |
| `VersionNotSupportedError` | `UNIMPLEMENTED` | `FAILED_PRECONDITION` |

A `v1.0.0`-conformant server answers `409` where a `v1.0.1`-conformant server
answers `400`. Both are "A2A 1.0", both are correct against the text they
implement, and §3.6 forbids the only mechanism that could distinguish them.

**Why it is not theoretical.** Two independent implementations read the
`v1.0.0` table and had no signal it had moved: `a2aproject/a2a-tck`, the
project's own conformance kit, which still grades against it (a2a-tck#231); and
this SDK, which implemented it until 2026-08-30 and found the drift only by
testing against an agent built on a different SDK and noticing they disagreed —
not by reading anything. Nothing in `docs/` records that the table changed or
what it used to say.

**What would resolve it**, either one being sufficient: qualify §3.6 to say a
patch may correct transport mappings and how that is to be discovered; or note
under §5.4 (or in `whats-new-v1.md`) that the table was corrected in `v1.0.1`,
with the prior values and a pointer to #1627. A third option, if the
classification rather than the text is the thing to fix: treat wire-observable
corrections as warranting a `Minor` bump, so the version an agent advertises
continues to determine what it answers.

## Verification

```
$ git clone -q https://github.com/a2aproject/A2A /tmp/A2A
$ git -C /tmp/A2A show v1.0.0:docs/specification.md | grep -n "^### 3.6"
708:### 3.6 Versioning

$ git -C /tmp/A2A log -1 --format='%cI %s' 757f0ec
2026-04-14T15:37:40-04:00 fix(spec): recent transcoding-related error changes (#1627)

$ for r in v1.0.0 v1.0.1 main; do
    printf '%-8s ' "$r"
    git -C /tmp/A2A merge-base --is-ancestor 757f0ec "$r" && echo has || echo lacks
  done
v1.0.0   lacks
v1.0.1   has
main     has

$ grep -n "recent transcoding-related error changes" /tmp/A2A/CHANGELOG.md
9:* **spec:** recent transcoding-related error changes ([#1627](...)) ([757f0ec](...))
   # under "## [1.0.1] ... (2026-05-26)", heading "### Bug Fixes"
```

## Duplicate-check note — what it did and did not cover

`a2aproject/A2A`'s issues were read for "patch version", "error code mappings",
"409" and "versioning". The neighbours found, none of them this:

* [#2184](https://github.com/a2aproject/A2A/issues/2184) — a §3.6.1
  contradiction, header vs. request parameter for `A2A-Version`. Same section,
  different defect.
* [#1925](https://github.com/a2aproject/A2A/issues/1925) — a proposal for a
  predictable documentation and release cadence on `main`. Adjacent, since
  discoverability of in-version changes is part of it.
* [#1596](https://github.com/a2aproject/A2A/issues/1596) and #1627 — the issue
  and PR behind the change itself.

**The limitation, stated because it is the same one the a2a-tck#231 report
carries:** the search read the rendered issue list, titles only, not full-text
bodies, because the session that prepared this had GitHub API access scoped to
this repository. A duplicate whose title names none of those four terms would
have been missed. `a2aproject/A2A` is a large repository and this is a weaker
check there than it was on the kit.
