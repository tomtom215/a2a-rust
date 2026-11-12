// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The client/server boundary, through real sockets: what the client puts on
//! the wire and what the server does with it (adversarial review P1-P4, 2026-11-12).
//!
//! Each helper that is correct alone passed its own tests while the composition
//! failed (a trailing slash, a tenant named like a route, a binding override
//! after version filtering), so these drive a real client at a real server
//! through a recording proxy and assert on the **request target that crossed
//! the wire**, not on a string helper.

mod bearer;
mod common;
mod extended_card;
mod tenants;
mod urls;
