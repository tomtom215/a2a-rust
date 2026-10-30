<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# JWT and OIDC

Accept tokens an identity provider signs (HS256, RS256 or ES256) with `JwtAuthInterceptor`. The caller's identity comes from the token's issuer and subject (`jwt:<issuer>:<sub>`; see [Naming the caller](./api-keys.md#naming-the-caller)). To require a scope on every call, see [Authorization](./authorization.md); the algorithm and secret-length rules are under [Security properties](../authentication.md#security-properties).

## What is checked

Enable the `auth-jwt` feature. `JwtAuthInterceptor` verifies the token's
signature, then its claims: `exp` must be present and in the future
(`allow_missing_exp()` lifts that), and `nbf` is honoured when present, both
with 60 s of clock-skew leeway (`with_leeway`). `iss` and `aud` are always
checked: name the accepted ones with `with_issuer` and `with_audience`. A
validator that names no issuer, or no audience, refuses every token (and
logs why when the interceptor is built) unless `allow_any_issuer()` or
`allow_any_audience()` says that is meant. Without the check, a key set that
signs for more than one issuer or application (another tenant of a
multi-tenant identity provider, a token minted for another API) let any of
its tokens in; that was the default until this release. Keys come from a
static `Jwks`, a shared HS256 secret, or a remote JWKS endpoint. A remote
JWKS or discovery URL must be `https://`, or `http://` to a loopback host.

```toml
a2a-protocol-server = { version = "0.15", features = ["auth-jwt", "tls-rustls"] }
```

`tls-rustls` is needed to fetch JWKS or OIDC discovery documents over
`https://`; without it the fetch is plaintext-only. Static keys and HS256 need
only `auth-jwt`.

## Tokens from an OIDC issuer

`JwtAuthInterceptor::from_oidc_issuer` discovers the issuer's `jwks_uri` at
construction, fetches and caches the set on the first request, and refetches it
on key rotation. The discovery document's `issuer` must be the one you name
(OpenID Connect Discovery 1.0 §4.3; a trailing `/` is ignored), and a
validator that names no issuer is bound to it:

```rust,no_run
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_server::auth::jwt::{JwtAuthInterceptor, JwtValidator};

let validator = JwtValidator::new()
    .with_issuer("https://login.example.com")
    .with_audience("my-a2a-agent");

let interceptor =
    JwtAuthInterceptor::from_oidc_issuer("https://login.example.com", validator).await?;
# Ok(())
# }
```

## A JWKS URL you already know

`JwtAuthInterceptor::from_jwks_url` takes the
validator and the URL; `from_jwks_url_with_tls_config` (`tls-rustls`) serves an
identity provider behind a private CA. Nothing is fetched at construction; the
first request fetches the set, and `with_jwks_ttl` changes how long it is
cached (one hour by default). A token naming an unknown `kid` refetches the
set, at most once per 10 s (`with_jwks_refetch_interval`): inside that
interval of the last fetch it is refused with `401` and nothing is fetched,
and requests that arrive while a refetch is under way wait for it. Before
2026-10-30 every such token forced a fetch, so anyone, with no credential,
could make the agent send the identity provider one request per request it
received. A fetch is bounded to 30 s and 256 KiB. If the first fetch fails, or
the set holds no usable key, the call is refused with a server error (not
`401`), because no verdict about the token could be reached; a refetch for an
unknown `kid` that fails leaves the keys in hand, and the token is refused
with `401`. Only RSA keys and P-256 EC keys are
loaded, and an RS256 key must be 2048 to 8192 bits; entries with
`"use": "enc"` or another key type are skipped.

## Static keys and HS256

Supply a `Jwks` directly (from a JWKS JSON document or key by
key), or a shared secret for HS256:

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let jwks_bytes: &[u8] = br#"{"keys": []}"#;
use a2a_protocol_server::auth::jwt::{Jwks, JwtAuthInterceptor, JwtValidator};

// RS256/ES256 from a JWKS document:
let jwks = Jwks::from_json(jwks_bytes)?;
let interceptor = JwtAuthInterceptor::new(
    JwtValidator::new().with_issuer("https://issuer").with_audience("agent"),
    jwks,
);

// HS256 shared secret (no JWKS). At least 32 bytes (RFC 7518 §3.2);
// `try_with_hs256_secret` refuses a shorter one here, at startup.
let secret = b"0123456789abcdef0123456789abcdef".to_vec(); // load yours from a secret store
let interceptor = JwtAuthInterceptor::new(
    JwtValidator::new()
        .with_issuer("https://issuer")
        .with_audience("agent")
        .try_with_hs256_secret(secret)?,
    Jwks::new(),
);
# Ok(())
# }
```

## Revoking a token

A JWT is revoked by its `exp`, or by removing its signing key from the JWKS. A
remote set is cached for an hour by default (`JwtAuthInterceptor::with_jwks_ttl`)
and refetched when the cache expires or a token names a `kid` it does not hold,
so tokens signed by a removed key verify until then; a static `Jwks` changes
only by building a new interceptor. `JwtAuthInterceptor` keeps no list of
revoked token ids.
