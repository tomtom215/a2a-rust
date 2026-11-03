# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""The 26 wire structs and the constructor each gets (PR 2).

kind: req   positional argument of `new`
      opt   Option<T> field: `.with_x(T)` for `Some(T)`, nothing for `None`
      coll  Vec/HashMap field defaulting to empty: `.with_x(v)` unless empty
      val   plain field `new` fills with a default: `.with_x(v)` always, except when
            the value is spelled as that default (`default_if`)
`default_base`: the type implements `Default`, so `..Default::default()` / `..Type::default()`
starts the chain from `Type::default()`.
"""

from rust_literal import FieldSpec as F, TypeSpec


def T(name, *fields, default_base=False):
    return TypeSpec(name, list(fields), default_base)


SPECS = {s.name: s for s in [
    # params
    T("SendMessageConfiguration",
      F("accepted_output_modes", "val"), F("task_push_notification_config", "opt"),
      F("history_length", "opt"), F("return_immediately", "opt"), default_base=True),
    T("MessageSendParams", F("message", "req"), F("tenant", "opt"), F("configuration", "opt"), F("metadata", "opt")),
    T("TaskQueryParams", F("id", "req"), F("tenant", "opt"), F("history_length", "opt")),
    T("ListTasksParams", F("tenant", "opt"), F("context_id", "opt"), F("status", "opt"), F("page_size", "opt"),
      F("page_token", "opt"), F("history_length", "opt"), F("status_timestamp_after", "opt"),
      F("include_artifacts", "opt"), default_base=True),
    T("CancelTaskParams", F("id", "req"), F("tenant", "opt"), F("metadata", "opt")),
    T("GetPushConfigParams", F("task_id", "req"), F("id", "req"), F("tenant", "opt")),
    T("DeletePushConfigParams", F("task_id", "req"), F("id", "req"), F("tenant", "opt")),
    T("TaskIdParams", F("id", "req"), F("tenant", "opt")),
    T("ListPushConfigsParams", F("task_id", "req"), F("page_size", "opt"), F("page_token", "opt"), F("tenant", "opt")),
    T("GetExtendedAgentCardParams", F("tenant", "opt")),
    # push
    T("AuthenticationInfo", F("scheme", "req"), F("credentials", "opt")),
    # responses (TaskListResponse::new(tasks) exists and fills the sizes from the length)
    T("TaskListResponse", F("tasks", "req"), F("next_page_token", "val", skip_values=("String::new()", '"".to_string()', '"".into()', "String::default()")), F("page_size", "val"), F("total_size", "val")),
    T("ListPushConfigsResponse", F("configs", "req"), F("next_page_token", "opt")),
    # security
    T("StringList", F("list", "req")),
    T("SecurityRequirement", F("schemes", "req")),
    T("ApiKeySecurityScheme", F("location", "req"), F("name", "req"), F("description", "opt")),
    T("HttpAuthSecurityScheme", F("scheme", "req"), F("description", "opt"), F("bearer_format", "opt")),
    T("OAuth2SecurityScheme", F("flows", "req"), F("description", "opt"), F("oauth2_metadata_url", "opt")),
    T("OpenIdConnectSecurityScheme", F("open_id_connect_url", "req"), F("description", "opt")),
    T("MutualTlsSecurityScheme", F("description", "opt")),
    T("AuthorizationCodeFlow", F("authorization_url", "req"), F("token_url", "req"), F("refresh_url", "opt"),
      F("scopes", "coll"), F("pkce_required", "opt")),
    T("ClientCredentialsFlow", F("token_url", "req"), F("refresh_url", "opt"), F("scopes", "coll")),
    T("ImplicitFlow", F("authorization_url", "req"), F("refresh_url", "opt"), F("scopes", "coll")),
    T("PasswordOAuthFlow", F("token_url", "req"), F("refresh_url", "opt"), F("scopes", "coll")),
    T("DeviceCodeFlow", F("device_authorization_url", "req"), F("token_url", "req"), F("refresh_url", "opt"),
      F("scopes", "coll")),
    T("AgentCardSignature", F("protected", "req"), F("signature", "req"), F("header", "opt")),
]}

STRING_FIELDS = {"tenant", "id", "task_id", "context_id", "page_token", "status_timestamp_after", "scheme",
                 "credentials", "protected", "signature", "name", "description", "bearer_format",
                 "open_id_connect_url", "oauth2_metadata_url", "authorization_url", "token_url", "refresh_url",
                 "device_authorization_url", "next_page_token"}
for _spec in SPECS.values():
    for _f in _spec.fields:
        _f.string = _f.name in STRING_FIELDS
assert len(SPECS) == 26, len(SPECS)
