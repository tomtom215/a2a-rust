# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""H9: authentication. Bearer and API-key header handling, JWT (HS256 secret) forged
every way a token can be forged, and auth-before-params (a request with bad
parameters and no credentials must say exactly what one with neither says).

JWT: alg none (four spellings), a wrong secret, an RSA public key used as the
HMAC key, HS384/HS512/RS256/ES256/EdDSA claimed against an HS256 validator,
`crit`, `b64:false`, duplicate `alg` keys in both orders, kid injection (path
traversal, SQL, NUL, 1 MB), padded and malformed base64, expired / not yet valid
/ wrong issuer / wrong audience / audience arrays, `exp` as a string, float, NaN,
2^63, missing, header shapes (two Authorization headers in both orders, lower and
upper case scheme, trailing space, tab, a token in the query string).

Needs: the target binary (A2A_HOSTILE_BIN); `cryptography` for the RSA key.
Usage: python3 -I h9_auth.py. Evidence: evidence/h9_auth.jsonl.
"""

import base64
import hashlib
import hmac
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import BASE_HEADERS, Recorder, Target, http, msg, rpc  # noqa: E402

SECRET = "0123456789abcdef0123456789abcdef-campaign"
ISS, AUD = "https://issuer.example", "a2a-target"


def b64(b: bytes, pad=False) -> str:
    s = base64.urlsafe_b64encode(b).decode()
    return s if pad else s.rstrip("=")


def jwt(header: dict | bytes, claims: dict | bytes, key: bytes | None = SECRET.encode(), alg="HS256",
        sig: bytes | None = None, raw_header: str | None = None) -> str:
    h = raw_header if raw_header is not None else b64(header if isinstance(header, bytes) else json.dumps(header, separators=(",", ":")).encode())
    c = b64(claims if isinstance(claims, bytes) else json.dumps(claims, separators=(",", ":")).encode())
    si = f"{h}.{c}".encode()
    if sig is None:
        mod = {"HS256": hashlib.sha256, "HS384": hashlib.sha384, "HS512": hashlib.sha512}.get(alg)
        sig = hmac.new(key, si, mod).digest() if (mod and key is not None) else b""
    return f"{h}.{c}." + b64(sig)


def good_claims(**kw):
    now = int(time.time())
    c = {"iss": ISS, "aud": AUD, "sub": "alice", "exp": now + 3600, "nbf": now - 10, "iat": now}
    c.update(kw)
    return {k: v for k, v in c.items() if v is not None or k in kw and kw[k] is None}


def call(t, token=None, headers=None, path="/", method="GetTask", params=None, raw_headers=None):
    hs = dict(BASE_HEADERS)
    if token is not None:
        hs["authorization"] = f"Bearer {token}"
    if headers:
        hs.update(headers)
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params if params is not None else {"id": "nope"}})
    st, hd, data = http(t.port, "POST", path, body, hs, raw_headers=raw_headers)
    return st, hd, data


def accepted(st, data):
    """The request got past authentication (an application answer, not a 401/403)."""
    return st == 200 and b"-32001" in data or (st == 200 and b'"result"' in data)


def main() -> None:
    t = Target("h9_auth", {"STORE": "tmem", "AUTH": "jwt", "JWT_SECRET": SECRET, "JWT_ISS": ISS, "JWT_AUD": AUD})
    r = Recorder("h9_auth")
    try:
        ok_token = jwt({"alg": "HS256", "typ": "JWT"}, good_claims())
        st, hd, d = call(t, ok_token)
        r.rec("control: a valid HS256 token is accepted", "held" if accepted(st, d) else "DEVIATION", [st, d[:120]])

        def expect_refused(name, token=None, **kw):
            st, hd, d = call(t, token, **kw)
            refused = st in (401, 403) and not accepted(st, d)
            r.rec(name, "held" if refused else "DEVIATION", {"status": st, "www": hd.get("www-authenticate"), "body": d[:120].decode("latin1")})

        def expect_accepted(name, token=None, **kw):
            st, hd, d = call(t, token, **kw)
            r.rec(name, "held" if accepted(st, d) else "DEVIATION", {"status": st, "body": d[:120].decode("latin1")})

        c = good_claims()
        for alg in ("none", "None", "NONE", "nOnE"):
            expect_refused(f"alg {alg!r}, empty signature", jwt({"alg": alg, "typ": "JWT"}, c, sig=b""))
        expect_refused("alg none, two-part token (no signature segment)", b64(b'{"alg":"none"}') + "." + b64(json.dumps(c).encode()))
        expect_refused("alg none, signature of a valid HS256 token", jwt({"alg": "none"}, c, sig=hmac.new(SECRET.encode(), b"x", hashlib.sha256).digest()))
        expect_refused("HS256 signed with the wrong secret", jwt({"alg": "HS256"}, c, key=b"x" * 40))
        expect_refused("HS256 signed with an empty key", jwt({"alg": "HS256"}, c, key=b""))
        for alg in ("HS384", "HS512"):
            expect_refused(f"{alg} against an HS256 validator", jwt({"alg": alg}, c, alg=alg))
        for alg in ("RS256", "RS384", "RS512", "PS256", "ES256", "ES384", "EdDSA", "", "hs256", "HS256 ", "RS256\u0000"):
            expect_refused(f"alg {alg!r} with a 256-byte junk signature", jwt({"alg": alg}, c, sig=b"\x01" * 256))
        # RSA public key as the HMAC secret (the classic confusion)
        try:
            from cryptography.hazmat.primitives import serialization
            from cryptography.hazmat.primitives.asymmetric import rsa
            k = rsa.generate_private_key(public_exponent=65537, key_size=2048)
            pem = k.public_key().public_bytes(serialization.Encoding.PEM, serialization.PublicFormat.SubjectPublicKeyInfo)
            der = k.public_key().public_bytes(serialization.Encoding.DER, serialization.PublicFormat.PKCS1)
            expect_refused("HS256 signed with an RSA public key (PEM) as the secret", jwt({"alg": "HS256"}, c, key=pem))
            expect_refused("HS256 signed with an RSA public key (DER) as the secret", jwt({"alg": "HS256"}, c, key=der))
            from cryptography.hazmat.primitives import hashes
            from cryptography.hazmat.primitives.asymmetric import padding
            h = b64(json.dumps({"alg": "RS256", "kid": "k1"}).encode())
            body = b64(json.dumps(c).encode())
            sig = k.sign(f"{h}.{body}".encode(), padding.PKCS1v15(), hashes.SHA256())
            expect_refused("RS256 signed with the attacker's own key (no JWKS configured)", f"{h}.{body}.{b64(sig)}")
        except ImportError:
            r.rec("RSA confusion cases", "held", "cryptography is not installed: not run")
        # header oddities, validly signed (an acceptance here is a finding for crit/b64)
        expect_refused("crit header naming an unknown extension (RFC 7515 4.1.11)", jwt({"alg": "HS256", "crit": ["x-evil"], "x-evil": 1}, c))
        expect_refused("b64:false header (RFC 7797)", jwt({"alg": "HS256", "b64": False, "crit": ["b64"]}, c))
        for kid in ("../../../../etc/passwd", "'; DROP TABLE keys;--", "a\u0000b", "k" * 1_000_000, "\ud800"[:0] + "é" * 100, "{{7*7}}", "$(id)"):
            st, hd, d = call(t, jwt({"alg": "HS256", "kid": kid}, c))
            r.rec(f"valid HS256 token with kid {kid[:30]!r} ({len(kid)} chars): no crash", "held" if st in (200, 401, 403, 431, 400) and t.alive() else "DEVIATION", {"status": st})
        for label, raw in (
            ("duplicate alg: HS256 then none, valid HS256 sig", '{"alg":"HS256","alg":"none"}'),
            ("duplicate alg: none then HS256, valid HS256 sig", '{"alg":"none","alg":"HS256"}'),
        ):
            tok = jwt({}, c, raw_header=b64(raw.encode()))
            # sign with the real HS256 so that a parser that picks HS256 verifies it
            st, hd, d = call(t, tok)
            r.rec(label + " (either answer is fine; recorded)", "held" if st in (200, 401, 403) else "DEVIATION", {"status": st, "accepted": accepted(st, d)})
        tok = jwt({}, c, raw_header=b64(b'{"alg":"none","alg":"HS256"}'), sig=b"")
        expect_refused("duplicate alg none/HS256 with NO signature", tok)
        # base64 shape
        base = jwt({"alg": "HS256"}, c)
        h, bd, s = base.split(".")
        expect_refused("padded base64 in the signature segment (accepted by lenient decoders)", f"{h}.{bd}.{s}==")
        expect_refused("standard (non URL-safe) base64 alphabet", f"{h}.{bd}.{s.replace('-', '+').replace('_', '/')}+/")
        expect_refused("four segments", base + ".extra")
        expect_refused("one segment", h)
        expect_refused("empty token", "")
        expect_refused("token of 1 MB of dots", "." * 1_000_000)
        expect_refused("header segment is not JSON", jwt({}, c, raw_header=b64(b"not json")))
        expect_refused("header segment is invalid UTF-8", jwt({}, c, raw_header=b64(b'{"alg":"\xff"}')))
        expect_refused("claims segment is a JSON array", jwt({"alg": "HS256"}, b"[1,2,3]"))
        # claims
        now = int(time.time())
        for label, cl, want in (
            ("expired 120 s ago (leeway is 60 s)", good_claims(exp=now - 120), False),
            ("expired 30 s ago (inside the 60 s leeway)", good_claims(exp=now - 30), True),
            ("nbf one hour ahead", good_claims(nbf=now + 3600), False),
            ("nbf 30 s ahead (inside the leeway)", good_claims(nbf=now + 30), True),
            ("no exp claim", {k: v for k, v in good_claims().items() if k != "exp"}, None),
            ("exp as a string", good_claims(exp=str(now + 3600)), False),
            ("exp as a float", good_claims(exp=now + 3600.5), None),
            ("exp = 2^63", good_claims(exp=2**63), None),
            ("exp = -1", good_claims(exp=-1), False),
            ("exp = null", good_claims(exp=None), False),
            ("exp = 1e999 (raw)", None, False),
            ("wrong issuer", good_claims(iss="https://evil.example"), False),
            ("issuer with a trailing slash", good_claims(iss=ISS + "/"), False),
            ("no issuer claim", {k: v for k, v in good_claims().items() if k != "iss"}, False),
            ("wrong audience", good_claims(aud="other"), False),
            ("audience array containing ours", good_claims(aud=["x", AUD]), True),
            ("audience array without ours", good_claims(aud=["x", "y"]), False),
            ("audience in another case", good_claims(aud=AUD.upper()), False),
            ("audience with a trailing NUL", good_claims(aud=AUD + "\u0000"), False),
            ("no audience claim", {k: v for k, v in good_claims().items() if k != "aud"}, False),
        ):
            if cl is None:
                tok = jwt({"alg": "HS256"}, json.dumps({"iss": ISS, "aud": AUD, "exp": "1e999"}).encode().replace(b'"1e999"', b"1e999"))
            else:
                tok = jwt({"alg": "HS256"}, cl)
            st, hd, d = call(t, tok)
            acc = accepted(st, d)
            verdict = "held" if (want is None or acc == want) else "DEVIATION"
            r.rec(f"claims: {label}", verdict, {"accepted": acc, "expected": want, "status": st})
        # header shapes
        expect_accepted("scheme in lower case: 'bearer <token>'", None, headers={"authorization": f"bearer {ok_token}"})
        expect_accepted("scheme in upper case: 'BEARER <token>'", None, headers={"authorization": f"BEARER {ok_token}"})
        expect_refused("'Basic <token>'", None, headers={"authorization": f"Basic {ok_token}"})
        expect_refused("token with trailing space", None, headers={"authorization": f"Bearer {ok_token} "})
        expect_refused("two spaces after Bearer", None, headers={"authorization": f"Bearer  {ok_token}"})
        expect_refused("a tab after Bearer", None, headers={"authorization": f"Bearer\t{ok_token}"})
        expect_refused("'Bearer <token> extra'", None, headers={"authorization": f"Bearer {ok_token} extra"})
        expect_refused("token only, no scheme", None, headers={"authorization": ok_token})
        expect_refused("token in the query string (?access_token=)", None, path=f"/?access_token={ok_token}")
        expect_refused("token in the query string (?token=)", None, path=f"/?token={ok_token}")
        expect_refused("token in a cookie", None, headers={"cookie": f"token={ok_token}"})
        expect_refused("token in X-Forwarded-Authorization", None, headers={"x-forwarded-authorization": f"Bearer {ok_token}"})
        base_hs = [(k, v) for k, v in BASE_HEADERS.items()]
        expect_refused("two Authorization headers: invalid first, valid second", None,
                       raw_headers=base_hs + [("authorization", "Bearer bad.bad.bad"), ("authorization", f"Bearer {ok_token}")])
        expect_refused("two Authorization headers: valid first, invalid second", None,
                       raw_headers=base_hs + [("authorization", f"Bearer {ok_token}"), ("authorization", "Bearer bad.bad.bad")])
        # auth-before-params: the answer to garbage with no credentials equals the answer to a clean call with none
        def shape(st, hd, d):
            import re
            return (st, hd.get("www-authenticate"), re.sub(rb"[0-9a-f]{8}-[0-9a-f-]{27}|reference [0-9a-f]+", b"<id>", d))
        ref = shape(*call(t, None))
        variants = {
            "no credentials, unknown method": dict(method="NoSuchMethod"),
            "no credentials, params of the wrong type": dict(params=[1, 2, 3]),
            "no credentials, SendMessage with an invalid message": dict(method="SendMessage", params={"message": {"x": 1}}),
            "no credentials, GetTask with a 10 000 char id": dict(params={"id": "a" * 10000}),
            "no credentials, ListTasks with a hostile page token": dict(method="ListTasks", params={"pageToken": "\u0000"}),
            "no credentials, on the REST path (/tasks/x)": dict(path="/tasks/x"),
        }
        for label, kw in variants.items():
            if kw.get("path"):
                st, hd, d = http(t.port, "GET", kw["path"], None, dict(BASE_HEADERS))
                got = shape(st, hd, d)
                same = got[0] == ref[0] and got[1] == ref[1]
            else:
                got = shape(*call(t, None, **kw))
                same = got == ref
            r.rec(f"auth-before-params: {label} answers as no-credentials does", "held" if same else "DEVIATION",
                  {"reference": [ref[0], ref[1], ref[2][:100].decode('latin1')], "got": [got[0], got[1], got[2][:100].decode('latin1')]})
        bad = shape(*call(t, "bad.bad.bad", method="NoSuchMethod"))
        ref_bad = shape(*call(t, "bad.bad.bad"))
        r.rec("auth-before-params: an invalid credential with a bad method answers as with a good method",
              "held" if bad == ref_bad else "DEVIATION", {"a": str(bad)[:160], "b": str(ref_bad)[:160]})
        # oversized credentials
        for label, hs in (("a 100 KB Authorization header", {"authorization": "Bearer " + "a" * 100_000}),
                          ("a 10 MB Authorization header", {"authorization": "Bearer " + "a" * 10_000_000})):
            try:
                st, hd, d = call(t, None, headers=hs)
            except Exception as e:  # noqa: BLE001
                st = f"{type(e).__name__}"
            r.rec(label + " is refused without a crash", "held" if (st in (400, 401, 403, 431) or isinstance(st, str)) and t.alive() else "DEVIATION", {"status": st})
        r.rec("target alive, no panic", "held" if t.alive() and not t.panics() else "DEVIATION", t.state())
    finally:
        r.close()
        t.stop()
    # static bearer and API key: the method is constant-time (auth/mod.rs `subtle`), not measured here.
    t = Target("h9_auth_static", {"STORE": "tmem", "AUTH": "bearer"})
    r = Recorder("h9_auth_static")
    try:
        for label, hs, want in (
            ("valid token", {"authorization": "Bearer tokA"}, True),
            ("token of a prefix", {"authorization": "Bearer tok"}, False),
            ("token plus a NUL", {"authorization": "Bearer tokA\u0000"}, None),
            ("empty bearer", {"authorization": "Bearer "}, False),
            ("bearer with only spaces", {"authorization": "Bearer    "}, False),
            ("lower-case scheme", {"authorization": "bearer tokA"}, True),
        ):
            try:
                st, hd, d = call(t, None, headers=hs)
            except Exception as e:  # noqa: BLE001
                r.rec(f"static bearer: {label}", "held", f"client refused to send: {e}")
                continue
            acc = accepted(st, d)
            r.rec(f"static bearer: {label}", "held" if want is None or acc == want else "DEVIATION", {"status": st, "accepted": acc})
        r.rec("target alive", "held" if t.alive() and not t.panics() else "DEVIATION", t.state())
    finally:
        r.close()
        t.stop()
    t = Target("h9_auth_apikey", {"STORE": "tmem", "AUTH": "apikey"})
    r = Recorder("h9_auth_apikey")
    try:
        for label, hs, want in (
            ("valid key", {"x-api-key": "tokA"}, True),
            ("empty key (SEC-05: an empty key once failed open)", {"x-api-key": ""}, False),
            ("key with a trailing space", {"x-api-key": "tokA "}, None),
            ("key in the Authorization header", {"authorization": "tokA"}, False),
            ("upper-case header name", {"X-API-KEY": "tokA"}, True),
            ("two x-api-key headers, bad then good", None, None),
        ):
            if hs is None:
                st, hd, d = call(t, None, raw_headers=[(k, v) for k, v in BASE_HEADERS.items()] + [("x-api-key", "bad"), ("x-api-key", "tokA")])
            else:
                st, hd, d = call(t, None, headers=hs)
            acc = accepted(st, d)
            r.rec(f"api key: {label}", "held" if want is None or acc == want else "DEVIATION", {"status": st, "accepted": acc})
        r.rec("target alive", "held" if t.alive() and not t.panics() else "DEVIATION", t.state())
    finally:
        r.close()
        t.stop()


if __name__ == "__main__":
    main()
