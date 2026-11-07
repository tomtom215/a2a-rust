// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The security schemes the authentication environment variables turn on,
//! declared in the agent card, and how `A2A_JWT_HS256_SECRET` is read.

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
            SecurityScheme::Http(HttpAuthSecurityScheme::new("Bearer".to_owned())),
        );
    }
    if std::env::var_os("A2A_API_KEY").is_some() {
        schemes.insert(
            "apiKey".to_owned(),
            SecurityScheme::ApiKey(ApiKeySecurityScheme::new(
                ApiKeyLocation::Header,
                "x-api-key".to_owned(),
            )),
        );
    }
    if !schemes.is_empty() {
        // Interceptors compose with AND semantics: one requirement naming
        // every scheme set.
        card.security_requirements = Some(vec![SecurityRequirement::new(
            schemes
                .keys()
                .map(|name| (name.clone(), StringList::new(vec![])))
                .collect(),
        )]);
        card.security_schemes = Some(schemes);
    }
}

/// The HS256 key `A2A_JWT_HS256_SECRET` holds, decoded.
///
/// It is a key, not a password (RFC 8725 section 3.5): random bytes in
/// standard base64, as `openssl rand -base64 32` writes them. A passphrase
/// someone typed is therefore refused here (it is not base64) or, when it
/// happens to be base64, by the validator's 32-byte floor once decoded, rather
/// than used as the HMAC key. Surrounding whitespace, such as the newline of a
/// key read from a file, is ignored.
///
/// # Errors
///
/// A message naming the variable and how to make a key, when `value` is not
/// base64.
pub fn hs256_key(value: &str) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(value.trim())
        .map_err(|e| {
            format!(
                "A2A_JWT_HS256_SECRET is not base64 ({e}); \
                 make a key with `openssl rand -base64 32`"
            )
        })
}
