// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

// "OpenAPI", "OpenID", and similar proper-noun initialisms are intentionally
// not wrapped in backticks in this module's documentation.
#![allow(clippy::doc_markdown)]

//! Security scheme types for A2A agent authentication.
//!
//! These types follow the security-scheme specification used by A2A v1.0,
//! which is based on the OpenAPI 3.x security model.
//! The root discriminated union is [`SecurityScheme`], tagged on the `"type"` field.
//!
//! [`NamedSecuritySchemes`] is a type alias, and [`SecurityRequirement`] is a
//! struct used in [`crate::agent_card::AgentCard`] and
//! [`crate::agent_card::AgentSkill`].

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ── Type aliases ──────────────────────────────────────────────────────────────

/// A map from security scheme name to its definition, as used in
/// `AgentCard.securitySchemes`.
pub type NamedSecuritySchemes = HashMap<String, SecurityScheme>;

/// A list of strings used within a [`SecurityRequirement`] map value.
///
/// Proto equivalent: `StringList { repeated string list = 1; }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StringList {
    /// The string values (e.g. OAuth scopes).
    pub list: Vec<String>,
}

/// A security requirement object mapping scheme names to their required scopes.
///
/// Proto equivalent: `SecurityRequirement { map<string, StringList> schemes = 1; }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityRequirement {
    /// Map from scheme name to required scopes.
    pub schemes: HashMap<String, StringList>,
}

// ── SecurityScheme ────────────────────────────────────────────────────────────

/// A security scheme supported by an agent, discriminated by the `"type"` field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SecurityScheme {
    /// API key authentication (`"apiKey"`).
    #[serde(rename = "apiKey")]
    ApiKey(ApiKeySecurityScheme),

    /// HTTP authentication (e.g. Bearer, Basic) (`"http"`).
    #[serde(rename = "http")]
    Http(HttpAuthSecurityScheme),

    /// OAuth 2.0 (`"oauth2"`).
    ///
    /// Boxed to reduce the enum's stack size.
    #[serde(rename = "oauth2")]
    OAuth2(Box<OAuth2SecurityScheme>),

    /// OpenID Connect (`"openIdConnect"`).
    #[serde(rename = "openIdConnect")]
    OpenIdConnect(OpenIdConnectSecurityScheme),

    /// Mutual TLS (`"mutualTLS"`).
    #[serde(rename = "mutualTLS")]
    MutualTls(MutualTlsSecurityScheme),
}

// ── ApiKeySecurityScheme ──────────────────────────────────────────────────────

/// API key security scheme: a token sent in a header, query parameter, or cookie.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeySecurityScheme {
    /// Where the API key is transmitted.
    ///
    /// Serialized as `"in"` (a Rust keyword; mapped via `rename`).
    #[serde(rename = "in")]
    pub location: ApiKeyLocation,

    /// Name of the header, query parameter, or cookie.
    pub name: String,

    /// Optional human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Where an API key is placed in the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApiKeyLocation {
    /// Transmitted as an HTTP header.
    Header,
    /// Transmitted as a URL query parameter.
    Query,
    /// Transmitted as a cookie.
    Cookie,
}

// ── HttpAuthSecurityScheme ────────────────────────────────────────────────────

/// HTTP authentication security scheme (Bearer, Basic, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpAuthSecurityScheme {
    /// The HTTP authentication scheme name (e.g. `"bearer"`, `"basic"`).
    pub scheme: String,

    /// Format hint for Bearer tokens (e.g. `"JWT"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearer_format: Option<String>,

    /// Optional human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ── OAuth2SecurityScheme ──────────────────────────────────────────────────────

/// OAuth 2.0 security scheme.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuth2SecurityScheme {
    /// Available OAuth 2.0 flows.
    pub flows: OAuthFlows,

    /// URL of the OAuth 2.0 server metadata document (RFC 8414).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth2_metadata_url: Option<String>,

    /// Optional human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Available OAuth 2.0 flows for an [`OAuth2SecurityScheme`].
///
/// Mirrors the OpenAPI 3.x `OAuthFlows` object.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthFlows {
    /// Authorization code flow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_code: Option<AuthorizationCodeFlow>,

    /// Client credentials flow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_credentials: Option<ClientCredentialsFlow>,

    /// Device authorization flow (RFC 8628).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_code: Option<DeviceCodeFlow>,

    /// Implicit flow (deprecated in OAuth 2.1 but retained for compatibility).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implicit: Option<ImplicitFlow>,

    /// Resource owner password credentials flow (deprecated but present in spec).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<PasswordOAuthFlow>,
}

/// OAuth 2.0 authorization code flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationCodeFlow {
    /// URL of the authorization endpoint.
    pub authorization_url: String,

    /// URL of the token endpoint.
    pub token_url: String,

    /// URL of the refresh token endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_url: Option<String>,

    /// Available scopes: name → description.
    pub scopes: HashMap<String, String>,

    /// Whether PKCE (RFC 7636) is required for this flow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pkce_required: Option<bool>,
}

/// OAuth 2.0 client credentials flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCredentialsFlow {
    /// URL of the token endpoint.
    pub token_url: String,

    /// URL of the refresh token endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_url: Option<String>,

    /// Available scopes: name → description.
    pub scopes: HashMap<String, String>,
}

/// OAuth 2.0 device authorization flow (RFC 8628).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCodeFlow {
    /// URL of the device authorization endpoint.
    pub device_authorization_url: String,

    /// URL of the token endpoint.
    pub token_url: String,

    /// URL of the refresh token endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_url: Option<String>,

    /// Available scopes: name → description.
    pub scopes: HashMap<String, String>,
}

/// OAuth 2.0 implicit flow (deprecated; retained for compatibility).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImplicitFlow {
    /// URL of the authorization endpoint.
    pub authorization_url: String,

    /// URL of the refresh token endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_url: Option<String>,

    /// Available scopes: name → description.
    pub scopes: HashMap<String, String>,
}

/// OAuth 2.0 resource owner password credentials flow (deprecated but in spec).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordOAuthFlow {
    /// URL of the token endpoint.
    pub token_url: String,

    /// URL of the refresh token endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_url: Option<String>,

    /// Available scopes: name → description.
    pub scopes: HashMap<String, String>,
}

// ── OpenIdConnectSecurityScheme ───────────────────────────────────────────────

/// OpenID Connect security scheme.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenIdConnectSecurityScheme {
    /// URL of the OpenID Connect discovery document.
    pub open_id_connect_url: String,

    /// Optional human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ── MutualTlsSecurityScheme ───────────────────────────────────────────────────

/// Mutual TLS security scheme.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutualTlsSecurityScheme {
    /// Optional human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_scheme_roundtrip() {
        let scheme = SecurityScheme::ApiKey(ApiKeySecurityScheme {
            location: ApiKeyLocation::Header,
            name: "X-API-Key".into(),
            description: None,
        });
        let json = serde_json::to_string(&scheme).expect("serialize");
        assert!(
            json.contains("\"type\":\"apiKey\""),
            "tag must be present: {json}"
        );
        assert!(
            json.contains("\"in\":\"header\""),
            "location must use 'in': {json}"
        );

        let back: SecurityScheme = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back, SecurityScheme::ApiKey(_)));
    }

    #[test]
    fn http_bearer_scheme_roundtrip() {
        let scheme = SecurityScheme::Http(HttpAuthSecurityScheme {
            scheme: "bearer".into(),
            bearer_format: Some("JWT".into()),
            description: None,
        });
        let json = serde_json::to_string(&scheme).expect("serialize");
        assert!(json.contains("\"type\":\"http\""));
        let back: SecurityScheme = serde_json::from_str(&json).expect("deserialize");
        if let SecurityScheme::Http(h) = back {
            assert_eq!(h.bearer_format.as_deref(), Some("JWT"));
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn oauth2_scheme_roundtrip() {
        let scheme = SecurityScheme::OAuth2(Box::new(OAuth2SecurityScheme {
            flows: OAuthFlows {
                authorization_code: None,
                client_credentials: Some(ClientCredentialsFlow {
                    token_url: "https://auth.example.com/token".into(),
                    refresh_url: None,
                    scopes: HashMap::from([("read".into(), "Read access".into())]),
                }),
                device_code: None,
                implicit: None,
                password: None,
            },
            oauth2_metadata_url: None,
            description: None,
        }));
        let json = serde_json::to_string(&scheme).expect("serialize");
        assert!(json.contains("\"type\":\"oauth2\""));
        let back: SecurityScheme = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back, SecurityScheme::OAuth2(_)));
    }

    #[test]
    fn mutual_tls_scheme_roundtrip() {
        let scheme = SecurityScheme::MutualTls(MutualTlsSecurityScheme { description: None });
        let json = serde_json::to_string(&scheme).expect("serialize");
        assert!(json.contains("\"type\":\"mutualTLS\""));
        let back: SecurityScheme = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back, SecurityScheme::MutualTls(_)));
    }

    #[test]
    fn api_key_location_serialization() {
        assert_eq!(
            serde_json::to_string(&ApiKeyLocation::Header).expect("ser"),
            "\"header\""
        );
        assert_eq!(
            serde_json::to_string(&ApiKeyLocation::Query).expect("ser"),
            "\"query\""
        );
        assert_eq!(
            serde_json::to_string(&ApiKeyLocation::Cookie).expect("ser"),
            "\"cookie\""
        );
    }

    #[test]
    fn wire_format_security_requirement() {
        // Spec: {"schemes":{"oauth2":{"list":["read","write"]}}}
        let req = SecurityRequirement {
            schemes: HashMap::from([(
                "oauth2".into(),
                StringList {
                    list: vec!["read".into(), "write".into()],
                },
            )]),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed["schemes"]["oauth2"]["list"],
            serde_json::json!(["read", "write"])
        );

        // Roundtrip
        let back: SecurityRequirement = serde_json::from_str(&json).unwrap();
        assert_eq!(back.schemes["oauth2"].list, vec!["read", "write"]);
    }

    #[test]
    fn wire_format_password_oauth_flow() {
        let flows = OAuthFlows {
            authorization_code: None,
            client_credentials: None,
            device_code: None,
            implicit: None,
            password: Some(PasswordOAuthFlow {
                token_url: "https://auth.example.com/token".into(),
                refresh_url: None,
                scopes: HashMap::from([("read".into(), "Read access".into())]),
            }),
        };
        let json = serde_json::to_string(&flows).unwrap();
        assert!(
            json.contains("\"password\""),
            "password flow must be present: {json}"
        );

        let back: OAuthFlows = serde_json::from_str(&json).unwrap();
        assert!(back.password.is_some());
        assert_eq!(
            back.password.unwrap().token_url,
            "https://auth.example.com/token"
        );
    }
}
