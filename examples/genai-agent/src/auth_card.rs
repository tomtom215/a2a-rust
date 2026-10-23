// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The security schemes the authentication environment variables turn on,
//! declared in the agent card.

use a2a_protocol_types::agent_card::AgentCard;

/// Declares in `card` the scheme of each interceptor `main` registers for
/// `A2A_BEARER_TOKEN`, `A2A_JWT_HS256_SECRET` and `A2A_API_KEY`: spec §13.3
/// has requests
/// authenticated "using one of the schemes declared in the public
/// `AgentCard`", and the handler's build refuses an authenticator the card
/// does not declare.
pub fn declare(card: &mut AgentCard) {
    use a2a_protocol_types::security::{
        ApiKeyLocation, ApiKeySecurityScheme, HttpAuthSecurityScheme, SecurityRequirement,
        SecurityScheme, StringList,
    };
    let mut schemes = std::collections::HashMap::new();
    if std::env::var_os("A2A_BEARER_TOKEN").is_some()
        || std::env::var_os("A2A_JWT_HS256_SECRET").is_some()
    {
        schemes.insert(
            "bearer".to_owned(),
            SecurityScheme::Http(HttpAuthSecurityScheme {
                scheme: "Bearer".to_owned(),
                description: None,
                bearer_format: None,
            }),
        );
    }
    if std::env::var_os("A2A_API_KEY").is_some() {
        schemes.insert(
            "apiKey".to_owned(),
            SecurityScheme::ApiKey(ApiKeySecurityScheme {
                location: ApiKeyLocation::Header,
                name: "x-api-key".to_owned(),
                description: None,
            }),
        );
    }
    if !schemes.is_empty() {
        // Interceptors compose with AND semantics: one requirement naming
        // every scheme set.
        card.security_requirements = Some(vec![SecurityRequirement {
            schemes: schemes
                .keys()
                .map(|name| (name.clone(), StringList { list: vec![] }))
                .collect(),
        }]);
        card.security_schemes = Some(schemes);
    }
}
