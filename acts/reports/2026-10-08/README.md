<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ACTS reports, 2026-10-08

The official A2A conformance suite (ACTS, `a2aproject/a2a-itk` at
`82458ceac60208cc46ccbe52b68dbb8e1ca2614a`, that day's `main`) run against
this repository's ITK agent (`itk/`) over JSON-RPC, gRPC and HTTP+JSON, after
HTTP+JSON media-type negotiation was added.

| Binding | MUST | SHOULD | MAY | Not passed |
|---|---|---|---|---|
| JSON-RPC | 59/59 | 29/29 | 13/13 | none |
| gRPC | 47/47 | 28/28 | 13/13 | none |
| HTTP+JSON | 49/49 | 32/32 | 13/13 | none |

`REST-CT-001` ("REST responses use application/a2a+json content type"), the
one failure on every earlier run, passes.

**How it was run.** `itk/run_itk.sh`'s settings with `ITK_ACTS_RUN=1
ITK_ACTS_TRANSPORTS=jsonrpc,grpc,rest`, the a2a-itk checkout copied to
`itk/a2a-itk`, and host networking plus the proxy CA overlay from
`benches/sdk-comparison/scripts/itk_dockerfile_overlay.py`, because the
sandbox reaches the network through a TLS-re-terminating proxy. No test,
scenario or runner file was changed. The agent was built inside the
container from the uncommitted working tree that became the commit adding
this file, before three edits clippy asked for in
`dispatch/rest/media_type.rs`: `pub(crate)` items in a private module made
`pub`, `filter_map(..).next()` written as `find_map(..)`, and comment
wording. None changes behaviour; `rest_and_axum_negotiate_the_response_media_type`
pins the behaviour and passes on the committed code.

The `sdk.repository` field in each report reads
`https://github.com/a2aproject/a2a-rust`; that is the shared driver's default
for the `rust` line, not this repository's URL.

Tally reproduced from the JSON with:

```sh
python3 - acts-report-*.json <<'PY'
import collections, json, sys
for f in sys.argv[1:]:
    c = collections.Counter()
    def walk(o):
        if isinstance(o, dict):
            if {"id", "level", "result"} <= o.keys():
                c[(o["level"], o["result"])] += 1
                return
            for v in o.values(): walk(v)
        elif isinstance(o, list):
            for v in o: walk(v)
    walk(json.load(open(f)))
    print(f, dict(c))
PY
```
