# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Agent Card signatures, both directions, against the official Python SDK.

Spec §8.4 makes a card signature portable only if every implementation
canonicalizes the card to the same bytes (§8.4.1). This SDK's tests checked
that it agreed with itself; nothing checked that it agreed with anyone else,
and until 2026-10-10 it did not: it kept default-valued fields (an empty
`extensions` list, an empty `tenant`) that §8.4.1 says MUST be omitted, so a
card carrying one verified here and nowhere that follows the spec.

For each card below, this signs with the official `a2a-sdk` and verifies with
`a2a card verify`, then signs with `a2a card sign` and verifies with the SDK.
Of the four official SDKs only Python implements card signing (a2a-go 2.5.0
and @a2a-js/sdk 1.1.0 have none), so it is the one peer there is.

The SDK and the specification disagree on one point, and the cards are chosen
to show it rather than hide it. `a2a.utils.signing._clean_empty` removes every
empty string, list and object, including `REQUIRED` fields that §8.4.1 says
MUST stay (`"description": ""`, `"skills": []`, an empty `capabilities`
object) and which the specification's own worked example keeps. This SDK
follows the specification. A card that has such a field therefore canonicalizes
differently in the two. The same function also drops a security requirement
whose scope list is empty, which is the ordinary bearer case: the SDK signs a
card without its `securityRequirements`. Both cases are listed as expected
divergences:
the run fails if it ever starts agreeing (the divergence was fixed upstream
and the expectation must go) as much as if a realistic card disagrees.

Usage:
    python3 itk/interop/card_signature_interop.py --cli target/debug/a2a
Needs `a2a-sdk[signing]` and `cryptography` importable. Exit 0 when every
outcome matches its expectation, 1 otherwise.
"""

from __future__ import annotations

import argparse
import base64
import json
import subprocess
import sys
import tempfile
from pathlib import Path

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ec
from google.protobuf.json_format import MessageToDict, ParseDict

from a2a.types import AgentCard
from a2a.utils.signing import create_agent_card_signer, create_signature_verifier

KID = "interop-es256"


def _b64(n: int) -> str:
    return base64.urlsafe_b64encode(n.to_bytes(32, "big")).rstrip(b"=").decode()


def interface(**extra: str) -> dict:
    return {
        "url": "https://agent.example.com/a2a",
        "protocolBinding": "JSONRPC",
        "protocolVersion": "1.0",
        **extra,
    }


SKILL = {
    "id": "triage",
    "name": "Triage",
    "description": "Sorts incoming incidents.",
    "tags": ["ops"],
}

BASE = {
    "name": "Interop Agent",
    "description": "An agent whose card two SDKs sign and verify.",
    "version": "1.0.0",
    "supportedInterfaces": [interface()],
    "capabilities": {"streaming": True, "pushNotifications": False},
    "defaultInputModes": ["text/plain"],
    "defaultOutputModes": ["text/plain"],
    "skills": [SKILL],
}

# (name, card, whether the two canonical forms are expected to agree)
CARDS: list[tuple[str, dict, bool]] = [
    ("realistic card", BASE, True),
    (
        "implicit fields at their defaults (the 2026-10-10 fix)",
        {
            **BASE,
            "supportedInterfaces": [interface(tenant="")],
            "capabilities": {
                "streaming": True,
                "extensions": [{"uri": "https://example.com/ext/v1", "required": False}],
            },
        },
        True,
    ),
    (
        "optional field explicitly false",
        {**BASE, "capabilities": {"streaming": False, "pushNotifications": False}},
        True,
    ),
    (
        "security requirement with scopes",
        {
            **BASE,
            "securitySchemes": {
                "oidc": {"openIdConnectSecurityScheme": {"openIdConnectUrl": "https://id.example.com"}}
            },
            "securityRequirements": [{"schemes": {"oidc": {"list": ["openid"]}}}],
        },
        True,
    ),
    (
        # The common bearer case. The SDK's `_clean_empty` turns the empty
        # StringList into nothing, then the requirement, then the list, so the
        # requirement is not in the bytes it signs at all and can be removed
        # from a signed card without breaking the signature.
        "scope-less bearer requirement (SDK leaves it out of the signed bytes)",
        {
            **BASE,
            "securitySchemes": {
                "bearer": {"httpAuthSecurityScheme": {"scheme": "Bearer", "bearerFormat": "JWT"}}
            },
            "securityRequirements": [{"schemes": {"bearer": {"list": []}}}],
        },
        False,
    ),
    (
        "REQUIRED description left empty (SDK strips it; §8.4.1 keeps it)",
        {**BASE, "description": ""},
        False,
    ),
]


def run(cli: str, args: list[str]) -> subprocess.CompletedProcess:
    return subprocess.run([cli, *args], capture_output=True, text=True, check=False)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--cli", required=True, help="path to the built `a2a` binary")
    cli = parser.parse_args().cli

    private = ec.generate_private_key(ec.SECP256R1())
    public = private.public_key()
    numbers = public.public_numbers()
    jwks = {
        "keys": [
            {
                "kty": "EC",
                "crv": "P-256",
                "x": _b64(numbers.x),
                "y": _b64(numbers.y),
                "kid": KID,
                "alg": "ES256",
                "use": "sig",
            }
        ]
    }
    pem_private = private.private_bytes(
        serialization.Encoding.PEM,
        serialization.PrivateFormat.PKCS8,
        serialization.NoEncryption(),
    )
    der_private = private.private_bytes(
        serialization.Encoding.DER,
        serialization.PrivateFormat.PKCS8,
        serialization.NoEncryption(),
    )
    pem_public = public.public_bytes(
        serialization.Encoding.PEM, serialization.PublicFormat.SubjectPublicKeyInfo
    )

    sdk_sign = create_agent_card_signer(
        signing_key=pem_private,
        protected_header={"alg": "ES256", "kid": KID, "typ": "JOSE"},
    )
    sdk_verify = create_signature_verifier(
        key_provider=lambda _kid, _jku: pem_public, algorithms=["ES256"]
    )

    failures = 0
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        (tmpdir / "jwks.json").write_text(json.dumps(jwks))
        (tmpdir / "key.der").write_bytes(der_private)

        for name, card, agree in CARDS:
            # Python signs, Rust verifies.
            signed = sdk_sign(ParseDict(card, AgentCard()))
            path = tmpdir / "py-signed.json"
            path.write_text(json.dumps(MessageToDict(signed)))
            rust_ok = run(cli, ["card", "verify", "--jwks", str(tmpdir / "jwks.json"), str(path)])
            py_to_rust = rust_ok.returncode == 0

            # Rust signs, Python verifies.
            path = tmpdir / "card.json"
            path.write_text(json.dumps(card))
            out = run(cli, ["card", "sign", "--key", str(tmpdir / "key.der"), "--alg", "es256",
                            "--kid", KID, str(path)])
            if out.returncode != 0:
                print(f"FAIL  {name}: a2a card sign exited {out.returncode}: {out.stderr.strip()}")
                failures += 1
                continue
            try:
                sdk_verify(ParseDict(json.loads(out.stdout), AgentCard()))
                rust_to_py = True
            except Exception:  # noqa: BLE001 - any verifier failure is a "no"
                rust_to_py = False

            ok = (py_to_rust, rust_to_py) == (agree, agree)
            verdict = "ok   " if ok else "FAIL "
            expectation = "agree" if agree else "diverge (expected)"
            print(
                f"{verdict} {name}: python->rust {'verifies' if py_to_rust else 'rejects'}, "
                f"rust->python {'verifies' if rust_to_py else 'rejects'}; expected to {expectation}"
            )
            if not ok:
                failures += 1
                if not py_to_rust and agree:
                    print("      a2a card verify said:", rust_ok.stdout.strip(), rust_ok.stderr.strip())

    print(f"\n{len(CARDS) - failures}/{len(CARDS)} cards matched their expectation")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
