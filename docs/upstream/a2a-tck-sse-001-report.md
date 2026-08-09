<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Draft issue for `a2aproject/a2a-tck` — HTTP+JSON client crashes on any non-2xx streamed response

**Status: NOT FILED.** This is a prepared report awaiting a human decision to
submit it. Nothing in this file has been sent to `a2aproject/a2a-tck`.

Everything below was reproduced against pristine upstream at
`5996b79f9cefa6fc390980e383e358a66fb9e49e` (`main`, 2026-06-29,
*"fix: skip CARD-EXT-002 when extended card is configured (#186)"*) with
`httpx 0.28.1`. A search of that repo's issues found nothing covering it; the
nearest, #99 *"REST transport streaming fails with 'Event loop is closed'"*,
is a different failure.

Everything from `## Title` down is the proposed issue body, ready to paste.

---

## Title

`[Bug]: HTTP+JSON client raises httpx.ResponseNotRead for any non-2xx response to a streaming request`

## Body

### Summary

`tck/transport/http_json_client.py::_extract_error` raises
`httpx.ResponseNotRead` instead of returning an error string, for **any**
server that answers a streaming request with an HTTP status ≥ 400. The
requirement `HTTP_JSON-SSE-001` then errors instead of skipping.

This is reachable by any conformant implementation, because returning a
non-2xx to `POST /v1/message:stream` while `capabilities.streaming` is unset
is exactly what `CORE-CAP-002` requires. In the same run that produces this
error, `CORE-CAP-002` passes — the server is being marked correct for the
behaviour that crashes the harness.

### Root cause

Two locations, both in `tck/transport/http_json_client.py`.

`_request_streaming` opens the response as a stream and, on an error status,
closes it **without reading the body** (lines 184–195):

```python
response = self._client.send(request, stream=True)
resp_headers = dict(response.headers)
if response.status_code >= _HTTP_ERROR_MIN:
    response.close()            # <-- closed, never read
    return HttpJsonStreamingResponse(
        transport=self.transport,
        success=False,
        raw_response=response,
        ...
    )
```

`_extract_error` then calls `.json()` on that response (lines 44–52):

```python
try:
    body = response.json()
    ...
except (json.JSONDecodeError, ValueError):
    pass
return f"[{response.status_code}] {response.text}"
```

`httpx.ResponseNotRead` is **not** a subclass of either caught type — its MRO
is `ResponseNotRead → StreamError → RuntimeError` — so it escapes the
`except`. The fallback on the last line calls `.text`, which raises the same
exception for the same reason. **No path through `_extract_error` survives a
closed, unread, non-2xx streamed response.**

The trigger is `tests/compatibility/core_operations/test_transport_behavior.py:419`,
inside `TestRestStreaming::test_streaming_content_type`, which touches
`.error` while building the message for a skip it has already decided to take:

```python
response = client.send_streaming_message(message=_SAMPLE_MESSAGE)
if not response.success:
    pytest.skip(f"Streaming not supported: {response.error}")   # line 419
```

### Reproduction — no A2A SDK on either side

The server here is stdlib `http.server`; the client is the TCK's own
function. This isolates the defect from any implementation.

```python
import json, sys, threading
from http.server import BaseHTTPRequestHandler, HTTPServer
import httpx

sys.path.insert(0, ".")
from tck.transport.http_json_client import _extract_error, _HTTP_ERROR_MIN

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.dumps({"error": {"code": 9,
                                     "message": "streaming is not supported",
                                     "reason": "UNSUPPORTED_OPERATION"}}).encode()
        self.send_response(400)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *a): pass

srv = HTTPServer(("127.0.0.1", 0), Handler)
threading.Thread(target=srv.serve_forever, daemon=True).start()

client = httpx.Client(base_url=f"http://127.0.0.1:{srv.server_address[1]}")
request = client.build_request(
    "POST", "/v1/message:stream", json={"message": {}},
    headers={"Content-Type": "application/json", "Accept": "text/event-stream"})
response = client.send(request, stream=True)
response.close()                      # exactly http_json_client.py:187
print(_extract_error(response))       # exactly test_transport_behavior.py:419
```

Run from an `a2a-tck` checkout (`uv venv && uv pip install -e .`). Output:

```
httpx 0.28.1
ResponseNotRead MRO: ['ResponseNotRead', 'StreamError', 'RuntimeError', 'Exception', 'BaseException', 'object']
_extract_error catches: (json.JSONDecodeError, ValueError)
  issubclass(ResponseNotRead, ValueError)           = False
  issubclass(ResponseNotRead, json.JSONDecodeError) = False

status 400 >= 400 -> True
RESULT: raised httpx.ResponseNotRead: Attempted to access streaming response content, without having called `read()`.
```

### Reproduction via the suite

Against a server advertising no `capabilities.streaming`, on pristine
upstream:

```
$ ./.venv/bin/python -m pytest \
    "tests/compatibility/core_operations/test_transport_behavior.py::TestRestStreaming::test_streaming_content_type" \
    --sut-host=http://127.0.0.1:9997 -q

    @property
    def content(self) -> bytes:
        if not hasattr(self, "_content"):
>           raise ResponseNotRead()
E           httpx.ResponseNotRead: Attempted to access streaming response content, without having called `read()`.

httpx/_models.py:638: ResponseNotRead
FAILED .../TestRestStreaming::test_streaming_content_type
1 failed in 0.35s
```

### Expected behaviour

The test should skip with its intended message, and `_extract_error` should
return a string for any response it is handed.

### The sibling transport already does this correctly

This is not a design question — the JSON-RPC client, handling the same
situation on the same `httpx` streaming API, reads the body first:

```python
# tck/transport/jsonrpc_client.py:147-155
response = self._client.send(request, stream=True)
content_type = response.headers.get("content-type", "")
if "text/event-stream" not in content_type:
    # Server returned a plain JSON-RPC response (e.g. an immediate error)
    response.read()          # <-- reads before touching .json()/.text
    try:
        body = response.json()
    except Exception:
        body = response.text
```

```python
# tck/transport/http_json_client.py:184-187
response = self._client.send(request, stream=True)
resp_headers = dict(response.headers)
if response.status_code >= _HTTP_ERROR_MIN:
    response.close()         # <-- closes without reading; body is now gone
```

`jsonrpc_client` also catches bare `Exception` rather than
`(json.JSONDecodeError, ValueError)`, so it would survive this even without
the `read()`. The HTTP+JSON client is the odd one out on both counts; the fix
below simply brings it in line with its sibling.

### Suggested fix

Read the body before closing it, so `.json()` and `.text` both stay valid and
the real server error survives into the skip message —
`tck/transport/http_json_client.py`, in `_request_streaming`:

```diff
             if response.status_code >= _HTTP_ERROR_MIN:
+                # Read the body before closing: callers reach `.error` ->
+                # _extract_error(), whose .json()/.text both raise
+                # httpx.ResponseNotRead on a closed, unread streamed response.
+                try:
+                    response.read()
+                except httpx.StreamError:
+                    pass
                 response.close()
```

**Verified**: with that diff applied, the same command above gives

```
SKIPPED [1] tests/compatibility/core_operations/test_transport_behavior.py:419:
  Streaming not supported: [400] agent does not support streaming
  (AgentCard.capabilities.streaming is not true)
1 skipped in 0.39s
```

— the server's actual error text now reaches the skip message.

A defensive guard inside `_extract_error` also stops the crash, but only
after `close()` has already discarded the body, so the message degrades to
`[400] <response body unavailable>`. Both were tested; the call-site fix is
the better one. Hardening `_extract_error` as well would be reasonable
belt-and-braces, since it is reachable from other paths:

```diff
-    except (json.JSONDecodeError, ValueError):
+    except (json.JSONDecodeError, ValueError, httpx.StreamError):
         pass
```

(`httpx.StreamError` is the common base of `ResponseNotRead`,
`StreamConsumed` and `StreamClosed`.)

### Impact

- Any implementation that correctly rejects streaming when it is unadvertised
  hits this, so it is not specific to one SDK.
- `HTTP_JSON-SSE-001` reports an error rather than a skip, which reads as an
  implementation failure when it is a harness failure.
- It pushes SDK maintainers toward blanket CI waivers over the whole run to
  stay green, which is the opposite of what a conformance suite should
  incentivise.

### Environment

| | |
|---|---|
| `a2a-tck` | `5996b79f9cefa6fc390980e383e358a66fb9e49e` (`main`, 2026-06-29) |
| `httpx` | 0.28.1 |
| Python | 3.11.15 |
| SUT | any server returning ≥400 to `POST /v1/message:stream`; reproduced with stdlib `http.server` and with `tomtom215/a2a-rust`'s SUT |

**Re-verified 2026-08-06** against a fresh clone of `a2aproject/a2a-tck`
(`5996b79`, still `main` HEAD at that date), Python 3.11.15, httpx 0.28.1,
`pip install -e .`:

```
ResponseNotRead MRO: ['ResponseNotRead', 'StreamError', 'RuntimeError', 'Exception', 'BaseException', 'object']
  issubclass(ResponseNotRead, ValueError)           = False
  issubclass(ResponseNotRead, json.JSONDecodeError) = False
status 400 >= 400 -> True
RESULT: raised httpx.ResponseNotRead: Attempted to access streaming response content, without having called `read()`.
```

and with `response.read()` inserted before `response.close()`:

```
RESULT: returned '[400] streaming is not supported'
```

A GitHub issue search over `a2aproject/a2a-tck` for `ResponseNotRead`,
`SSE-001` and `streaming_content_type` returned no matching issues on
2026-08-06, so this appears not to be a duplicate — worth a second check at
filing time.

---

## Notes for the filer (not part of the issue body)

- The runnable script is `docs/upstream/repro_tck_sse_bug.py` in this
  repository.
- Full analysis, including why the reference-SDK comparator was unavailable
  (`a2a-tck`'s own reference SUT imports `a2a.server.apps`, absent from both
  `a2a-sdk` 1.1.2 and `a2aproject/a2a-python@main`), is in
  `docs/official-tck-findings.md` §17.
- This repository currently works around the bug with a single
  `pytest --deselect` of the affected test on the minimal-capability profile
  only, documented in `.github/workflows/official-tck.yml`. The requirement
  itself is still graded — it passes on the full profile — so no coverage is
  lost. If upstream fixes this, that `--deselect` should be removed.
