# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""A tiny OTLP/HTTP-JSON receiver for the campaign's observability checks.

Needs: python3; an agent started with OTEL_EXPORTER_OTLP_PROTOCOL=http/json
(the a2a-campaign-agent is built with the SDK's `otlp-json` feature) and
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:<sink.port>.
Proves nothing by itself: it keeps every batch it receives, decoded, so a driver
can search them for canary strings and count metric label values.
"""

from __future__ import annotations

import gzip
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Sink:
    def __init__(self, port: int = 0):
        self.batches: list[dict] = []  # {signal, headers, raw (bytes), json}
        self.lock = threading.Lock()
        sink = self

        class H(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *a):
                pass

            def do_POST(self):
                n = int(self.headers.get("content-length") or 0)
                body = self.rfile.read(n) if n else b""
                if self.headers.get("transfer-encoding", "").lower() == "chunked":
                    body = b""
                    while True:
                        size = int(self.rfile.readline().strip() or b"0", 16)
                        if size == 0:
                            self.rfile.readline()
                            break
                        body += self.rfile.read(size)
                        self.rfile.readline()
                if self.headers.get("content-encoding") == "gzip":
                    try:
                        body = gzip.decompress(body)
                    except OSError:
                        pass
                signal = self.path.rsplit("/", 1)[-1]
                try:
                    parsed = json.loads(body)
                except ValueError:
                    parsed = None
                with sink.lock:
                    sink.batches.append({"signal": signal, "path": self.path, "headers": dict(self.headers),
                                         "raw": body, "json": parsed})
                out = b"{}"
                self.send_response(200)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(out)))
                self.end_headers()
                self.wfile.write(out)

        self.server = ThreadingHTTPServer(("127.0.0.1", port), H)
        self.server.daemon_threads = True
        self.port = self.server.server_address[1]
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def url(self) -> str:
        return f"http://127.0.0.1:{self.port}"

    def stop(self) -> None:
        self.server.shutdown()

    # --- queries -----------------------------------------------------------------

    def snapshot(self) -> list[dict]:
        with self.lock:
            return list(self.batches)

    def canary_hits(self, canaries: list[str]) -> dict[str, dict[str, int]]:
        """{canary: {signal: count of batches whose body contains it}}; the export's own
        request headers are searched too and reported under `<signal>:request-header`."""
        out: dict[str, dict[str, int]] = {c: {} for c in canaries}
        for b in self.snapshot():
            text = b["raw"].decode("utf-8", "replace")
            hdrs = json.dumps(b["headers"])
            for c in canaries:
                if c in text:
                    out[c][b["signal"]] = out[c].get(b["signal"], 0) + 1
                if c in hdrs:
                    k = b["signal"] + ":request-header"
                    out[c][k] = out[c].get(k, 0) + 1
        return out

    def spans(self) -> list[dict]:
        found = []
        for b in self.snapshot():
            if b["signal"] != "traces" or not b["json"]:
                continue
            for rs in b["json"].get("resourceSpans", []):
                for ss in rs.get("scopeSpans", []):
                    found.extend(ss.get("spans", []))
        return found

    def logs(self) -> list[dict]:
        found = []
        for b in self.snapshot():
            if b["signal"] != "logs" or not b["json"]:
                continue
            for rl in b["json"].get("resourceLogs", []):
                for sl in rl.get("scopeLogs", []):
                    found.extend(sl.get("logRecords", []))
        return found

    def points(self, metric: str) -> list[dict]:
        """Every data point of `metric` with its attributes flattened to {key: value}."""
        found = []
        for b in self.snapshot():
            if b["signal"] != "metrics" or not b["json"]:
                continue
            for rm in b["json"].get("resourceMetrics", []):
                for sm in rm.get("scopeMetrics", []):
                    for m in sm.get("metrics", []):
                        if m.get("name") != metric:
                            continue
                        for kind in ("sum", "gauge", "histogram", "exponentialHistogram"):
                            for dp in (m.get(kind) or {}).get("dataPoints", []):
                                found.append({"attributes": flatten(dp.get("attributes", [])), "dp": dp})
        return found

    def metric_names(self) -> set[str]:
        names = set()
        for b in self.snapshot():
            if b["signal"] == "metrics" and b["json"]:
                for rm in b["json"].get("resourceMetrics", []):
                    for sm in rm.get("scopeMetrics", []):
                        names.update(m.get("name", "") for m in sm.get("metrics", []))
        return names


def flatten(attrs: list[dict]) -> dict:
    out = {}
    for a in attrs:
        v = a.get("value", {})
        out[a.get("key")] = next(iter(v.values())) if v else None
    return out
