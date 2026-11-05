#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Checks each extension's conformance vectors against its JSON Schema (ADR 0049).

    python3 scripts/check_extension_vectors.py [crates/a2a-protocol-types/extensions]

Needs the `jsonschema` package. For every `*schema.json` beside a matching
`*vectors.json`: the schema must be a valid Draft 2020-12 schema, and every
valid `value`, and what it `reads` as, must satisfy it. Tolerated values (read,
but a sender must not write them) are not checked against it. An invalid value the schema accepts is listed, not failed: some rules (a
length in bytes, the 2^53 bound on a double) are beyond JSON Schema, and the
vectors file says which. The SDK's own reading of every vector is the Rust test
`a2a-protocol-types/tests/extension_vectors.rs`. Exit 1 on any failure.
"""

import glob
import json
import os
import sys

import jsonschema

ROOT = sys.argv[1] if len(sys.argv) > 1 else "crates/a2a-protocol-types/extensions"
failed = 0
for schema_path in sorted(glob.glob(f"{ROOT}/*/*schema.json")):
    vectors_path = schema_path.replace("schema.json", "vectors.json")
    name = os.path.relpath(vectors_path, ROOT)
    schema = json.load(open(schema_path))
    doc = json.load(open(vectors_path))
    jsonschema.Draft202012Validator.check_schema(schema)
    check = jsonschema.Draft202012Validator(schema)
    bad = [c["name"] for c in doc["valid"] if not check.is_valid(c["value"])]
    bad += [f"{c['name']} (reads)" for c in doc["valid"] if not check.is_valid(c["reads"])]
    beyond = [c["name"] for c in doc["invalid"] if check.is_valid(c["value"])]
    print(f"{name}: {len(doc['valid'])} valid, {len(doc['invalid'])} invalid, "
          f"{len(doc.get('tolerated', []))} tolerated")
    for b in bad:
        print(f"  FAIL  the schema refuses a valid value: {b}")
    for b in beyond:
        print(f"  note  the schema cannot refuse: {b}")
    failed += len(bad)
print(f"check_extension_vectors: {'FAIL' if failed else 'OK'} ({failed} failure(s))")
sys.exit(1 if failed else 0)
