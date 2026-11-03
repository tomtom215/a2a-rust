// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A delegation resumes a stream the network cut before it gives up on the
//! child, and a supervisor can pick a child up again (audit H4, ADR 0030).
//!
//! The server is this repository's, over real sockets. A proxy sits in front
//! of it that can sever every open connection (`cut`) and refuse new ones
//! (`block`), which is what an L4 reset or a rollout does to a long-lived
//! SSE stream. Each test asserts what became of the child on the server, not
//! only what the delegation reported.

mod harness;
mod options;
mod reattach;
mod resume;
