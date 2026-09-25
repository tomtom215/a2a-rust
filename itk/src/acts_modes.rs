// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The two extra ACTS passes, and the extended card's guard.
//!
//! The a2a-itk runner (`acts_runner.py`) starts this agent up to three
//! times. The main pass is the ordinary agent. Two more cover tests that no
//! single agent can: an agent cannot both stream and not stream, nor both
//! require and not require a credential.
//!
//! - `ITK_ACTS_REDUCED_CAPABILITIES`: advertise no optional capability, so
//!   the tests asserting "an agent without X refuses X" (`CORE-CAP-001/002`,
//!   `PUSH-CFG-004`, `SEC-EXTCARD-003`) apply. The SDK enforces its card
//!   already; this only publishes a smaller one.
//! - `ITK_ACTS_AUTH`: declare a bearer scheme on the card and enforce it on
//!   every operation, so the `SEC-AUTH` tests apply.
//!
//! The extended card is guarded in every pass (A2A §13.3 makes its
//! authentication unconditional). All enforcement goes through this SDK's
//! own `BearerTokenAuthInterceptor` and `A2aError::permission_denied`, so the
//! tests grade the auth an adopter gets, not a harness-side stand-in.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use a2a_protocol_server::call_context::CallContext;
use a2a_protocol_server::{BearerTokenAuthInterceptor, ServerInterceptor};
use a2a_protocol_types::AgentCapabilities;
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::security::{
    HttpAuthSecurityScheme, NamedSecuritySchemes, SecurityRequirement, SecurityScheme, StringList,
};

const REDUCED_CAPABILITIES_ENV: &str = "ITK_ACTS_REDUCED_CAPABILITIES";
const AUTH_ENFORCED_ENV: &str = "ITK_ACTS_AUTH";

/// The credential the ACTS runner presents on every abstract operation. Not a
/// secret: `acts_runner.py` publishes it.
const VALID_TOKEN: &str = "itk-valid-token";

/// The credential `SEC-AUTH-002` and `SEC-EXTCARD-002` present: one that
/// authenticates but is not permitted, which A2A §3.3.2 answers as an
/// authorization error (HTTP `403`), not an authentication one (`401`).
const INSUFFICIENT_TOKEN: &str = "itk-insufficient-token";

/// The caller `INSUFFICIENT_TOKEN` authenticates as.
const LIMITED_CALLER: &str = "limited";

const SCHEME_ID: &str = "bearerAuth";

pub(crate) fn reduced() -> bool {
    std::env::var_os(REDUCED_CAPABILITIES_ENV).is_some()
}

pub(crate) fn auth_enforced() -> bool {
    std::env::var_os(AUTH_ENFORCED_ENV).is_some()
}

pub(crate) fn capabilities() -> AgentCapabilities {
    let all = !reduced();
    let mut caps = AgentCapabilities::none()
        .with_streaming(all)
        .with_push_notifications(all);
    caps.extended_agent_card = Some(all);
    caps
}

/// Declared only when enforced: a card claiming a scheme it does not check
/// would be false, and the runner reads this to decide whether `SEC-AUTH`
/// applies.
pub(crate) fn security() -> (
    Option<NamedSecuritySchemes>,
    Option<Vec<SecurityRequirement>>,
) {
    if !auth_enforced() {
        return (None, None);
    }
    let scheme = SecurityScheme::Http(HttpAuthSecurityScheme {
        scheme: "Bearer".into(),
        bearer_format: Some("opaque".into()),
        description: Some("Bearer token presented by the ACTS runner.".into()),
    });
    let requirement = SecurityRequirement {
        schemes: HashMap::from([(SCHEME_ID.to_owned(), StringList { list: Vec::new() })]),
    };
    (
        Some(HashMap::from([(SCHEME_ID.to_owned(), scheme)])),
        Some(vec![requirement]),
    )
}

/// What the guard covers.
#[derive(Clone, Copy)]
pub(crate) enum Scope {
    /// Every operation: the ACTS auth pass.
    Everything,
    /// `GetExtendedAgentCard` alone: every other pass, whose operations are
    /// dialled with no credential. A2A §13.3 makes the extended card's
    /// authentication unconditional.
    ExtendedCard,
}

/// Authenticates with this SDK's `BearerTokenAuthInterceptor`, then refuses
/// the limited caller with `A2aError::permission_denied` — so both refusals
/// ACTS distinguishes go through the SDK: an unknown credential answers
/// `401`, a known but insufficient one `403`.
pub(crate) struct Guard {
    bearer: BearerTokenAuthInterceptor,
    scope: Scope,
}

impl Guard {
    pub(crate) fn new(scope: Scope) -> Self {
        Self {
            bearer: BearerTokenAuthInterceptor::with_labelled_tokens([
                (VALID_TOKEN, "runner"),
                (INSUFFICIENT_TOKEN, LIMITED_CALLER),
            ]),
            scope,
        }
    }
}

impl ServerInterceptor for Guard {
    fn before<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            if matches!(self.scope, Scope::ExtendedCard) && ctx.method() != "GetExtendedAgentCard" {
                return Ok(());
            }
            self.bearer.before(ctx).await?;
            if ctx.caller_identity() == Some(LIMITED_CALLER) {
                return Err(A2aError::permission_denied("not permitted"));
            }
            Ok(())
        })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn authenticates(&self) -> bool {
        true
    }
}
