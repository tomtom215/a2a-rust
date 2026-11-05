// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `HttpPool` (ADR 0032, audit D-12): clients that share one set of HTTP
//! connections, and what a pool does with them.
//!
//! The server is this repository's, over real sockets, behind a tap that
//! counts the connections it accepts, the HTTP/2 PINGs the client sends and
//! the connections the client closes. Each case asserts what the tap saw, not
//! only what the client returned.

#![cfg(feature = "tls-rustls")]

mod conflicts;
mod connect;
mod harness;
mod idle;
mod keep_alive;
mod lifetime;
mod sharing;
mod tls;
