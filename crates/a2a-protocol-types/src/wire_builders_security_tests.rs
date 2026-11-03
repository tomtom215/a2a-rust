// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The builders of the security objects, the push authentication object and
//! the card signature: `new` yields exactly the required fields, and each
//! `with_*` sets its own field and no other. The generated structs do not
//! implement `PartialEq`, so the values are compared as the JSON they
//! serialize to, which is the wire form.

use serde_json::{Value, json};

use super::*;

fn json_of<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("serializes")
}

/// `got` equals `base` with `key` set to `want` and nothing else changed. (`got` and `want` are
/// taken by value because every caller hands over a temporary it has just built.)
#[track_caller]
#[allow(clippy::needless_pass_by_value)]
fn only_sets(base: &Value, got: Value, key: &str, want: Value) {
    let mut expected = base.clone();
    expected
        .as_object_mut()
        .expect("an object")
        .insert(key.to_owned(), want);
    assert_eq!(
        got, expected,
        "the setter for {key} must set {key} and no other field"
    );
}

fn scopes() -> HashMap<String, String> {
    HashMap::from([("read".to_owned(), "Read access".to_owned())])
}

#[test]
fn authentication_info_new_sets_the_scheme_and_with_credentials_sets_the_credentials() {
    let base = json_of(&AuthenticationInfo::new("Bearer"));
    assert_eq!(base, json!({"scheme": "Bearer"}));
    only_sets(
        &base,
        json_of(&AuthenticationInfo::new("Bearer").with_credentials("tok")),
        "credentials",
        json!("tok"),
    );
}

#[test]
fn agent_card_signature_takes_protected_then_signature_and_with_header_sets_the_header() {
    let base = json_of(&AgentCardSignature::new("prot", "sig"));
    assert_eq!(base, json!({"protected": "prot", "signature": "sig"}));
    only_sets(
        &base,
        json_of(&AgentCardSignature::new("prot", "sig").with_header(json!({"kid": "k1"}))),
        "header",
        json!({"kid": "k1"}),
    );
}

#[test]
fn string_list_and_security_requirement_hold_what_they_were_given() {
    assert_eq!(
        json_of(&StringList::new(vec!["a".into(), "b".into()])),
        json!({"list": ["a", "b"]})
    );
    let requirement = SecurityRequirement::new(HashMap::from([(
        "oauth".to_owned(),
        StringList::new(vec!["read".into()]),
    )]));
    assert_eq!(
        json_of(&requirement),
        json!({"schemes": {"oauth": {"list": ["read"]}}})
    );
}

#[test]
fn api_key_scheme_takes_location_then_name_and_with_description_sets_the_description() {
    let base = json_of(&ApiKeySecurityScheme::new(ApiKeyLocation::Header, "X-Key"));
    assert_eq!(base, json!({"location": "header", "name": "X-Key"}));
    only_sets(
        &base,
        json_of(&ApiKeySecurityScheme::new(ApiKeyLocation::Header, "X-Key").with_description("d")),
        "description",
        json!("d"),
    );
}

#[test]
fn http_auth_scheme_each_setter_sets_one_field() {
    let base = json_of(&HttpAuthSecurityScheme::new("bearer"));
    assert_eq!(base, json!({"scheme": "bearer"}));
    only_sets(
        &base,
        json_of(&HttpAuthSecurityScheme::new("bearer").with_description("d")),
        "description",
        json!("d"),
    );
    only_sets(
        &base,
        json_of(&HttpAuthSecurityScheme::new("bearer").with_bearer_format("JWT")),
        "bearerFormat",
        json!("JWT"),
    );
}

#[test]
fn oauth2_scheme_openid_scheme_and_mutual_tls_scheme() {
    let flows =
        || OAuthFlows::ClientCredentials(ClientCredentialsFlow::new("https://idp.test/token"));
    let oauth = json_of(&OAuth2SecurityScheme::new(flows()));
    assert_eq!(oauth, json!({"flows": json_of(&flows())}));
    only_sets(
        &oauth,
        json_of(&OAuth2SecurityScheme::new(flows()).with_description("d")),
        "description",
        json!("d"),
    );
    only_sets(
        &oauth,
        json_of(
            &OAuth2SecurityScheme::new(flows()).with_oauth2_metadata_url(
                "https://idp.test/.well-known/oauth-authorization-server",
            ),
        ),
        "oauth2MetadataUrl",
        json!("https://idp.test/.well-known/oauth-authorization-server"),
    );

    let oidc = json_of(&OpenIdConnectSecurityScheme::new(
        "https://idp.test/.well-known/openid-configuration",
    ));
    assert_eq!(
        oidc,
        json!({"openIdConnectUrl": "https://idp.test/.well-known/openid-configuration"})
    );
    only_sets(
        &oidc,
        json_of(
            &OpenIdConnectSecurityScheme::new("https://idp.test/.well-known/openid-configuration")
                .with_description("d"),
        ),
        "description",
        json!("d"),
    );

    let mtls = json_of(&MutualTlsSecurityScheme::new());
    assert_eq!(mtls, json!({}));
    assert_eq!(json_of(&MutualTlsSecurityScheme::default()), mtls);
    only_sets(
        &mtls,
        json_of(&MutualTlsSecurityScheme::new().with_description("d")),
        "description",
        json!("d"),
    );
}

#[test]
fn authorization_code_flow_takes_authorization_url_then_token_url() {
    let base = json_of(&AuthorizationCodeFlow::new(
        "https://idp.test/auth",
        "https://idp.test/token",
    ));
    assert_eq!(
        base,
        json!({"authorizationUrl": "https://idp.test/auth", "tokenUrl": "https://idp.test/token", "scopes": {}})
    );
    let flow = || AuthorizationCodeFlow::new("https://idp.test/auth", "https://idp.test/token");
    only_sets(
        &base,
        json_of(&flow().with_refresh_url("https://idp.test/refresh")),
        "refreshUrl",
        json!("https://idp.test/refresh"),
    );
    only_sets(
        &base,
        json_of(&flow().with_scopes(scopes())),
        "scopes",
        json!({"read": "Read access"}),
    );
    only_sets(
        &base,
        json_of(&flow().with_pkce_required(true)),
        "pkceRequired",
        json!(true),
    );
    assert_eq!(flow().with_pkce_required(false).pkce_required, Some(false));
}

#[test]
fn client_credentials_and_password_flows_take_a_token_url_and_set_their_own_fields() {
    let cc = || ClientCredentialsFlow::new("https://idp.test/token");
    let base = json_of(&cc());
    assert_eq!(
        base,
        json!({"tokenUrl": "https://idp.test/token", "scopes": {}})
    );
    only_sets(
        &base,
        json_of(&cc().with_refresh_url("r")),
        "refreshUrl",
        json!("r"),
    );
    only_sets(
        &base,
        json_of(&cc().with_scopes(scopes())),
        "scopes",
        json!({"read": "Read access"}),
    );

    let pw = || PasswordOAuthFlow::new("https://idp.test/token");
    let base = json_of(&pw());
    assert_eq!(
        base,
        json!({"tokenUrl": "https://idp.test/token", "scopes": {}})
    );
    only_sets(
        &base,
        json_of(&pw().with_refresh_url("r")),
        "refreshUrl",
        json!("r"),
    );
    only_sets(
        &base,
        json_of(&pw().with_scopes(scopes())),
        "scopes",
        json!({"read": "Read access"}),
    );
}

#[test]
fn implicit_flow_takes_an_authorization_url_and_sets_its_own_fields() {
    let imp = || ImplicitFlow::new("https://idp.test/auth");
    let base = json_of(&imp());
    assert_eq!(
        base,
        json!({"authorizationUrl": "https://idp.test/auth", "scopes": {}})
    );
    only_sets(
        &base,
        json_of(&imp().with_refresh_url("r")),
        "refreshUrl",
        json!("r"),
    );
    only_sets(
        &base,
        json_of(&imp().with_scopes(scopes())),
        "scopes",
        json!({"read": "Read access"}),
    );
}

#[test]
fn device_code_flow_takes_the_device_url_then_the_token_url() {
    let dc = || DeviceCodeFlow::new("https://idp.test/device", "https://idp.test/token");
    let base = json_of(&dc());
    assert_eq!(
        base,
        json!({"deviceAuthorizationUrl": "https://idp.test/device", "tokenUrl": "https://idp.test/token", "scopes": {}})
    );
    only_sets(
        &base,
        json_of(&dc().with_refresh_url("r")),
        "refreshUrl",
        json!("r"),
    );
    only_sets(
        &base,
        json_of(&dc().with_scopes(scopes())),
        "scopes",
        json!({"read": "Read access"}),
    );
}
