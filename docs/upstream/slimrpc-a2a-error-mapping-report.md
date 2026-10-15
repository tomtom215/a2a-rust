<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Issue draft — SLIMRPC's error mapping and A2A §5.4's gRPC column disagree

**Status: DRAFT, held — not filed.** Written and re-verified 2026-10-15 against A2A v1.0.1 (`docs/implementation/a2a-v1.0.1-specification.md`; §5.4 unchanged on A2A `main` 5eced49) and the SLIMRPC specification (`spec/slimrpc_v1/slimrpc.md` §6.1, byte-identical to `a2aproject/experimental-cpb-slimrpc` `main` 1328426, `spec/v1/slimrpc.md`). For `a2aproject/experimental-cpb-slimrpc`, or `a2aproject/A2A`; the maintainer decides. No existing issue in either repository reports it. Audit register, the §12 / §5.4 row.

---

Title: SLIMRPC's error mapping and A2A §5.4's gRPC column disagree for three A2A errors

A2A v1.0.1 §12 requires custom bindings to "comply with all requirements
defined in Section 5", and §5.4 requires custom bindings to "define
equivalent error code mappings that preserve the semantic meaning of each
A2A error type". §5.4's gRPC column maps PushNotificationNotSupportedError
and UnsupportedOperationError to FAILED_PRECONDITION, and
ContentTypeNotSupportedError to INVALID_ARGUMENT.

The SLIMRPC specification's §6.1 requires every A2A error to be mapped (a
MUST) and gives a "canonical mapping" that maps all three to UNIMPLEMENTED.
For PushNotificationNotSupportedError and UnsupportedOperationError that
matches A2A v1.0.0's gRPC column, which SLIMRPC links to; A2A #1627 changed
both to FAILED_PRECONDITION in v1.0.1. ContentTypeNotSupportedError was
INVALID_ARGUMENT in both A2A versions. An implementation that follows the SLIMRPC text exactly
therefore differs from the gRPC column, and one that follows the gRPC
column breaks SLIMRPC §6.1.

a2a-rust follows SLIMRPC §6.1 for the errors it lists, and A2A §5.4's gRPC
column for the three it does not, and reads §5.4 as asking for an equivalent
mapping that preserves meaning, not for the gRPC column itself. Could the
two specifications state which governs, or align the three codes?

Also noted: the SLIMRPC mapping table has no row for
ExtendedAgentCardNotConfiguredError, ExtensionSupportRequiredError or
VersionNotSupportedError (-32007, -32008, -32009), against §5.4's SHOULD
that a binding provide a mapping table for each A2A error type, §12.4's MUST
to "provide mappings for all A2A-specific error types", and SLIMRPC §6.1's own
MUST that all A2A-specific errors be mapped. All three errors already existed
in A2A v1.0.0.

Also noted: SLIMRPC §6.2 gives errors optional structured `details`, and A2A
§12.4 requires that "error details are accessible to clients", but
`agntcy-slim-rpc` sends only the code and message: `send_error_for_rpc`
(2.3.x) and `send_error_to` (3.4.1, the latest release) publish
`error.message()` with code and rpc_id metadata and drop `RpcError::details`.
Errors such as ExtensionSupportRequiredError therefore cannot carry their
`ErrorInfo` over SLIMRPC.
