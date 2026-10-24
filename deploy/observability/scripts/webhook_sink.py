#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""A push-notification receiver that accepts every POST with 200 and counts
them, for exercising push delivery against a live stack. Usage: PORT."""
import http.server
import sys


class Sink(http.server.BaseHTTPRequestHandler):
    received = 0

    def do_POST(self):  # noqa: N802 (http.server's naming)
        self.rfile.read(int(self.headers.get("content-length") or 0))
        Sink.received += 1
        self.send_response(200)
        self.end_headers()

    def log_message(self, *args):
        pass


http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), Sink).serve_forever()
