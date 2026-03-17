// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Task lifecycle methods: get, list, cancel, resubscribe, extended agent card.
//!
//! # Module structure
//!
//! | Module | Handler method |
//! |---|---|
//! | [`get_task`] | `on_get_task` — retrieve a single task |
//! | [`list_tasks`] | `on_list_tasks` — paginated task listing |
//! | [`cancel_task`] | `on_cancel_task` — cancel an in-flight task |
//! | [`subscribe`] | `on_resubscribe` — resubscribe to a task event stream |
//! | [`extended_card`] | `on_get_extended_agent_card` — return the full agent card |

mod cancel_task;
mod extended_card;
mod get_task;
mod list_tasks;
mod subscribe;
