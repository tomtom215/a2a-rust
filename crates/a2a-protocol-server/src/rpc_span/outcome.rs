// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! How a finished call is recorded: its status, whether it failed, and
//! whether the peer abandoned it.

use super::{CANCELLED, RpcSystem};

/// gRPC's name for the status of a call the client cancelled.
const GRPC_CANCELLED: &str = "CANCELLED";

/// A finished call's `rpc.status_code`, its `error.type`, and whether the
/// peer abandoned it.
pub(super) struct Outcome<'a> {
    pub(super) status_code: Option<&'a str>,
    pub(super) failed_with: Option<&'a str>,
    pub(super) abandoned: bool,
}

/// Classifies a call on `system` that ended with `status` (`None` for
/// success; [`CANCELLED`] for a call dropped before it finished).
///
/// A call the peer abandoned ([`CANCELLED`]) is not the server failing,
/// and its span is never marked `Error` (audit C-09; ADR 0038 §2). On
/// gRPC it records the status `CANCELLED` and no `error.type`, as
/// `docs/rpc/grpc.md` has a server span do ("The following status codes
/// SHOULD be considered errors: `UNKNOWN`, `DEADLINE_EXCEEDED`, …";
/// `CANCELLED` is not among them). The other bindings have no status for
/// a call that never answered — none is recorded (until 2026-11-04 it was
/// `cancelled`, which is no binding's status) — so `error.type` is `cancelled`, the
/// component-specific identifier `rpc-spans.md` asks for ("If the RPC
/// fails with an error before status code is returned"), which the
/// shipped error ratios leave out; the span status stays unset, as
/// `http-spans.md` has it for a request "cancelled intentionally by the
/// caller".
pub(super) fn classify(system: RpcSystem, status: Option<&str>) -> Outcome<'_> {
    let abandoned = status == Some(CANCELLED);
    // gRPC always has a status; the other bindings have one only on error.
    let status_code = match (status, system) {
        (Some(CANCELLED), RpcSystem::Grpc) => Some(GRPC_CANCELLED),
        // No status was sent; `cancelled` is not one of the binding's.
        (Some(CANCELLED), _) => None,
        (Some(code), _) => Some(code),
        (None, RpcSystem::Grpc) => Some("OK"),
        (None, RpcSystem::Custom(binding)) => binding.ok_status,
        (None, _) => None,
    };
    let failed_with = match system {
        RpcSystem::Grpc if abandoned => None,
        _ if abandoned => Some(CANCELLED),
        system => status.filter(|s| system.is_server_error(s)),
    };
    Outcome {
        status_code,
        failed_with,
        abandoned,
    }
}
