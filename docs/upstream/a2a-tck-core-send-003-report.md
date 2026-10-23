<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Issue for `a2aproject/a2a-tck` — `CORE-SEND-003` passes only when the agent accepts an unsupported media type

**Status: DRAFT, not sent** (2026-10-23). The maintainer files it. Until
2026-10-23 this defect was deliberately left unreported
(`docs/official-tck-findings.md` §22); the maintainer reversed that on
2026-10-23 so that every baselined failure in `tck/conformance-baseline.json`
has an upstream report.

Verified against a fresh clone of `a2a-tck` at `d4d6fae`
(`main`, 2026-10-22) on 2026-10-23: the requirement, the harness path and the
vendored specification text quoted below are read from that clone. The
upstream form splits the body across fields; the headings below are those
fields.

---

## Title

`[Bug]: CORE-SEND-003 declares no expected_error, so it fails an agent that returns ContentTypeNotSupportedError`

## What happened?

`CORE-SEND-003` (`tck/requirements/core_operations.py`, line 99) is titled
*"SendMessage returns ContentTypeNotSupportedError for unsupported media"*,
describes the requirement as *"A Media Type provided in the request's message
parts that is not supported by the agent MUST result in
ContentTypeNotSupportedError"*, and sends one part with
`mediaType: "application/x-unsupported-tck-type"`. Its `RequirementSpec` sets
`expected_behavior="ContentTypeNotSupportedError returned"` but not
`expected_error`.

`_validate_response` (`tests/compatibility/core_operations/test_requirements.py`,
line 90) checks for an error only when `requirement.expected_error is not
None`. Otherwise, for a unary call, it appends `Operation failed: …` whenever
`response.success` is false (lines 111-114). So the check passes only if the
agent **accepts** the unsupported part, and fails an agent that does what the
requirement's own title and description say.

## What did you expect to happen?

An agent whose card does not declare the part's media type (for example
`defaultInputModes: ["text/plain"]` and no skill-level `inputModes`) answers
`ContentTypeNotSupportedError` — §3.1.1 lists it for exactly this case, and
§5.4 maps it to JSON-RPC `-32005`, gRPC `INVALID_ARGUMENT` and HTTP `400` —
and `CORE-SEND-003` passes on every binding.

## Proposed fix

Set `expected_error` on the requirement to the `ErrorBinding` for
`ContentTypeNotSupportedError`, as the other error requirements in the file
do. Whether the SUT's card can be relied on not to declare
`application/x-unsupported-tck-type` is worth a precondition, since a card
that declares `*/*` or that type would legitimately accept it.

## Evidence

- An SDK that enforces `defaultInputModes` / `inputModes` (a2a-rust since
  0.14.0) fails the check on JSON-RPC, gRPC and HTTP+JSON, each time because
  it refused the part (HTTP `400` in the output below; JSON-RPC `-32005` per
  the SDK's own tests, which the log does not show); before it
  enforced them, it accepted the part and passed. Job log:
  <https://github.com/tomtom215/a2a-rust/actions/runs/36225060630/job/108357365124>
  (2026-09-26, `a2a-tck@263b9cf`).
- The code path is unchanged at `d4d6fae`.

## Relevant log output

Copied from the job log above (minimal profile, `a2a-tck@263b9cf`); the
`Operation failed:` text is the agent's own error message for the refusal:

```
✗ CORE-SEND-003 (grpc, jsonrpc, http_json): Operation failed: message part 0 has media type "application/x-unsupported-tck-type", which the agent card does not declare (defaultInputModes or a skill's inputModes)
E   AssertionError: CORE-SEND-003 [SendMessage returns ContentTypeNotSupportedError for unsupported media] failed on http_json: Operation failed: [400] message part 0 has media type "application/x-unsupported-tck-type", which the agent card does not declare (defaultInputModes or a skill's inputModes) (see specification/specification.md#311-send-message)
```
