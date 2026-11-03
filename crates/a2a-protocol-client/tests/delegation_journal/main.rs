// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The delegation journal (ADR 0030): the record is written before the child
//! exists, follows it, and is settled however the delegation ends; a
//! restarted supervisor picks its open children up from the records.
//!
//! The server is this repository's, over real sockets.

mod hand_off;
mod harness;
mod lifecycle;
mod progress;
mod records;
