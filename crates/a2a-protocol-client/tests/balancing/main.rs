// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Client-side balancing, ejection and hedging (ADR 0050, audit H12), and the
//! per-origin state of an `HttpPool` (ADR 0032 decision 5).
//!
//! Every case runs against real replicas on local sockets that count what
//! they receive, and asserts those counts, not only what the client returned.

mod distribution;
mod ejection;
mod harness;
mod hedging;
#[cfg(feature = "tls-rustls")]
mod pinning;
#[cfg(feature = "tls-rustls")]
mod pool;
#[cfg(feature = "tls-rustls")]
mod premise;
