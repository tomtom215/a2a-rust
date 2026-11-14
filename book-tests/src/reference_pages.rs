// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The book's `reference/` pages, each compiled as a doc-test like the rest
//! (`lib.rs`; `scripts/check_book_code.sh` reads both files).

#[doc = include_str!("../../book/src/reference/adrs.md")]
pub mod page_reference_adrs {}

#[doc = include_str!("../../book/src/reference/api-docs.md")]
pub mod page_reference_api_docs {}

#[doc = include_str!("../../book/src/reference/api-reference.md")]
pub mod page_reference_api_reference {}

#[doc = include_str!("../../book/src/reference/benchmarks.md")]
pub mod page_reference_benchmarks {}

#[doc = include_str!("../../book/src/reference/cross-language-benchmarks.md")]
pub mod page_reference_cross_language_benchmarks {}

#[doc = include_str!("../../book/src/reference/changelog.md")]
pub mod page_reference_changelog {}

#[doc = include_str!("../../book/src/reference/configuration.md")]
pub mod page_reference_configuration {}

#[doc = include_str!("../../book/src/reference/conformance-history.md")]
pub mod page_reference_conformance_history {}

#[doc = include_str!("../../book/src/reference/dashboard.md")]
pub mod page_reference_dashboard {}

#[doc = include_str!("../../book/src/reference/mutation-history.md")]
pub mod page_reference_mutation_history {}

#[doc = include_str!("../../book/src/reference/pitfalls.md")]
pub mod page_reference_pitfalls {}

#[doc = include_str!("../../book/src/reference/regression-gate.md")]
pub mod page_reference_regression_gate {}

#[doc = include_str!("../../book/src/reference/swarm-extensions.md")]
pub mod page_reference_swarm_extensions {}

#[doc = include_str!("../../book/src/reference/upgrading.md")]
pub mod page_reference_upgrading {}

#[doc = include_str!("../../book/src/reference/changelog/older.md")]
pub mod pages_reference_changelog {}

#[doc = include_str!("../../book/src/reference/conformance-history/acts-and-in-repo.md")]
#[doc = include_str!("../../book/src/reference/conformance-history/official-tck.md")]
#[doc = include_str!("../../book/src/reference/conformance-history/suppressions.md")]
#[doc = include_str!("../../book/src/reference/conformance-history/verification-log.md")]
pub mod pages_reference_conformance_history {}

#[doc = include_str!("../../book/src/reference/mutation-history/gate-defects.md")]
#[doc = include_str!("../../book/src/reference/mutation-history/per-file.md")]
#[doc = include_str!("../../book/src/reference/mutation-history/sweeps.md")]
pub mod pages_reference_mutation_history {}

#[doc = include_str!("../../book/src/reference/pitfalls/async-and-lifecycle.md")]
#[doc = include_str!("../../book/src/reference/pitfalls/contributing.md")]
#[doc = include_str!("../../book/src/reference/pitfalls/fixed.md")]
#[doc = include_str!("../../book/src/reference/pitfalls/http-and-transports.md")]
#[doc = include_str!("../../book/src/reference/pitfalls/security-and-resources.md")]
#[doc = include_str!("../../book/src/reference/pitfalls/serialization-and-validation.md")]
pub mod pages_reference_pitfalls {}

#[doc = include_str!("../../book/src/reference/upgrading/0.10-to-0.11.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.11-to-0.12.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.12-to-0.13.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.13-to-0.14.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.14-to-0.15.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.15-to-0.16.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.6-to-0.7.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.7-to-0.8.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.8-to-0.9.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.9-to-0.10.md")]
#[doc = include_str!("../../book/src/reference/upgrading/older.md")]
pub mod pages_reference_upgrading {}

#[doc = include_str!("../../book/src/reference/upgrading/0.14-to-0.15/agent-cards.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.14-to-0.15/authentication.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.14-to-0.15/client.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.14-to-0.15/errors-and-rust-api.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.14-to-0.15/not-breaking.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.14-to-0.15/push.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.14-to-0.15/stores-and-tenants.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.14-to-0.15/streams-and-executors.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.14-to-0.15/telemetry.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.14-to-0.15/toolchain.md")]
#[doc = include_str!("../../book/src/reference/upgrading/0.14-to-0.15/wire-and-bindings.md")]
pub mod pages_reference_upgrading_0_14_to_0_15 {}

#[doc = include_str!("../../book/src/reference/cli.md")]
pub mod page_reference_cli {}

#[doc = include_str!("../../book/src/reference/conformance-testing.md")]
pub mod page_reference_conformance_testing {}

#[doc = include_str!("../../book/src/reference/conformance-statement.md")]
pub mod page_reference_conformance_statement {}

#[doc = include_str!("../../book/src/reference/support-matrix.md")]
pub mod page_reference_support_matrix {}
