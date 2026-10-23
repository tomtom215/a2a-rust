// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The tenant is resolved before admission, and resolving it can refuse: a
//! client tenant the resolver disagrees with, or no tenant at all under
//! strict multi-tenancy, was answered `InvalidParams` naming the tenant rule
//! to a caller the chain had not admitted (found reviewing the params fix,
//! 2026-10-20). A caller the chain refuses is now refused; an admitted one
//! still gets the tenant error.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use a2a_protocol_server::RequestHandlerBuilder;
use a2a_protocol_server::error::ServerError;
use a2a_protocol_server::tenant_resolver::HeaderTenantResolver;
use a2a_protocol_types::params::TaskQueryParams;

use super::{Exec, Misses, TOKENS, TokenGate};

fn headers(token: Option<&str>, tenant: Option<&str>) -> HashMap<String, String> {
    let mut h = HashMap::new();
    if let Some(token) = token {
        h.insert("authorization".to_owned(), format!("Bearer {token}"));
    }
    if let Some(tenant) = tenant {
        h.insert("x-tenant".to_owned(), tenant.to_owned());
    }
    h
}

/// Each token against one tenant situation: refused unless admitted, and
/// then the tenant error.
async fn check(strict: bool, header_tenant: Option<&str>, client_tenant: Option<&str>) {
    let mut builder = RequestHandlerBuilder::new(Exec)
        .without_agent_card()
        .with_interceptor(TokenGate(Arc::new(AtomicUsize::new(0))))
        .with_tenant_resolver(HeaderTenantResolver::new("x-tenant"));
    if strict {
        builder = builder.require_resolved_tenant();
    }
    let handler = builder.build().expect("handler");
    let mut misses = Misses::default();
    for token in TOKENS {
        let mut params = TaskQueryParams::new("t-1");
        params.tenant = client_tenant.map(str::to_owned);
        let answer = handler
            .on_get_task(params, Some(&headers(token, header_tenant)))
            .await;
        let admitted = token == Some("good");
        let right = match &answer {
            Err(ServerError::InvalidParams(_)) => admitted,
            Err(e) => !admitted && e.to_a2a_error().message.contains("not authenticated"),
            Ok(_) => false,
        };
        misses.check(right, || {
            format!("strict={strict} header={header_tenant:?} client={client_tenant:?} token={token:?}: {answer:?}")
        });
    }
    misses.assert_none();
}

#[tokio::test]
async fn a_tenant_the_resolver_disagrees_with_is_refused_before_it_is_explained() {
    check(false, Some("acme"), Some("other")).await;
}

#[tokio::test]
async fn strict_tenancy_without_a_tenant_is_refused_before_it_is_explained() {
    check(true, None, None).await;
}
