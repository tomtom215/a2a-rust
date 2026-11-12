// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The bounded label values the [`Metrics`](crate::metrics::Metrics) hooks report.

/// How an executor's run ended, as [`Metrics::on_executor_run`](crate::metrics::Metrics::on_executor_run) reports it.
///
/// A bounded set, so a dashboard can list every value.
pub mod executor_outcome {
    /// The executor returned `Ok`.
    pub const COMPLETED: &str = "completed";
    /// It returned an error that is not a spent budget.
    pub const ERROR: &str = "error";
    /// A budget ran out: the executor timeout passed (the future was
    /// dropped), or the caller's deadline or tokens did.
    pub const BUDGET_EXHAUSTED: &str = "budget_exhausted";
    /// The task was canceled or halted, or the server was shutting down,
    /// when the executor returned.
    pub const CANCELED: &str = "canceled";
    /// The future ended without returning a result: the executor panicked,
    /// or the run was dropped.
    pub const ABORTED: &str = "aborted";
}

/// Operation labels passed to [`Metrics::on_persistence_error`](crate::metrics::Metrics::on_persistence_error).
///
/// Named constants rather than string literals at the call sites, so the set
/// stays bounded and greppable — an operator building a dashboard needs to know
/// every value this can take, and a typo at one call site would otherwise
/// create a silent second series.
pub mod persistence_operation {
    /// Persisting a task status transition.
    pub const STATUS_UPDATE: &str = "status_update";
    /// Persisting parts appended to an existing artifact.
    pub const ARTIFACT_APPEND: &str = "artifact_append";
    /// Persisting a newly added artifact.
    pub const ARTIFACT_PUSH: &str = "artifact_push";
    /// Appending one event to a task's event log.
    pub const EVENT_APPEND: &str = "event_append";
    /// Persisting a whole-task snapshot event.
    pub const TASK_SNAPSHOT: &str = "task_snapshot";
    /// Persisting the failed state after an invalid transition was rejected.
    pub const FAILED_STATE: &str = "failed_state";
    /// Persisting an agent message appended to the task's history.
    pub const HISTORY_APPEND: &str = "history_append";
    /// Waiting for a task's event log to catch up with what was already
    /// broadcast, on a resubscribe that asked to resume.
    ///
    /// Reported when the wait bounded by
    /// [`HandlerLimits::subscribe_replay_catchup`](crate::handler::HandlerLimits::subscribe_replay_catchup)
    /// expired with the log still short of the writer's position. The replay
    /// was served with what the log held, so the subscriber is missing the
    /// shortfall — this is the only signal that it happened.
    pub const EVENT_LOG_CATCHUP: &str = "event_log_catchup";
    /// Handing an event from the executor's queue to the persistence processor.
    ///
    /// Reported with `error_kind = "channel_closed"` when the processor is
    /// gone and the event was dropped: the stream still serves live
    /// subscribers, but the task's stored state stops advancing, and until
    /// 0.12 a default build said nothing about it.
    pub const QUEUE_HANDOFF: &str = "queue_handoff";
    /// One PostgreSQL statement, run again after its connection was lost
    /// (`store::pg_retry`); reported only through
    /// [`Metrics::on_store_retry`](crate::metrics::Metrics::on_store_retry).
    pub const STATEMENT: &str = "statement";
    /// Reading a task's webhook list to deliver an event.
    ///
    /// Reported through [`Metrics::on_store_retry`](crate::metrics::Metrics::on_store_retry)
    /// while the read is retried; when it gives up, the event's push is
    /// reported as a `failed` delivery (`push_outcome::FAILED`).
    pub const PUSH_CONFIG_LIST: &str = "push_config_list";
    /// Writing a record to the audit log (the `audit` feature).
    ///
    /// A call or event whose record failed was served or processed unrecorded; the
    /// gap shows in the chain only as a missing record, so this is how it
    /// is noticed.
    pub const AUDIT_APPEND: &str = "audit_append";
}

/// Error kinds passed to [`Metrics::on_persistence_error`](crate::metrics::Metrics::on_persistence_error) for
/// [`persistence_operation::EVENT_LOG_CATCHUP`].
pub mod event_log_catchup_error {
    /// The bounded wait expired with the log still behind the position the
    /// writer had already broadcast. The replay was served short.
    pub const TIMED_OUT: &str = "timed_out";
}

/// Error kinds passed to [`Metrics::on_persistence_error`](crate::metrics::Metrics::on_persistence_error) for
/// [`persistence_operation::EVENT_APPEND`].
///
/// # Why an append that raised nothing is reported here
///
/// An append is `INSERT ... ON CONFLICT (task_id, seq) DO NOTHING`, because
/// `seq` is a *position*: writing one twice must leave one row. That is the
/// safety property, and it is also a way to lose an event without an error.
/// Two replicas each running their own in-process event queue number one
/// task's events from the same place — nothing here hands out a
/// database-backed lease — so replica B's event *n* lands on the position
/// replica A already wrote, and the row that stays is A's. Until these labels
/// existed the store returned `Ok(())` and nothing anywhere said so.
///
/// The first two kinds mean **an event the agent emitted is not in the log**,
/// which is what [`Metrics::on_persistence_error`](crate::metrics::Metrics::on_persistence_error) is for; the third,
/// [`HOLD_CEILING`](event_append_error::HOLD_CEILING), that a stream lost its
/// copy of one the log held. A retried append of the
/// *same* event is not reported here: see
/// [`POSITION_CONFLICT`](event_append_error::POSITION_CONFLICT).
pub mod event_append_error {
    /// The position was already held, and the store could not establish that
    /// the row already there is this same event.
    ///
    /// On a collision the store reads the stored payload back and compares it
    /// with what it was asked to write. Identical bytes mean a replay of the
    /// event already recorded, which loses nothing and is **not** counted.
    /// This label covers the rest: the stored event is a different one, or the
    /// comparison could not be made (the read-back failed, or the event would
    /// not serialize). It errs towards reporting.
    ///
    /// A non-zero rate is the shape the design has no other defence against:
    /// two writers numbering one task's log. Look for a second replica
    /// serving the same task id.
    pub const POSITION_CONFLICT: &str = "position_conflict";

    /// The store held no such task, so there was nowhere to put the event.
    ///
    /// Only the two in-memory stores raise this: they keep the log on the task
    /// entry, so eviction takes the log with it and the event processor can
    /// outlive the sweep that removed the task it is still writing for. The
    /// append stays a success on purpose — failing it would turn a swept task
    /// into a failed agent — and this is the report that it happened.
    pub const TASK_ABSENT: &str = "task_absent";

    /// The append succeeded, and the log, at its held-events ceiling, dropped
    /// its oldest event though an attached stream still owed it.
    ///
    /// Unlike the two above, the event was in the log; what is lost is one
    /// stream's copy of it. Only the two in-memory stores raise this (since
    /// 2026-10-20): an attached stream holds the events it owes past
    /// [`max_events_per_task`](crate::store::TaskStoreConfig::max_events_per_task),
    /// up to [`max_held_events_factor`](crate::store::TaskStoreConfig::max_held_events_factor)
    /// times it. That stream ends with `streamLagged` after what it
    /// delivered. A non-zero rate is a client reading far slower than its
    /// task writes, or one that stopped reading and kept its connection.
    pub const HOLD_CEILING: &str = "hold_ceiling";
}

/// Error kinds passed to [`Metrics::on_persistence_error`](crate::metrics::Metrics::on_persistence_error) for
/// [`persistence_operation::QUEUE_HANDOFF`].
pub mod queue_handoff_error {
    /// The persistence channel's receiver was dropped; the event was not
    /// persisted.
    pub const CHANNEL_CLOSED: &str = "channel_closed";
}

/// Why a `SubscribeToTask` stream stopped waiting on a task's event log, as
/// [`Metrics::on_subscribe_wake`](crate::metrics::Metrics::on_subscribe_wake)
/// reports it (audit E-F25 (b), ADR 0057).
pub mod subscribe_wake {
    /// The store said an event was appended to the task's log
    /// ([`LogWake::Notified`](crate::store::LogWake::Notified)).
    pub const NOTIFICATION: &str = "notification";
    /// The store lost the source of its notifications and has it again, so it
    /// woke every stream to look
    /// ([`LogWake::Resynced`](crate::store::LogWake::Resynced)). A rising
    /// count is a database connection that keeps dropping.
    pub const RESYNC: &str = "resync";
    /// Nothing woke the stream before its timer ran out.
    ///
    /// On a store with notifications, a wait nothing was appended in, or a
    /// notification that never arrived. (A store without them reports no wake
    /// at all.)
    pub const TIMEOUT: &str = "timeout";
}

/// The control plane's label values (audit H11; ADR 0038 section 4).
///
/// Each set is bounded, so a dashboard can list every value; no task, root,
/// job or message id is ever one (ADR 0034 section 4).
pub mod control {
    /// Why a request was refused before any work, as
    /// [`Metrics::on_refusal`](crate::metrics::Metrics::on_refusal) reports it.
    pub mod refusal {
        /// The server holds its maximum of concurrent streams (event queues).
        pub const STREAM_CAP: &str = "stream_cap";
        /// The tenant has its maximum of tasks in flight.
        pub const TENANT_CAP: &str = "tenant_cap";
        /// The server holds its maximum of push notification configs.
        pub const PUSH_CONFIG_CAP: &str = "push_config_cap";
        /// The idempotency store could not take the key now.
        pub const IDEMPOTENCY_STORE: &str = "idempotency_store";
        /// The task's event log could not be read to number the turn.
        pub const EVENT_LOG: &str = "event_log";
        /// The halt store could not be read.
        pub const HALT_STORE: &str = "halt_store";
    }

    /// Task lease events, as
    /// [`Metrics::on_lease`](crate::metrics::Metrics::on_lease) reports them.
    pub mod lease {
        /// A lease ran out with its task running; the task was stopped.
        pub const LAPSED: &str = "lapsed";
    }

    /// What a halt covers.
    pub mod halt_scope {
        /// Every task.
        pub const ALL: &str = "all";
        /// One tenant's tasks.
        pub const TENANT: &str = "tenant";
        /// One task tree, from its root (a job; ADR 0034). The root is not a
        /// label: it is unbounded.
        pub const ROOT: &str = "root";
    }

    /// What was applied.
    pub mod halt_action {
        /// A halt.
        pub const HALT: &str = "halt";
        /// A resume.
        pub const RESUME: &str = "resume";
    }

    /// How a steering message ended, as
    /// [`Metrics::on_steering`](crate::metrics::Metrics::on_steering) reports
    /// it (ADR 0036). Every value is one message.
    pub mod steering {
        /// Put in the inbox of the turn running on this replica.
        pub const DELIVERED: &str = "delivered";
        /// Stored for the replica that holds the turn's claim.
        pub const HANDED_OFF: &str = "handed_off";
        /// Answered from the message's idempotency key: not delivered again.
        pub const REPLAYED: &str = "replayed";
        /// Refused: the turn's inbox holds its capacity.
        pub const FULL: &str = "full";
        /// Refused: the turn ended, or its claim lapsed, before delivery.
        pub const ENDED: &str = "ended";
        /// Refused: larger than the configured message size.
        pub const TOO_LARGE: &str = "too_large";
        /// Refused: the job (or tenant, or server) is halted.
        pub const HALTED: &str = "halted";
        /// Accepted, and still unread when the turn ended: dropped with it.
        pub const UNREAD: &str = "unread";
        /// Taken from the store into a turn's inbox on this replica, after
        /// another replica handed it off.
        pub const RECEIVED: &str = "received";
    }

    /// Turn-claim events, as
    /// [`Metrics::on_claim`](crate::metrics::Metrics::on_claim) reports them.
    pub mod claim {
        /// The reaper ended a task its replica left running (its claim had
        /// expired).
        pub const REAPED: &str = "reaped";
        /// The reaper completed the ending of a task that was already
        /// terminal in the store but whose replica stopped before it logged
        /// the ending and told the webhooks (its claim had expired).
        pub const FINISHED: &str = "finished";
    }
}
