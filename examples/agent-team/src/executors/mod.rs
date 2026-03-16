// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Agent executor implementations.
//!
//! Each module defines an [`AgentExecutor`] implementation for one of the
//! four agents in the team demo.

mod build_monitor;
mod code_analyzer;
mod coordinator;
mod health_monitor;

pub use build_monitor::BuildMonitorExecutor;
pub use code_analyzer::CodeAnalyzerExecutor;
pub use coordinator::CoordinatorExecutor;
pub use health_monitor::HealthMonitorExecutor;
