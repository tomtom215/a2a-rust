// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Prompt-injection containment: what stops an injected instruction from
//! having its effect, when the model obeys every instruction it reads.
//!
//! A model cannot be relied on to tell its operator's instructions from an
//! attacker's text in a web page, a tool result or another agent's
//! artifact. So the model here is the worst case, deterministic: it obeys
//! every `CALL <tool> <json>` line it reads, in the user's message or in a
//! tool's result, in the order it read them (`world::Obedient`). The
//! orchestrator is a `ToolLoopExecutor` over it, with tools that act on the
//! world: a payment ledger, a page fetcher serving attacker text, and tools
//! that reach other agents over A2A on loopback.
//!
//! Each test plants an injection where an attacker can put one and asserts
//! what the SDK does about the effect. Where the SDK cannot stop it, the
//! test asserts that the attack succeeds, so the documented limit is checked
//! as well as the control: `docs/injection-containment.md` has the table.
#![cfg(all(feature = "tool-loop", feature = "signing"))]

mod world;

mod approval;
mod delegation;
mod operator;
mod peers;
mod tool_results;
