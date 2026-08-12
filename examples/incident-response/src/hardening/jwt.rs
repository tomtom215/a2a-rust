// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Bearer tokens an identity provider signed, validated against its JWKS.

use super::Check;

const LABEL: &str = "JWT auth via JWKS (wrong signer and expired both refused)";

/// Validates ES256 tokens against a JWKS the example serves over HTTP.
///
/// The remote-JWKS path is the one a deployment actually uses — an issuer
/// publishes `jwks_uri`, the server fetches it, caches it, and refetches on a
/// key-id miss. A static [`Jwks`] would exercise none of that, so this serves a
/// real document from a loopback socket and points
/// [`JwtAuthInterceptor::from_jwks_url`] at it.
///
/// Three tokens, because a token being accepted proves almost nothing on its
/// own:
///
/// 1. **Valid** — signed by the published key, unexpired, right issuer and
///    audience. Must be accepted, or the check would pass against a server that
///    refuses everything.
/// 2. **Signed by a different key** — a well-formed ES256 token whose `kid`
///    matches but whose signature was made with a key the JWKS never published.
///    This is the forgery case; a validator that parses claims without
///    verifying the signature accepts it.
/// 3. **Expired** — signed by the *right* key, so only the `exp` claim
///    separates it from (1). A validator that verifies signatures but ignores
///    time accepts it, and a stolen token then works forever.
///
/// [`Jwks`]: a2a_protocol_server::auth::jwt::Jwks
/// [`JwtAuthInterceptor::from_jwks_url`]: a2a_protocol_server::auth::jwt::JwtAuthInterceptor::from_jwks_url
#[cfg(feature = "auth-jwt")]
pub(super) async fn jwt_auth() -> Check {
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use a2a_protocol_client::ClientBuilder;
    use a2a_protocol_server::auth::jwt::{JwtAuthInterceptor, JwtValidator};
    use a2a_protocol_server::builder::RequestHandlerBuilder;

    use super::{bind, is_refusal, plain_card, serve, HeaderInterceptor};
    use crate::agents::LogSearchExecutor;
    use crate::{send_params, user_message};

    const ISSUER: &str = "https://issuer.incident-response.invalid";
    const AUDIENCE: &str = "incident-triage";
    const KID: &str = "incident-signing-key";

    let signer = match EcKey::generate() {
        Ok(key) => key,
        Err(e) => return Check::fail(LABEL, format!("generating the signing key: {e}")),
    };
    // A second key that is never published. Its tokens carry the same `kid`, so
    // only the signature distinguishes them from real ones.
    let impostor = match EcKey::generate() {
        Ok(key) => key,
        Err(e) => return Check::fail(LABEL, format!("generating the impostor key: {e}")),
    };

    let jwks_url = serve_jwks(signer.jwks_document(KID)).await;
    let validator = JwtValidator::new()
        .with_issuer(ISSUER)
        .with_audience(AUDIENCE);

    let (listener, url) = bind().await;
    let handler = match RequestHandlerBuilder::new(LogSearchExecutor)
        .with_agent_card(plain_card(&url, "JWT Agent"))
        .with_interceptor(JwtAuthInterceptor::from_jwks_url(validator, jwks_url))
        .build()
    {
        Ok(handler) => Arc::new(handler),
        Err(e) => return Check::fail(LABEL, format!("building the handler: {e}")),
    };
    serve(listener, handler);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let claims = |exp: u64| {
        format!(
            r#"{{"iss":"{ISSUER}","aud":"{AUDIENCE}","sub":"oncall@example.com","iat":{iat},"exp":{exp}}}"#,
            iat = now.saturating_sub(60)
        )
    };

    // `JwtValidator` applies a 60s clock-skew leeway to `exp` by default, so a
    // token that expired a few seconds ago is *correctly* still accepted. This
    // one is an hour stale, which is unambiguously outside any sane leeway —
    // the first draft used 30 seconds and reported a defect that was the
    // check's own misreading of the contract.
    const WELL_PAST_LEEWAY: u64 = 3600;
    let cases = [
        ("a token signed by an unpublished key", &impostor, now + 300),
        (
            "an expired token",
            &signer,
            now.saturating_sub(WELL_PAST_LEEWAY),
        ),
    ];
    for (case, key, exp) in cases {
        let token = match key.sign_jwt(KID, &claims(exp)) {
            Ok(token) => token,
            Err(e) => return Check::fail(LABEL, format!("minting {case}: {e}")),
        };
        let client = match ClientBuilder::new(&url)
            .with_interceptor(HeaderInterceptor::new(
                "authorization",
                format!("Bearer {token}"),
            ))
            .build()
        {
            Ok(client) => client,
            Err(e) => return Check::fail(LABEL, format!("building the {case} client: {e}")),
        };
        match client
            .send_message(send_params(user_message("payments-api")))
            .await
        {
            Ok(_) => {
                return Check::fail(LABEL, format!("{case} was ACCEPTED"));
            }
            Err(e) if !is_refusal(&e) => {
                return Check::fail(
                    LABEL,
                    format!("the {case} call never reached the server: {e}"),
                )
            }
            Err(_) => {}
        }
    }

    // The valid token last, so a server that refuses everything cannot pass.
    let token = match signer.sign_jwt(KID, &claims(now + 300)) {
        Ok(token) => token,
        Err(e) => return Check::fail(LABEL, format!("minting the valid token: {e}")),
    };
    let client = match ClientBuilder::new(&url)
        .with_interceptor(HeaderInterceptor::new(
            "authorization",
            format!("Bearer {token}"),
        ))
        .build()
    {
        Ok(client) => client,
        Err(e) => return Check::fail(LABEL, format!("building the valid client: {e}")),
    };
    match client
        .send_message(send_params(user_message("payments-api")))
        .await
    {
        Ok(_) => Check::pass(
            LABEL,
            "ES256 token accepted from the published JWKS; forged and expired refused",
        ),
        Err(e) => Check::fail(LABEL, format!("a valid ES256 token was refused: {e}")),
    }
}

/// Serves a JWKS document at `/.well-known/jwks.json` and returns its URL.
#[cfg(feature = "auth-jwt")]
async fn serve_jwks(document: String) -> String {
    use std::sync::Arc;

    let (listener, url) = super::bind().await;
    let body = Arc::new(document);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let body = Arc::clone(&body);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |_req| {
                    let body = Arc::clone(&body);
                    async move {
                        let mut resp = hyper::Response::new(http_body_util::Full::new(
                            bytes::Bytes::from(body.as_str().to_owned()),
                        ));
                        resp.headers_mut().insert(
                            hyper::header::CONTENT_TYPE,
                            hyper::header::HeaderValue::from_static("application/json"),
                        );
                        Ok::<_, std::convert::Infallible>(resp)
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await;
            });
        }
    });
    format!("{url}/.well-known/jwks.json")
}

/// A throwaway P-256 key that can publish itself as a JWK and mint ES256 JWTs.
///
/// ES256's JWS signature is exactly the r‖s fixed-width form `ring` produces,
/// and a P-256 public key is `0x04 ‖ X ‖ Y`, so the JWK's `x`/`y` are the two
/// 32-byte halves — no ASN.1 involved in either direction.
#[cfg(feature = "auth-jwt")]
struct EcKey {
    pair: ring::signature::EcdsaKeyPair,
    public: Vec<u8>,
    rng: ring::rand::SystemRandom,
}

#[cfg(feature = "auth-jwt")]
impl EcKey {
    fn generate() -> Result<Self, String> {
        use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};

        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
            .map_err(|e| e.to_string())?;
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
            .map_err(|e| e.to_string())?;
        let public = pair.public_key().as_ref().to_vec();
        Ok(Self { pair, public, rng })
    }

    /// The JWKS document publishing this key under `kid`.
    fn jwks_document(&self, kid: &str) -> String {
        let (x, y) = self.coordinates();
        format!(
            r#"{{"keys":[{{"kty":"EC","crv":"P-256","use":"sig","alg":"ES256","kid":"{kid}","x":"{x}","y":"{y}"}}]}}"#
        )
    }

    /// The base64url-encoded affine coordinates of the public key.
    fn coordinates(&self) -> (String, String) {
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        // `0x04` marks the uncompressed point; the two 32-byte halves follow.
        (
            b64.encode(&self.public[1..33]),
            b64.encode(&self.public[33..]),
        )
    }

    /// Signs `claims` as a compact ES256 JWT.
    fn sign_jwt(&self, kid: &str, claims: &str) -> Result<String, String> {
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;

        let header = format!(r#"{{"alg":"ES256","typ":"JWT","kid":"{kid}"}}"#);
        let signing_input = format!("{}.{}", b64.encode(header), b64.encode(claims));
        let signature = self
            .pair
            .sign(&self.rng, signing_input.as_bytes())
            .map_err(|e| e.to_string())?;
        Ok(format!(
            "{signing_input}.{}",
            b64.encode(signature.as_ref())
        ))
    }
}

#[cfg(not(feature = "auth-jwt"))]
pub(super) async fn jwt_auth() -> Check {
    Check::skipped(LABEL, "auth-jwt")
}
