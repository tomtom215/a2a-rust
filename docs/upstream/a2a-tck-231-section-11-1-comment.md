<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Comment for `a2aproject/a2a-tck#231` — the stale copy also fixes §11.1's media type as a MUST

**Status: DRAFT, not sent** (2026-10-23). A comment on the existing issue
(filed 2026-09-01, open, no maintainer response when checked on 2026-10-23),
not a new issue: the cause is the same pin. The maintainer posts it.

Verified against a fresh clone of `a2a-tck` at `d4d6fae` (`main`,
2026-10-22) on 2026-10-23, and against the A2A specification at tag `v1.0.1`
(`3303592`, vendored in this repository as
`docs/implementation/a2a-v1.0.1-specification.md`). The background is
`docs/official-tck-findings.md` §23.

---

## Comment

A second consequence of the v1.0.0 pin, on a different section: §11.1's media
type.

The vendored `specification/specification.md` (still v1.0.0 per
`specification/version.json` at `d4d6fae`) has, in §11.1:

> - **Content-Type:** `application/json` for requests and responses

A2A v1.0.1 changed that line to:

> - **Content-Type:** application/a2a+json **SHOULD** be used for requests and responses

and its §11.6 error example carries `Content-Type: application/a2a+json`.

The kit turns the v1.0.0 text into two MUSTs in
`tck/requirements/binding_http_json.py`:

- `HTTP_JSON-SVC-001` (line 59): "HTTP+JSON/REST binding MUST use
  Content-Type application/json for requests and responses."
- `HTTP_JSON-ERR-001` (line 98): "HTTP error responses MUST use the AIP-193
  representation with Content-Type application/json …"

and checks both with a substring test that `application/a2a+json` cannot
pass:

- `tests/compatibility/core_operations/test_transport_behavior.py` line 259:
  `if "application/json" not in ct:`
- `tests/compatibility/http_json/test_problem_details.py` line 101:
  `valid = "application/json" in ct.lower()`

So an HTTP+JSON server that follows v1.0.1's SHOULD and answers
`application/a2a+json` to a request that states no preference fails two
MUST-level checks. a2a-rust has done so since 2026-10-15 and baselines both
(`tck/conformance-baseline.json`).

Suggested, alongside the §5.4 refresh this issue proposes: after `make spec`
moves to v1.0.1, regrade §11.1 as a SHOULD for `application/a2a+json`, and
have both checks accept either `application/json` or `application/a2a+json`
(parsing the media type rather than matching a substring), since v1.0.1 still
permits `application/json` for a client that asks for it.
