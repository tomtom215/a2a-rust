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
its tokens in; that was the default until this release. A token whose header
has a `crit` member is refused: the validator understands no critical
extension, and RFC 7515 section 4.1.11 says to reject what it does not
understand. Keys come from a
static `Jwks`, a shared HS256 secret, or a remote JWKS endpoint. A remote
JWKS or discovery URL must be `https://`, or `http://` to a loopback host:
`localhost` or a loopback address such as `127.0.0.1` or `[::1]`. A name under
`.localhost` (`http://idp.localhost/`) is refused since 0.16.0: glibc and musl
send such a name to DNS unless nss-myhostname or systemd-resolved answers it,
so it is not guaranteed to stay on this host. Use `localhost` with a port, a
loopback address, or `https://`.

```toml
a2a-protocol-server = { version = "0.16", features = ["auth-jwt", "tls-rustls"] }
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
received. A fetch is bounded to 30 s (`with_jwks_fetch_budget`) and 256 KiB. If the first fetch fails, or
the set holds no usable key, the call is refused with a server error (not
`401`), because no verdict about the token could be reached. With the cache
cold or expired, **a failed fetch is shared**: the requests queued behind it are
given its error and fetch nothing until a cooldown of the refetch interval (plus
up to a fifth, jittered) has passed, so an identity provider that is down is asked
once and not once per request, no request waits longer than about one fetch budget,
and past 256 requests waiting for one refresh the rest are refused at once
(adversarial review R4, 2026-11-12); a refetch for an
unknown `kid` that fails leaves the keys in hand, and the token is refused
with `401`. Only RSA keys and P-256 EC keys are
loaded, and an RS256 key must be 2048 to 8192 bits; entries of another key
type are skipped, and so is any entry that is not for verifying signatures: a
`use` other than `"sig"`, or a `key_ops` that does not list `"verify"`. A key
that names an `alg` verifies only tokens that name it (RFC 8725 section 3.1), so
an RSA key that says `RS512` verifies nothing here, where only `RS256` is
accepted; a key that names none verifies the one algorithm its type fixes.

## Static keys and HS256

Supply a `Jwks` directly (from a JWKS JSON document or key by
key), or a shared secret for HS256:

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let jwks_bytes: &[u8] = br#"{"keys": []}"#;
# let key_from_your_secret_store: Vec<u8> = vec![0x5a; 32];
use a2a_protocol_server::auth::jwt::{Jwks, JwtAuthInterceptor, JwtValidator};

// RS256/ES256 from a JWKS document:
let jwks = Jwks::from_json(jwks_bytes)?;
let interceptor = JwtAuthInterceptor::new(
    JwtValidator::new().with_issuer("https://issuer").with_audience("agent"),
    jwks,
);

// HS256 shared secret (no JWKS): a random key of at least 32 bytes (RFC 7518
// §3.2), not a password (RFC 8725 §3.5). `openssl rand -base64 32` makes one;
// keep it in a secret store and decode it when you load it.
// `try_with_hs256_secret` refuses a shorter one here, at startup.
let secret = key_from_your_secret_store;
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
