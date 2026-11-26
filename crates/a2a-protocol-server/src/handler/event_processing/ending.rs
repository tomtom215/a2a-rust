// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Telling a task's webhooks and its log that it ended, from a path with no
//! background processor to do it: the claim reaper, and a `CancelTask` of a
//! task no turn is running (COR-02, register E-F25 and X-11).

use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::task::TaskId;

use crate::handler::PushLanes;
use crate::handler::limits::HandlerLimits;
use crate::metrics::Metrics;
use crate::push::{PushConfigStore, PushSender};
use crate::store::TaskStore;

/// Where an ending is announced.
#[derive(Clone, Copy)]
pub struct EndingSinks<'a> {
    pub task_store: &'a dyn TaskStore,
    pub push_config_store: &'a dyn PushConfigStore,
    pub push_sender: Option<&'a dyn PushSender>,
    pub push_lanes: &'a PushLanes,
    pub limits: &'a HandlerLimits,
    pub metrics: &'a dyn Metrics,
}

/// Records `event`, a stored task's ending, in the outbox, appends it to the
/// task's log, and delivers it to the webhooks when there is no outbox.
///
/// Recorded in the outbox before it is logged, when there is one: a stop in
/// between leaves the log without the ending, which the reaper finishes,
/// rather than a log that has it and webhooks that were never told. So a
/// recording that is not durable (the configs unread, the outbox refusing) is
/// not followed by the log's append (adversarial review N06). Whether the
/// ending is now durably told.
pub async fn announce_ending(sinks: EndingSinks<'_>, id: &TaskId, event: &StreamResponse) -> bool {
    let recorded = sinks.push_lanes.record_now(id, event).await;
    if recorded.is_some_and(|r| !r.is_durable()) {
        return false;
    }
    if !super::append_ending(
        sinks.task_store,
        id,
        event,
        sinks.metrics,
        sinks.limits.store_write_retry,
    )
    .await
    {
        return false;
    }
    if recorded.is_none() {
        super::deliver_push_bg(
            id,
            event,
            sinks.push_config_store,
            sinks.push_sender,
            sinks.limits,
            sinks.metrics,
        )
        .await;
    }
    true
}
