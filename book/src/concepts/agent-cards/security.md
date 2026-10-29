<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Security schemes

Agent cards declare security schemes and the requirements that reference
them. This is the v1.0 wire form, which is what this SDK emits:

```json
{
  "securitySchemes": {
    "bearer": { "httpAuthSecurityScheme": { "scheme": "bearer" } },
    "oauth": { "oauth2SecurityScheme": { "flows": { "clientCredentials": {
      "tokenUrl": "https://auth.example.com/token", "scopes": { "read": "Read" } } } } }
  },
  "securityRequirements": [
    { "schemes": { "bearer": { "list": [] } } },
    { "schemes": { "oauth": { "list": ["read"] } } }
  ]
}
```

Each scope list is a `StringList` object (`{"list": [...]}`), because the spec
defines the JSON as the ProtoJSON of `map<string, StringList>`. When reading a
card, a bare array (`"oauth": ["read"]`, as a2a-go v2.5.0 writes it) and `null`
are accepted too, so a Go agent's card parses; only the `{"list": [...]}` form
is ever written.

Individual skills can also declare the security requirements that apply to
them (`AgentSkill::security_requirements`). The SDK does not enforce them per
skill: `build()` only counts them when it checks that an authenticating
interceptor exists.

## What `build()` checks

`RequestHandlerBuilder::build` also checks the card's security against the
handler's interceptors (spec §13.3). It fails with `InvalidParams` when:

- an authenticating interceptor is registered but the public card declares no
  `securitySchemes`, or none the interceptor checks:
  `BearerTokenAuthInterceptor` and `JwtAuthInterceptor` check an HTTP
  `bearer` scheme (an OAuth 2.0 or OpenID Connect scheme also counts, as
  their tokens are bearer tokens), `ApiKeyAuthInterceptor` an
  `apiKeySecurityScheme` in a header of the name it reads, and
  `MutualTlsAuthInterceptor` an `mtlsSecurityScheme`;
- the extended card declares a scheme the public card does not declare under
  the same name with the same definition;
- the card, or a skill, has `securityRequirements` and no interceptor
  authenticates, unless you call `authenticated_upstream()`;
- an OAuth flow URL, a refresh URL or an `openIdConnectUrl` is not a URL, or
  `oauth2MetadataUrl` (and the deprecated implicit and password flows' URLs)
  is not `https`;
- any card URL carries user information or a query parameter named like a
  secret (`token`, `api_key`, `password`, ...).
