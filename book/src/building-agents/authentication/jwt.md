<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# JWT and OIDC

Accept tokens an identity provider signs (HS256, RS256 or ES256) with `JwtAuthInterceptor`. The caller's identity comes from the token's issuer and subject (`jwt:<issuer>:<sub>`; see [Naming the caller](./api-keys.md#naming-the-caller)). To require a scope on every call, see [Authorization](./authorization.md); the algorithm and secret-length rules are under [Security properties](../authentication.md#security-properties).

## What is checked

Enable the `auth-jwt` feature. `JwtAuthInterceptor` verifies the token's
signature, then its claims: `exp` must be present and in the future
(`allow_missing_exp()` lifts that), and `nbf` is honoured when present, both
with 60 s of clock-skew leeway (`with_leeway`). `iss` and `aud` are checked
**only when you set them** (`with_issuer`, `with_audience`); a validator with
neither accepts any correctly signed, unexpired token, so set both. Keys come
from a static `Jwks`, a shared HS256 secret, or a remote JWKS endpoint.

```toml
a2a-protocol-server = { version = "0.15", features = ["auth-jwt", "tls-rustls"] }
```

`tls-rustls` is needed to fetch JWKS or OIDC discovery documents over
`https://`; without it the fetch is plaintext-only. Static keys and HS256 need
only `auth-jwt`.

## Tokens from an OIDC issuer

`JwtAuthInterceptor::from_oidc_issuer` discovers the issuer's `jwks_uri` at
construction, fetches and caches the set on the first request, and refetches it
on key rotation:

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
cached (one hour by default). A token naming an unknown `kid` forces one
refetch. A fetch is bounded to 30 s and 256 KiB; if it fails, or the set holds
no usable key, the call is refused with a server error (not `401`), because no
verdict about the token could be reached. Only RSA keys and P-256 EC keys are
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
    JwtValidator::new().try_with_hs256_secret(secret)?,
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
