// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A handle for the unit tests of the delegation's parts, which never
//! touches the network.

use std::sync::Arc;

use super::origin::Origin;
use super::{DEFAULT_ID_WAIT, Delegation, ResumePolicy};

/// A handle for a child named `child` whose client points at a port nothing
/// listens on, with no stream, no lease, no budget and no journal. It is not
/// armed, so dropping it cancels nothing.
pub(super) fn parked() -> Delegation {
    let client = Arc::new(
        crate::ClientBuilder::new("http://127.0.0.1:9")
            .build()
            .expect("a client"),
    );
    Delegation {
        client,
        tenant: None,
        stream: None,
        task_id: Some("child".into()),
        settled: None,
        armed: false,
        id_wait: DEFAULT_ID_WAIT,
        renewal: None,
        budget: None,
        idempotency_key: None,
        origin: Origin::current(),
        resume_policy: ResumePolicy::default(),
        last_event_id: None,
        streak: 0,
        resumed_at: None,
        opened_at: None,
        events_since_open: 0,
        journal: None,
    }
}
