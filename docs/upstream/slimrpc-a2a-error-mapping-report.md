<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Issue draft — SLIMRPC's error mapping and A2A §5.4's gRPC column disagree

**Status: DRAFT, not sent.** Written 2026-10-15 against A2A v1.0.1 (`docs/implementation/a2a-v1.0.1-specification.md`) and the vendored SLIMRPC specification (`spec/slimrpc_v1/slimrpc.md`, §6.1). For whichever repository owns the SLIMRPC specification, or `a2aproject/A2A`; the maintainer decides. Audit register, the §12 / §5.4 row.

---

Title: SLIMRPC's error mapping and A2A §5.4's gRPC column disagree for three A2A errors

A2A v1.0.1 §12 requires custom bindings to "comply with all requirements
defined in Section 5", and §5.4 requires custom bindings to "define
equivalent error code mappings that preserve the semantic meaning of each
A2A error type". §5.4's gRPC column maps PushNotificationNotSupportedError
and UnsupportedOperationError to FAILED_PRECONDITION, and
ContentTypeNotSupportedError to INVALID_ARGUMENT.

The SLIMRPC specification's §6.1 canonical mapping (a MUST) maps all three
to UNIMPLEMENTED. An implementation that follows the SLIMRPC text exactly
therefore differs from the gRPC column, and one that follows the gRPC
column breaks SLIMRPC §6.1.

a2a-rust follows SLIMRPC §6.1 and reads §5.4 as asking for an equivalent
mapping that preserves meaning, not for the gRPC column itself. Could the
two specifications state which governs, or align the three codes?

Also noted: the SLIMRPC mapping table has no row for
ExtendedAgentCardNotConfiguredError, ExtensionSupportRequiredError or
VersionNotSupportedError (-32007, -32008, -32009), against §5.4's SHOULD
that a binding provide a mapping table for each A2A error type.
