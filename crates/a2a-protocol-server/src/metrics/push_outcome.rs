// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Outcome labels passed to [`Metrics::on_push_delivery`](super::Metrics::on_push_delivery).
//!
//! In a file of its own since 0.16.0, when `throttled` and `expired` took
//! `metrics/mod.rs` past 500 lines.

/// The webhook accepted the delivery.
pub const DELIVERED: &str = "delivered";
/// The webhook was reached and refused it, or the sender errored.
///
/// Also reported, once for the event and not once per config, when the
/// task's webhook list could not be read (after the retry that a task
/// write gets, `persistence_operation::PUSH_CONFIG_LIST`), so none of its
/// webhooks was tried. Until 2026-10-31 that case reported nothing at all
/// (audit D-04).
pub const FAILED: &str = "failed";
/// The delivery did not complete within the configured timeout.
pub const TIMEOUT: &str = "timeout";
/// The delivery was cut short by `push_delivery_timeout` while the sender's
/// own schedule still had attempts left.
///
/// Distinct from [`TIMEOUT`], which is a webhook that did not answer inside
/// the time it was given. This one is a *configuration* result: the sender
/// reports (via `PushSender::max_delivery_duration`) that it wanted longer
/// than the handler allows, so retries it advertises can never run in full.
/// At the shipped defaults that can only be the last attempt — a 38-second
/// schedule against a 30-second bound, in which every attempt starts and
/// the third has 2 seconds when the first two run to their timeouts. It
/// is worth its own label because the fix is a config change, not a
/// webhook investigation.
pub const TIMEOUT_TRUNCATED: &str = "timeout_truncated";
/// A delivery dropped before it started: its task's push lane was full.
///
/// The lane (`HandlerLimits::push_queue_capacity`) fills when the webhook
/// is slower than the agent or not answering. Counted once per dropped
/// delivery (one event, or one blocking send's batch), not per config.
/// A terminal or interrupted status is never dropped.
pub const DROPPED: &str = "dropped";
/// A delivery not attempted, because its webhook is suspended.
///
/// A webhook is suspended after
/// [`HandlerLimits::push_suspend_after`](crate::handler::HandlerLimits::push_suspend_after)
/// failed deliveries in a row, for
/// [`push_suspend_for`](crate::handler::HandlerLimits::push_suspend_for) and
/// longer after each failed probe. Counted once per skipped delivery, one
/// per config. A delivery that ends a turn is attempted anyway, and
/// counted as whatever it came to; a success ends the suspension (audit
/// D-05).
pub const SUSPENDED: &str = "suspended";
/// The webhook answered `410 Gone` and its config was removed.
///
/// See [`HandlerLimits::push_remove_gone`](crate::handler::HandlerLimits::push_remove_gone).
/// Counted for each delivery that learned it; a blocking send's batch
/// reads its configs once, so its later events are counted too, and the
/// config takes no delivery after that (audit D-05).
pub const GONE: &str = "gone";
/// A delivery not attempted: it waited for a slot in the push gate for
/// all of `push_delivery_timeout`.
///
/// The gate is `HandlerLimits::push_max_in_flight`, `_per_partition` and
/// `_per_host`. No request was made, so the webhook is not counted as failing
/// (audit NEW-f-2). Until 0.16.0 the wait counted against the delivery's
/// timeout and such a delivery was reported as [`TIMEOUT`].
pub const THROTTLED: &str = "throttled";
/// A delivery dropped by the push outbox because it was older than
/// `HandlerLimits::push_outbox_max_age` when it was next due (ADR 0037).
///
/// Its webhook did not take it in that time; the event is still in the
/// task's log, for a receiver to read after the gap.
pub const EXPIRED: &str = "expired";
/// The outbox worker settled a push reservation that a replica left behind:
/// it reserved an event's rows and stopped before arming or releasing them,
/// and the task store said whether the event was stored (ADR 0037, audit
/// H14). Not a delivery: a count of crashes (or stalls past the hold) the
/// outbox recovered from, so an alert can tell that replicas are dying
/// mid-event.
pub const RESERVATION_SETTLED: &str = "reservation_settled";
/// No longer reported.
///
/// It counted configs the per-event delivery budget never reached; since
/// 2026-10-11 every config is attempted (spec §4.3.3) and the budget sets
/// concurrency instead, so nothing is skipped. Kept so code matching on
/// the label still compiles; removing a public constant is a semver break
/// (see RELEASING.md).
pub const SKIPPED: &str = "skipped";
