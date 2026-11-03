// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Constructors and builders for the security objects, the push
//! authentication object and the card signature, which are
//! `#[non_exhaustive]` (audit G-10, the 26 wire structs).
//!
//! `new` takes the fields the protocol requires; a `with_*` method sets each
//! optional one. Each leaves every field it is not given at the value a struct
//! literal would have written (`None`, or empty), so converting a literal
//! changes no wire byte.

use std::collections::HashMap;

use serde_json::Value;

use crate::extensions::AgentCardSignature;
use crate::push::AuthenticationInfo;
use crate::security::{
    ApiKeyLocation, ApiKeySecurityScheme, AuthorizationCodeFlow, ClientCredentialsFlow,
    DeviceCodeFlow, HttpAuthSecurityScheme, ImplicitFlow, MutualTlsSecurityScheme,
    OAuth2SecurityScheme, OAuthFlows, OpenIdConnectSecurityScheme, PasswordOAuthFlow,
    SecurityRequirement, StringList,
};

impl AuthenticationInfo {
    /// Authentication by `scheme` (`"Bearer"`, `"Basic"`, ...), with no credentials.
    #[must_use]
    pub fn new(scheme: impl Into<String>) -> Self {
        Self {
            scheme: scheme.into(),
            credentials: None,
        }
    }

    /// Sets the credentials the scheme presents (a token, for `Bearer`).
    #[must_use]
    pub fn with_credentials(mut self, credentials: impl Into<String>) -> Self {
        self.credentials = Some(credentials.into());
        self
    }
}

impl AgentCardSignature {
    /// A JWS signature: the base64url `protected` header and the base64url `signature`.
    #[must_use]
    pub fn new(protected: impl Into<String>, signature: impl Into<String>) -> Self {
        Self {
            protected: protected.into(),
            signature: signature.into(),
            header: None,
        }
    }

    /// Sets the unprotected JWS header.
    #[must_use]
    pub fn with_header(mut self, header: Value) -> Self {
        self.header = Some(header);
        self
    }
}

impl StringList {
    /// A list of scopes (or other strings).
    #[must_use]
    pub const fn new(list: Vec<String>) -> Self {
        Self { list }
    }
}

impl SecurityRequirement {
    /// A requirement that all of `schemes` hold, each with the scopes it names.
    #[must_use]
    pub const fn new(schemes: HashMap<String, StringList>) -> Self {
        Self { schemes }
    }
}

impl ApiKeySecurityScheme {
    /// An API key carried in `location` under `name`.
    #[must_use]
    pub fn new(location: ApiKeyLocation, name: impl Into<String>) -> Self {
        Self {
            description: None,
            location,
            name: name.into(),
        }
    }

    /// Sets the scheme's description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl HttpAuthSecurityScheme {
    /// HTTP authentication by `scheme` (`"Bearer"`, `"Basic"`, ...).
    #[must_use]
    pub fn new(scheme: impl Into<String>) -> Self {
        Self {
            description: None,
            scheme: scheme.into(),
            bearer_format: None,
        }
    }

    /// Sets the scheme's description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the format of the bearer token (`"JWT"`, for one).
    #[must_use]
    pub fn with_bearer_format(mut self, format: impl Into<String>) -> Self {
        self.bearer_format = Some(format.into());
        self
    }
}

impl OAuth2SecurityScheme {
    /// OAuth 2.0 with the given `flows`.
    #[must_use]
    pub const fn new(flows: OAuthFlows) -> Self {
        Self {
            description: None,
            flows,
            oauth2_metadata_url: None,
        }
    }

    /// Sets the URL of the authorization server's metadata document (RFC 8414).
    #[must_use]
    pub fn with_oauth2_metadata_url(mut self, url: impl Into<String>) -> Self {
        self.oauth2_metadata_url = Some(url.into());
        self
    }

    /// Sets the scheme's description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl OpenIdConnectSecurityScheme {
    /// `OpenID` Connect discovery at `open_id_connect_url`.
    #[must_use]
    pub fn new(open_id_connect_url: impl Into<String>) -> Self {
        Self {
            description: None,
            open_id_connect_url: open_id_connect_url.into(),
        }
    }

    /// Sets the scheme's description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl MutualTlsSecurityScheme {
    /// Mutual TLS, with no description.
    #[must_use]
    pub const fn new() -> Self {
        Self { description: None }
    }

    /// Sets the scheme's description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl Default for MutualTlsSecurityScheme {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthorizationCodeFlow {
    /// The authorization-code flow with its two endpoints, no refresh URL, no scopes.
    #[must_use]
    pub fn new(authorization_url: impl Into<String>, token_url: impl Into<String>) -> Self {
        Self {
            authorization_url: authorization_url.into(),
            token_url: token_url.into(),
            refresh_url: None,
            scopes: HashMap::new(),
            pkce_required: None,
        }
    }

    /// Sets the URL for refreshing tokens.
    #[must_use]
    pub fn with_refresh_url(mut self, url: impl Into<String>) -> Self {
        self.refresh_url = Some(url.into());
        self
    }

    /// Sets the available scopes, each with its description.
    #[must_use]
    pub fn with_scopes(mut self, scopes: HashMap<String, String>) -> Self {
        self.scopes = scopes;
        self
    }

    /// Sets whether PKCE is required.
    #[must_use]
    pub const fn with_pkce_required(mut self, required: bool) -> Self {
        self.pkce_required = Some(required);
        self
    }
}

impl ClientCredentialsFlow {
    /// The client-credentials flow with its token endpoint, no refresh URL, no scopes.
    #[must_use]
    pub fn new(token_url: impl Into<String>) -> Self {
        Self {
            token_url: token_url.into(),
            refresh_url: None,
            scopes: HashMap::new(),
        }
    }

    /// Sets the URL for refreshing tokens.
    #[must_use]
    pub fn with_refresh_url(mut self, url: impl Into<String>) -> Self {
        self.refresh_url = Some(url.into());
        self
    }

    /// Sets the available scopes, each with its description.
    #[must_use]
    pub fn with_scopes(mut self, scopes: HashMap<String, String>) -> Self {
        self.scopes = scopes;
        self
    }
}

impl ImplicitFlow {
    /// The implicit flow with its authorization endpoint, no refresh URL, no scopes.
    #[must_use]
    pub fn new(authorization_url: impl Into<String>) -> Self {
        Self {
            authorization_url: authorization_url.into(),
            refresh_url: None,
            scopes: HashMap::new(),
        }
    }

    /// Sets the URL for refreshing tokens.
    #[must_use]
    pub fn with_refresh_url(mut self, url: impl Into<String>) -> Self {
        self.refresh_url = Some(url.into());
        self
    }

    /// Sets the available scopes, each with its description.
    #[must_use]
    pub fn with_scopes(mut self, scopes: HashMap<String, String>) -> Self {
        self.scopes = scopes;
        self
    }
}

impl PasswordOAuthFlow {
    /// The password flow with its token endpoint, no refresh URL, no scopes.
    #[must_use]
    pub fn new(token_url: impl Into<String>) -> Self {
        Self {
            token_url: token_url.into(),
            refresh_url: None,
            scopes: HashMap::new(),
        }
    }

    /// Sets the URL for refreshing tokens.
    #[must_use]
    pub fn with_refresh_url(mut self, url: impl Into<String>) -> Self {
        self.refresh_url = Some(url.into());
        self
    }

    /// Sets the available scopes, each with its description.
    #[must_use]
    pub fn with_scopes(mut self, scopes: HashMap<String, String>) -> Self {
        self.scopes = scopes;
        self
    }
}

impl DeviceCodeFlow {
    /// The device-code flow with its two endpoints, no refresh URL, no scopes.
    #[must_use]
    pub fn new(device_authorization_url: impl Into<String>, token_url: impl Into<String>) -> Self {
        Self {
            device_authorization_url: device_authorization_url.into(),
            token_url: token_url.into(),
            refresh_url: None,
            scopes: HashMap::new(),
        }
    }

    /// Sets the URL for refreshing tokens.
    #[must_use]
    pub fn with_refresh_url(mut self, url: impl Into<String>) -> Self {
        self.refresh_url = Some(url.into());
        self
    }

    /// Sets the available scopes, each with its description.
    #[must_use]
    pub fn with_scopes(mut self, scopes: HashMap<String, String>) -> Self {
        self.scopes = scopes;
        self
    }
}

#[cfg(test)]
#[path = "wire_builders_security_tests.rs"]
mod tests;
