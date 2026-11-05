<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Issue draft for `a2aproject/A2A`: `ErrorInfo` reasons for the standard errors

**Status: draft, not sent.** Written 2026-11-05 against A2A v1.0.1,
a2a-go v2.7.0 and a2a-python (`a2a-sdk`) 1.2.3, read from installed sources.
Sending it is the maintainer's decision. **Updated 2026-11-05:** this SDK now
sends the reasons both SDKs use on HTTP+JSON (ADR 0049, second amendment), so
the interop gap described below is closed on our side; the request to the
specification stands, since a server that follows v1.0.1 to the letter is
still misread.

---

Title: The spec defines `ErrorInfo` reasons only for A2A-specific errors, but
both reference SDKs need one for invalid params too

**What the spec says.** §11.6 (HTTP+JSON): implementations "**MUST** include a
`google.rpc.ErrorInfo` object in the `details` array for A2A-specific errors",
with `reason` "the A2A error type in UPPER_SNAKE_CASE without the 'Error'
suffix" and `domain` `a2a-protocol.org`; for validation errors they "**SHOULD**
use well-known types such as `google.rpc.BadRequest`". §10.6 says the same for
gRPC. The A2A-specific errors are the nine of §3.3.2 (`TaskNotFoundError` ...
`VersionNotSupportedError`). Invalid params, invalid request, method not found,
parse error and internal error are not among them, so no reason is defined for
them.

**What the reference SDKs do.**

* a2a-go v2.7.0 `a2a/errors.go` names reasons for them (`PARSE_ERROR`,
  `INVALID_REQUEST`, `METHOD_NOT_FOUND`, `INVALID_PARAMS`, `INTERNAL_ERROR`),
  and its REST client (`internal/rest/rest.go`, `ConvertErrorBody`) maps an
  error body by its `a2a-protocol.org` `ErrorInfo` reason; with none, by HTTP
  status only for 401, 403, 404, 405 and 501, and otherwise to
  `ErrInternalError`. A `400 INVALID_ARGUMENT` body without that `ErrorInfo`
  therefore reads as an internal error.
* a2a-python 1.2.3 `a2a/utils/errors.py` maps `InvalidParamsError` to
  `(400, 'INVALID_ARGUMENT', 'INVALID_PARAMS')`, and its server always sends a
  leading `ErrorInfo` with that reason (`utils/error_handlers.py`,
  `build_error_details`). Its REST client maps only a known reason; otherwise it
  raises `A2AClientError('HTTP Error 400: ...')` without the server's message.

**What follows.** A server that does what the spec requires and no more (ours
did until 2026-11-05, for invalid params: `400`, status `INVALID_ARGUMENT`, a message, and a
`BadRequest` detail where it has field violations) is read by a2a-go as an
internal error and by a2a-python as a bare HTTP 400. We found this from the
interop runs in `itk/interop/` (a lineage past its depth cap, a lease out of
range).

**Proposal.** Either define the reasons the SDKs already use for the standard
errors in §5.4's table and require them in §10.6 and §11.6 (the smaller change,
matching both SDKs), or say in §11.6 that a client maps a body without an A2A
`ErrorInfo` by its `status` (`INVALID_ARGUMENT` is invalid params), as AIP-193
makes `status` the canonical code.
