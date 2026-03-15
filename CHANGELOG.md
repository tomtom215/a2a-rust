<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. -->

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial implementation of the A2A (Agent-to-Agent) v1.0.0 protocol specification.
- Core protocol type definitions and serialization.
- HTTP server with streaming (SSE) and JSON-RPC 2.0 dual transport.
- Client library for interacting with A2A-compatible agents.
- `SECURITY.md` with coordinated disclosure policy.
- `GOVERNANCE.md` with project governance and contribution guidelines.
- Health check endpoints (`GET /health`, `GET /ready`) for liveness/readiness probes.
- Request body size limits (4 MiB) to prevent DoS via oversized payloads.
- Content-Type validation on both JSON-RPC and REST dispatchers.
- Path traversal protection on the REST dispatcher.
- `TaskStoreConfig` with configurable TTL and capacity for `InMemoryTaskStore`.
- `RequestHandlerBuilder::with_task_store_config()` for store configuration.
- `ServerError::PayloadTooLarge` variant for body size limit violations.
- Executor timeout support via `RequestHandlerBuilder::with_executor_timeout()` to kill hung executors.
- Per-request HTTP timeout for `HttpPushSender` (default 30s) via `HttpPushSender::with_timeout()`.
- `TaskState::can_transition_to()` for handler-level state machine validation.
- Cursor-based pagination for `ListTasks` via `TaskStoreConfig`.
- URL percent-decoding for REST dispatcher path parameters.
- BOM (byte order mark) handling in JSON request bodies.
- Comprehensive hardening, dispatch, handler, push sender, and client test suites (379 tests).

### Changed

- **Breaking:** `AgentExecutor` trait now uses `impl Future` returns instead of
  `Pin<Box<dyn Future>>`, enabling direct `async fn` syntax in implementations.
  Requires Rust 1.75+.
- `InMemoryTaskStore` now performs TTL-based eviction of terminal tasks (default
  1 hour) and enforces a maximum capacity (default 10,000 tasks).

### Fixed

- Invalid state transitions (e.g. Submitted → Completed) are now rejected with `InvalidStateTransition` error.
- Push notification delivery now properly times out instead of hanging indefinitely.

### Removed

- (Nothing removed — this is the initial release.)

<!--
## [0.1.0] - YYYY-MM-DD

### Added
### Changed
### Fixed
### Removed
-->
