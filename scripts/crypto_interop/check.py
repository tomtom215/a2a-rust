#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

"""Reads what the SDK seals at rest with an independent implementation, and
seals values of its own for the SDK to open.

The independent implementation is the `cryptography` package (OpenSSL's
AES-256-GCM, HKDF-SHA-256 and HMAC-SHA-256). Nothing here imports or calls
the SDK: every check is computed from the layouts stated below, which were
read from the code (`store/encrypted/cipher.rs`, `store/encrypted/seal.rs`,
`push/cipher.rs`, `audit/privacy/`).

# The layouts

`ctx(xs)` is the JSON array `xs` serialised without whitespace, non-ASCII
characters as UTF-8 (serde_json's `to_string`; here `json.dumps` with
`separators=(",", ":")` and `ensure_ascii=False`).

1. **A ring value** (`Aes256GcmCipher`): one byte `n`, the key id (`n`
   bytes of UTF-8, 1 to 255), a 12-byte nonce, then AES-256-GCM ciphertext
   with its 16-byte tag. The associated data is the caller's context; the
   key id is not in it.
2. **A sealed task part**: the part, as JSON, is
   `{"data": {"sealedPart": {"v": 1, "c": B64(ring value)}},
   "mediaType": "application/vnd.a2a-rust.sealed-part+json"}`; the
   plaintext is the original part's JSON; the context is
   `ctx(["a2a-sealed-part/v1", partition, task_id, where, kind, id, index])`
   with `where` the string `"record"` or the log position (a number).
3. **A sealed push config**: the text `"a2a-sealed:v1:" + B64(ring value)`;
   the plaintext is the config's JSON; the context is
   `ctx(["a2a-push-config/v1", partition, task_id, config_id])`.
4. **A wrapped subject key**: `SubjectKey.wrapped` is `B64(ring value)` under
   the key-encryption key, sealing the person's 32-byte key under
   `ctx(["a2a-audit-subject-key/v1", chain, sid, lookup])`; `lookup` is
   `kid + ":" + HEX(HMAC-SHA-256(hmac_key, ctx(["a2a-audit-subject/v1",
   chain, subject])))`.
5. **A sealed audit value**: `"a2a-sealed:v1:" + sid + ":" + B64(nonce ||
   ciphertext || tag)`, AES-256-GCM under `HKDF-SHA-256(salt =
   "a2a-audit-sealed/v1", ikm = the person's key, info = C, L = 32)` with
   `C = ctx(["a2a-audit-sealed/v1", chain, sid, field, seq])` also the
   associated data; `field` is `actor`, `requestedBy`, `reason` or
   `actors/<i>`.
6. **A keyed digest**: `"hmac-sha256:" + kid + ":" + HEX(HMAC-SHA-256(
   hmac_key, ctx(["a2a-audit-digest/v1", chain, seq, name, plain])))` with
   `plain` the record's `"sha256:<hex>"` of the content's RFC 8785 JSON.

B64 is standard base64 with padding; HEX is lowercase.

# What it proves, and what it does not

That each layout above is exactly what the SDK wrote into
`rust_sealed.json`, that the plaintext and associated data are the stated
ones, and that the values are standard AES-256-GCM, HKDF and HMAC: a tag
verifies only under the stated key, nonce and associated data. Not that
nonces are unique across seals, not that keys are well kept, and nothing
about the primitives' strength. `--generate` writes `python_sealed.json`
with this script's own random nonces, subject handles and person keys;
`tests/crypto_interop.rs` opens it through the SDK's read paths.

Usage:
    python3 scripts/crypto_interop/check.py            # check both fixtures
    python3 scripts/crypto_interop/check.py --generate # rewrite python_sealed.json

Exit codes: 0 every check passed; 1 a check failed.
"""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
import os
import sys
from pathlib import Path

from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.kdf.hkdf import HKDF

FIXTURES = (
    Path(__file__).resolve().parents[2]
    / "crates/a2a-protocol-server/tests/fixtures/crypto"
)
SEALED_MEDIA_TYPE = "application/vnd.a2a-rust.sealed-part+json"
PREFIX = "a2a-sealed:v1:"
NONCE_LEN = 12
TAG_LEN = 16


class Failed(Exception):
    pass


def check(cond: bool, what: str) -> None:
    if not cond:
        raise Failed(what)


def ctx(xs: list) -> bytes:
    return json.dumps(xs, separators=(",", ":"), ensure_ascii=False).encode()


def b64decode(s: str) -> bytes:
    raw = base64.b64decode(s, validate=True)
    check(base64.b64encode(raw).decode() == s, f"non-canonical base64 {s!r}")
    return raw


def b64(raw: bytes) -> str:
    return base64.b64encode(raw).decode()


def ring(keys: list) -> dict[str, bytes]:
    return {k["id"]: bytes.fromhex(k["key"]) for k in keys}


# 1. A ring value.
def open_ring_value(keys: dict[str, bytes], value: bytes, aad: bytes) -> bytes:
    check(len(value) >= 1, "empty ring value")
    n = value[0]
    check(1 <= n and len(value) >= 1 + n + NONCE_LEN + TAG_LEN, "ring value too short")
    kid = value[1 : 1 + n].decode("utf-8")
    check(kid in keys, f"unknown key id {kid!r}")
    nonce = value[1 + n : 1 + n + NONCE_LEN]
    return AESGCM(keys[kid]).decrypt(nonce, value[1 + n + NONCE_LEN :], aad)


def seal_ring_value(keys: list, plaintext: bytes, aad: bytes) -> bytes:
    kid, key = keys[0]["id"], bytes.fromhex(keys[0]["key"])
    nonce = os.urandom(NONCE_LEN)
    kid_bytes = kid.encode()
    return bytes([len(kid_bytes)]) + kid_bytes + nonce + AESGCM(key).encrypt(nonce, plaintext, aad)


def hmac_hex(key: bytes, xs: list) -> str:
    return hmac.new(key, ctx(xs), hashlib.sha256).hexdigest()


def value_key(dek: bytes, c: bytes) -> bytes:
    return HKDF(algorithm=hashes.SHA256(), length=32, salt=b"a2a-audit-sealed/v1", info=c).derive(dek)


def jcs_digest(content) -> str:
    # RFC 8785 for the fixtures' content (objects, strings, small integers),
    # where it coincides with sorted keys and no whitespace.
    text = json.dumps(content, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return "sha256:" + hashlib.sha256(text.encode()).hexdigest()


def part_aad(g: dict, i: int) -> bytes:
    return ctx(["a2a-sealed-part/v1", g["partition"], g["task_id"], g["where"], g["kind"], g["id"], i])


def push_aad(e: dict) -> bytes:
    return ctx(["a2a-push-config/v1", e["partition"], e["task_id"], e["id"]])


def check_fixture(fx: dict) -> int:
    n = 0
    store = ring(fx["keys"]["store"])
    for e in fx["cipher"]:
        got = open_ring_value(store, bytes.fromhex(e["envelope_hex"]), e["aad"].encode())
        check(got.hex() == e["plaintext_hex"], f"cipher {e['name']}")
        n += 1
    for g in fx["store_parts"]:
        for i, part in enumerate(g["stored"]):
            check(set(part) == {"data", "mediaType"}, f"{g['name']}: members {sorted(part)}")
            check(part["mediaType"] == SEALED_MEDIA_TYPE, f"{g['name']}: media type")
            sealed = part["data"]["sealedPart"]
            check(set(part["data"]) == {"sealedPart"} and set(sealed) == {"v", "c"}, "sealed form")
            check(sealed["v"] == 1, "version")
            aad = part_aad(g, i)
            if "aad" in g:
                check(aad == g["aad"][i].encode(), f"{g['name']}[{i}]: AAD {aad!r}")
            plain = open_ring_value(store, b64decode(sealed["c"]), aad)
            if "plaintext_json" in g:
                check(plain == g["plaintext_json"][i].encode(), f"{g['name']}[{i}]: bytes")
            check(json.loads(plain) == g["plaintext"][i], f"{g['name']}[{i}]: part")
            n += 1
    for e in fx["push"]:
        check(e["stored"].startswith(PREFIX), "push prefix")
        plain = open_ring_value(store, b64decode(e["stored"][len(PREFIX) :]), push_aad(e))
        check(plain == e["plaintext_json"].encode(), "push plaintext")
        n += 1
    return n + check_audit(fx)


def check_audit(fx: dict) -> int:
    a, n = fx["audit"], 0
    kek = ring(fx["keys"]["kek"])
    kid, hk = fx["keys"]["hmac"]["id"], bytes.fromhex(fx["keys"]["hmac"]["key"])
    chain, record = a["chain"], a["record"]
    seq = record["seq"]
    by_lookup = {f"{kid}:{hmac_hex(hk, ['a2a-audit-subject/v1', chain, s])}": s for s in a["subjects"]}
    deks, owner = {}, {}
    for k in a["subject_keys"]:
        check(k["lookup"] in by_lookup, f"lookup {k['lookup']} is no subject's")
        aad = ctx(["a2a-audit-subject-key/v1", chain, k["sid"], k["lookup"]])
        dek = open_ring_value(kek, b64decode(k["wrapped"]), aad)
        check(len(dek) == 32, "a person's key is 32 bytes")
        deks[k["sid"]], owner[k["sid"]] = dek, by_lookup[k["lookup"]]
        n += 1
    names = a["names"]
    values = [("actor", record["actor"]["subject"], names["actor"], names["actor"])]
    values.append(("requestedBy", record["detail"]["requestedBy"], names["requestedBy"], names["requestedBy"]))
    values.append(("reason", record["detail"]["reason"], names["reason"], names["actor"]))
    for i, v in enumerate(record["detail"]["actors"]):
        values.append((f"actors/{i}", v, names["actors"][i], names["actors"][i]))
    for field, value, want, whose in values:
        check(value.startswith(PREFIX), f"{field}: prefix")
        sid, body = value[len(PREFIX) :].split(":", 1)
        check(owner.get(sid) == whose, f"{field}: sealed under {owner.get(sid)!r}, not {whose!r}")
        raw = b64decode(body)
        c = ctx(["a2a-audit-sealed/v1", chain, sid, field, seq])
        plain = AESGCM(value_key(deks[sid], c)).decrypt(raw[:NONCE_LEN], raw[NONCE_LEN:], c)
        check(plain.decode() == want, f"{field}: {plain!r}")
        n += 1
    for name, content in a["digest_contents"].items():
        plain = jcs_digest(content)
        want = f"hmac-sha256:{kid}:{hmac_hex(hk, ['a2a-audit-digest/v1', chain, seq, name, plain])}"
        check(record["digests"][name] == want, f"digest {name}")
        n += 1
    return n


def generate(rust: dict) -> dict:
    keys = rust["keys"]
    store, old = keys["store"], [keys["store"][1]]
    cipher = []
    for name, by, plaintext, aad in [
        ("short", store, b"hello from python", "ctx"),
        ("empty", store, b"", ""),
        ("non-ASCII context", store, bytes(range(256)), '["é",1]'),
        ("sealed by the older key", old, b"rotated", "ctx"),
    ]:
        env = seal_ring_value(by, plaintext, aad.encode())
        cipher.append({"name": name, "aad": aad, "plaintext_hex": plaintext.hex(), "envelope_hex": env.hex()})
    groups = []
    for g in rust["store_parts"]:
        g = {k: g[k] for k in ("name", "partition", "task_id", "where", "kind", "id")}
        g["plaintext"] = [{"text": f"python {g['name']}, é"}, {"data": {"from": "python"}, "mediaType": "application/json"}]
        g["stored"] = [
            {"data": {"sealedPart": {"v": 1, "c": b64(seal_ring_value(store, json.dumps(p).encode(), part_aad(g, i)))}},
             "mediaType": SEALED_MEDIA_TYPE}
            for i, p in enumerate(g["plaintext"])
        ]
        groups.append(g)
    push_cfg = {"taskId": "task-p", "id": "cfg-p", "url": "https://hooks.example.com/py", "token": "py-token"}
    push = {"partition": "", "task_id": "task-p", "id": "cfg-p", "plaintext_json": json.dumps(push_cfg)}
    push["stored"] = PREFIX + b64(seal_ring_value(store, push["plaintext_json"].encode(), push_aad(push)))
    return {
        "about": "Sealed by scripts/crypto_interop/check.py --generate with cryptography. Test keys only.",
        "keys": keys,
        "cipher": cipher,
        "store_parts": groups,
        "push": [push],
        "audit": generate_audit(keys),
    }


def generate_audit(keys: dict) -> dict:
    kid, hk = keys["hmac"]["id"], bytes.fromhex(keys["hmac"]["key"])
    chain, seq = "py-chain", 5
    names = {"actor": "carol@example.test", "requestedBy": "dave@example.test",
             "reason": "python's reason, é", "actors": ["agent-py", "carol@example.test"]}
    subject_keys, sids, deks = [], {}, {}
    for s in ["carol@example.test", "dave@example.test", "agent-py"]:
        sid, dek = os.urandom(16).hex(), os.urandom(32)
        lookup = f"{kid}:{hmac_hex(hk, ['a2a-audit-subject/v1', chain, s])}"
        aad = ctx(["a2a-audit-subject-key/v1", chain, sid, lookup])
        subject_keys.append({"lookup": lookup, "sid": sid, "wrapped": b64(seal_ring_value(keys["kek"], dek, aad))})
        sids[s], deks[s] = sid, dek

    def seal(field: str, whose: str, text: str) -> str:
        sid = sids[whose]
        c = ctx(["a2a-audit-sealed/v1", chain, sid, field, seq])
        nonce = os.urandom(NONCE_LEN)
        return f"{PREFIX}{sid}:{b64(nonce + AESGCM(value_key(deks[whose], c)).encrypt(nonce, text.encode(), c))}"

    content = {"text": "from python", "n": 2}
    digest = hmac_hex(hk, ["a2a-audit-digest/v1", chain, seq, "message", jcs_digest(content)])
    record = {
        "schema": "a2a-audit/1", "chain": chain, "seq": seq, "time": "2026-10-21T00:00:00.000Z", "kind": "call",
        "actor": {"subject": seal("actor", names["actor"], names["actor"]), "scheme": "bearer"},
        "detail": {
            "requestedBy": seal("requestedBy", names["requestedBy"], names["requestedBy"]),
            "reason": seal("reason", names["actor"], names["reason"]),
            "actors": [seal(f"actors/{i}", a, a) for i, a in enumerate(names["actors"])],
        },
        "digests": {"message": f"hmac-sha256:{kid}:{digest}"},
        "hash": "",
    }
    return {"chain": chain, "record": record, "names": names, "subjects": list(sids),
            "digest_contents": {"message": content}, "subject_keys": subject_keys}


def load(name: str) -> dict:
    return json.loads((FIXTURES / name).read_text(encoding="utf-8"))


def main() -> int:
    rust = load("rust_sealed.json")
    if "--generate" in sys.argv[1:]:
        out = json.dumps(generate(rust), indent=2, ensure_ascii=False, sort_keys=True) + "\n"
        (FIXTURES / "python_sealed.json").write_text(out, encoding="utf-8")
    try:
        for name in ("rust_sealed.json", "python_sealed.json"):
            print(f"{name}: {check_fixture(load(name))} values opened and checked")
    except Exception as e:  # noqa: BLE001 - every failure is reported as one
        print(f"FAILED: {type(e).__name__}: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
