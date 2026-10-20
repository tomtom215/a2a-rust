// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Spec §5.1, functional equivalence across bindings: "When an agent
//! supports multiple protocols, all supported protocols MUST":
//!
//! | TCK ID | requirement | here |
//! |---|---|---|
//! | `BIND-EQUIV-001` | provide the same set of operations and capabilities | `functionality.rs` |
//! | `BIND-EQUIV-002` | return semantically equivalent results for the same requests | `results.rs` |
//! | `BIND-EQUIV-003` | map errors consistently using appropriate protocol-specific codes | `errors.rs` |
//! | `BIND-EQUIV-004` | support the same authentication schemes declared in the `AgentCard` | `auth.rs` |
//!
//! The official TCK has no test for any of the four (its backlog `task-28`;
//! `docs/official-tck-findings.md` §16). Here one `RequestHandler` is served
//! on JSON-RPC, HTTP+JSON, gRPC and WebSocket at once and driven through
//! this SDK's client on each, so a difference between bindings is a
//! difference in the bindings and nothing else. `tck/untested-musts.toml`
//! maps each requirement to its test.
//!
//! The same harness carries the version requirements the suite leaves
//! untested (`version.rs`: `VER-CLIENT-001`, `VER-CLIENT-002`,
//! `VER-SERVER-001`) and `CARD-EXT-002`'s wire evidence (`errors.rs`).

#![cfg(all(feature = "grpc", feature = "websocket"))]

mod auth;
mod errors;
mod functionality;
mod harness;
mod results;
mod version;
