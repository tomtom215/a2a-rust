// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Task storage backend.

pub mod task_store;

pub use task_store::{InMemoryTaskStore, TaskStore, TaskStoreConfig};
