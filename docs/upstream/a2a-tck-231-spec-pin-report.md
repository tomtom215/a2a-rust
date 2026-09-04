<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Issue for `a2aproject/a2a-tck` — vendored specification pinned to A2A v1.0.0

**Status: FILED 2026-09-01** as
[a2aproject/a2a-tck#231](https://github.com/a2aproject/a2a-tck/issues/231), by
the maintainer. Open, no maintainer response at the time of writing. This file
is the record of what was submitted and the evidence behind it; the issue is
the live thread.

It was first opened against the specification repository in error
([a2aproject/A2A#2199](https://github.com/a2aproject/A2A/issues/2199)) and
refiled here. Every artefact the report cites — `specification/version.json`,
`tck/requirements/base.py`, `make spec` — belongs to the kit, not to `A2A`,
whose `specification/` holds the proto and JSON schema and whose prose lives at
that repository's own `docs/specification.md`.

This is the finding recorded in `docs/official-tck-findings.md` §20 and §21,
with §21.1 as the correction that made it filable: §20 had concluded that
upstream "amended the table in place", which is not what happened and would
have been the wrong thing to report.

Everything below was established against a fresh clone of `a2a-tck` at
`de6af18` and of `a2aproject/A2A` at `main`, on 2026-09-01. The upstream form
splits the body across fields; the section headings below are those fields.

---

## Title

`[Bug]: vendored specification is pinned to A2A v1.0.0, so §5.4 error mappings are two releases stale`

## What happened?

`specification/` is pinned to A2A **v1.0.0** and has not been refreshed since
2026-03-13. A2A released **v1.0.1** on 2026-05-28, which rewrote six of §5.4's
nine error-mapping rows. The kit's expectations in `tck/requirements/base.py`
encode the v1.0.0 values, so a server that implements the current specification
fails MUST-level checks for doing the right thing.

This is not a report of a logic bug. The tests assert the table they were
given; the table they were given has been superseded.

### Evidence

`specification/version.json` in that repository records the pin itself:

```json
{
  "downloadTime": "2026-03-13T07:43:14Z",
  "organization": "a2aproject",
  "repository": "A2A",
  "branch": "v1.0.0",
  "commitHash": "173695755607e884aa9acf8ce4feed90e32727a1"
}
```

That commit is A2A's `v1.0.0` tag. `specification/specification.md` is
byte-identical to `git show v1.0.0:docs/specification.md` (md5
`65ae1635632ad20180dc78aad097ec2d`, zero differing lines), as is
`specification/a2a.proto`.

The change landed in A2A `757f0ec` ("fix(spec): recent transcoding-related
error changes", #1627) on 2026-04-14, and shipped in the `v1.0.1` tag on
2026-05-28.

### The six rows

| Error | vendored (A2A v1.0.0) | A2A v1.0.1 and `main` |
|---|---|---|
| `TaskNotCancelableError` | `409 Conflict` | **`400 Bad Request`** |
| `PushNotificationNotSupportedError` | `UNIMPLEMENTED` | **`FAILED_PRECONDITION`** |
| `UnsupportedOperationError` | `UNIMPLEMENTED` | **`FAILED_PRECONDITION`** |
| `ContentTypeNotSupportedError` | `415 Unsupported Media Type` | **`400 Bad Request`** |
| `InvalidAgentResponseError` | `502 Bad Gateway` | **`500 Internal Server Error`** |
| `VersionNotSupportedError` | `UNIMPLEMENTED` | **`FAILED_PRECONDITION`** |

`TaskNotFoundError`, `ExtendedAgentCardNotConfiguredError` and
`ExtensionSupportRequiredError` agree between the two.

### What this does to a run

Five of the six rows are reachable by tests today:

```
CORE-CANCEL-002      [http_json]  expected 409, got 400
STREAM-SUB-003       [grpc]       expected UNIMPLEMENTED, got FAILED_PRECONDITION
GRPC-ERR-002         [grpc]       VersionNotSupportedError, UnsupportedOperationError,
                                  PushNotificationNotSupportedError — all three expect
                                  UNIMPLEMENTED, all three get FAILED_PRECONDITION
HTTP_JSON-STATUS-001 [http_json]  expected 415, got 400
```

`InvalidAgentResponseError`'s row does not appear to be asserted by any test,
so it is stale but currently invisible.

The per-transport pattern is diagnostic rather than circumstantial: each of
these requirements is graded on all three bindings and fails on exactly the one
binding whose cell the two releases disagree about, passing on the others. A
server returning the wrong error would fail all three.

`#207` made this visible rather than causing it. Before it, the
`CORE-CANCEL-002` and `STREAM-SUB-003` tests accepted any error, so two of
these rows were unreachable. Tightening them was correct; it landed on stale
data.

### Suggested fix

1. `make spec` — `scripts/update_spec.sh` already defaults to `--branch main`,
   and the history shows the routine being run before ("feat: update A2A spec
   to commit … and align TCK").
2. Update the six `ErrorBinding` constants in `tck/requirements/base.py`, which
   are hand-maintained rather than derived from the vendored document. A unit
   test parsing §5.4's table out of `specification/specification.md` and
   asserting it against the `ErrorBinding` set would keep step 2 from being
   forgotten the next time step 1 runs.

### The caveat, offered rather than hidden

*(Submitted as written. The section citation in this paragraph is wrong: the
versioning clause is A2A **§3.6**, "Versioning"; §6 is "Common Workflows &
Examples". Noted here rather than silently fixed, because this file's job is to
be the record of what was actually submitted. The quoted text is accurate and
the argument is unaffected. See "Corrections to the submitted text" below.)*

A2A's §6 says patch version numbers "do not affect protocol compatibility" and
"MUST not be considered when clients and servers negotiate protocol versions" —
yet v1.0.1 changed HTTP statuses and gRPC statuses on the wire. Both documents
describe "A2A 1.0" and no peer can signal which patch it targets. A maintainer
could fairly answer that the underlying fault is a wire-affecting change shipped
in a patch release, which belongs upstream in `a2aproject/A2A`. That would not
change which text a conformance kit should grade against: the newest released
text of the version it targets.

*(That question was put to `a2aproject/A2A` the same day, as
[#2200](https://github.com/a2aproject/A2A/issues/2200) — see
`docs/upstream/a2a-2200-patch-versioning-report.md`. It is narrower there than
the paragraph above suggests: the change was announced in `v1.0.1`'s release
notes and was made deliberately, to align the HTTP codes with `google.rpc.Code`,
so the filed issue asks about §3.6 and discoverability rather than about the
change itself.)*

## Relevant log output

```
# 1. What the kit says it pinned
$ cat specification/version.json | head -8
  "downloadTime": "2026-03-13T07:43:14Z",
  "branch": "v1.0.0",
  "commitHash": "173695755607e884aa9acf8ce4feed90e32727a1"

# 2. The vendored copy is that release, byte for byte
$ git clone -q https://github.com/a2aproject/A2A /tmp/A2A
$ git -C /tmp/A2A show v1.0.0:docs/specification.md > /tmp/v100.md
$ diff /tmp/v100.md specification/specification.md && echo IDENTICAL
IDENTICAL

# 3. The change is not in v1.0.0, and is in v1.0.1 and main
$ for r in v1.0.0 v1.0.1 main; do
    printf '%-8s ' "$r"
    git -C /tmp/A2A merge-base --is-ancestor 757f0ec "$r" && echo has-757f0ec || echo lacks-757f0ec
  done
v1.0.0   lacks-757f0ec
v1.0.1   has-757f0ec
main     has-757f0ec

# 4. The two tables, side by side
$ git -C /tmp/A2A show v1.0.0:docs/specification.md | grep -E '^\| `TaskNotCancelableError'
| `TaskNotCancelableError` | `-32002` | `FAILED_PRECONDITION` | `409 Conflict`    |
$ git -C /tmp/A2A show v1.0.1:docs/specification.md | grep -E '^\| `TaskNotCancelableError'
| `TaskNotCancelableError` | `-32002` | `FAILED_PRECONDITION` | `400 Bad Request` |

# 5. Observed failures, full profile, a2a-tck@de6af18
CORE-CANCEL-002 (http_json): Expected error code 409 (TaskNotCancelableError), got 400
STREAM-SUB-003 (grpc): Expected error code UNIMPLEMENTED (UnsupportedOperationError), got FAILED_PRECONDITION
GRPC-ERR-002 (grpc): expected VersionNotSupportedError (UNIMPLEMENTED), got FAILED_PRECONDITION
HTTP_JSON-STATUS-001: expected ContentTypeNotSupportedError (415), got 400
```

---

## Duplicate-check note — what it did and did not cover

The kit's open and closed issues were read for "specification", "409", "error
mapping" and "1.0.1". The nearest neighbours are
[#201](https://github.com/a2aproject/a2a-tck/issues/201) (the vendored spec
declares `GET` for `/tasks/{id}:subscribe` while `SUBSCRIBE_TO_TASK_BINDING`
uses `POST` — the same vendored document treated as authoritative, a different
row) and [#216](https://github.com/a2aproject/a2a-tck/issues/216) (a different
complaint about `test_content_type_not_supported_error`). Neither covers the
snapshot being superseded.

**The limitation, stated because the search was not the exhaustive one this
repository's process asks for:** it was performed by reading the rendered issue
list, not the API, because the session that prepared this had GitHub API access
scoped to this repository only. Titles were read; bodies were not full-text
searched. A duplicate whose title does not mention any of those four terms
would have been missed.

## What was deliberately not claimed

The official Python SDK's behaviour. `docs/official-tck-findings.md` §20
records `a2a-sdk` 1.1.2 answering `400` for `TaskNotCancelableError`, which
would corroborate that a reference implementation already follows v1.0.1 — but
that was measured in an earlier session and was not re-measured for this
report, so it was left out rather than asserted second-hand about a third
party's software.

## Corrections to the submitted text

Found after filing, recorded here rather than edited into the body above.

* **`§6` should be `§3.6`.** The clause quoted in the caveat — patch numbers
  "do not affect protocol compatibility", and "MUST not be considered when
  clients and servers negotiate protocol versions" — is A2A §3.6,
  *Versioning*, in both `v1.0.0` and `main`. §6 is *Common Workflows &
  Examples*. The quotation itself is verbatim and correct, and nothing in the
  report's argument turns on the number, so this has not been raised as a
  comment on the issue; it would add noise to a thread awaiting a first
  maintainer response. Worth a correcting comment if the caveat is ever
  discussed there.
