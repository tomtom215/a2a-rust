# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""
Minimal reproduction: a2a-tck's HTTP+JSON client raises httpx.ResponseNotRead
when a server returns any non-2xx status to a streaming request.

No A2A SDK is involved on either side. The "server" is python's stdlib
http.server; the "client" is a2a-tck's own _extract_error, reached exactly as
TestRestStreaming::test_streaming_content_type reaches it.

Run:
    cd /path/to/a2a-tck && uv venv && uv pip install -e .
    ./.venv/bin/python repro_tck_sse_bug.py
"""
import json, sys, threading
from http.server import BaseHTTPRequestHandler, HTTPServer

import httpx

sys.path.insert(0, ".")  # import a2a-tck from a checkout
from tck.transport.http_json_client import _extract_error, _HTTP_ERROR_MIN


class Handler(BaseHTTPRequestHandler):
    """Any conformant server with streaming unadvertised must reject
    POST /v1/message:stream. CORE-CAP-002 requires exactly this."""

    def do_POST(self):
        body = json.dumps(
            {"error": {"code": 9, "message": "streaming is not supported",
                       "reason": "UNSUPPORTED_OPERATION"}}
        ).encode()
        self.send_response(400)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *a):
        pass


srv = HTTPServer(("127.0.0.1", 0), Handler)
threading.Thread(target=srv.serve_forever, daemon=True).start()

print(f"httpx {httpx.__version__}")
print("ResponseNotRead MRO:", [c.__name__ for c in httpx.ResponseNotRead.__mro__])
print("_extract_error catches: (json.JSONDecodeError, ValueError)")
print("  issubclass(ResponseNotRead, ValueError)         =",
      issubclass(httpx.ResponseNotRead, ValueError))
print("  issubclass(ResponseNotRead, json.JSONDecodeError) =",
      issubclass(httpx.ResponseNotRead, json.JSONDecodeError))
print()

# Exactly what _request_streaming does on the error branch (lines 184-195).
client = httpx.Client(base_url=f"http://127.0.0.1:{srv.server_address[1]}")
request = client.build_request(
    "POST", "/v1/message:stream", json={"message": {}},
    headers={"Content-Type": "application/json", "Accept": "text/event-stream"},
)
response = client.send(request, stream=True)
print(f"status {response.status_code} >= {_HTTP_ERROR_MIN} ->",
      response.status_code >= _HTTP_ERROR_MIN)
response.close()  # closed WITHOUT read() -- http_json_client.py:187

# Exactly what the test does at test_transport_behavior.py:419
try:
    msg = _extract_error(response)
    print("RESULT: returned", repr(msg))
    print("VERDICT: not reproduced")
    sys.exit(1)
except Exception as e:
    print(f"RESULT: raised {type(e).__module__}.{type(e).__name__}: {e}")
    print("VERDICT: reproduced -- defect is in the harness, no SDK involved")
finally:
    srv.shutdown()
