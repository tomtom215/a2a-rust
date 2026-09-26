# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
import json, sys
from http.server import BaseHTTPRequestHandler, HTTPServer
PORT = int(sys.argv[1]); LOG = sys.argv[2]
CARD = {"name":"capture","description":"d","version":"1.0.0",
 "supportedInterfaces":[{"url":f"http://127.0.0.1:{PORT}/","protocolBinding":"JSONRPC","protocolVersion":"1.0"}],
 "capabilities":{"extendedAgentCard":True},"defaultInputModes":["text/plain"],"defaultOutputModes":["text/plain"],
 "skills":[{"id":"s","name":"s","description":"s","tags":["t"]}]}
class H(BaseHTTPRequestHandler):
    def _log(self, body):
        with open(LOG, "a") as f:
            f.write(f"{self.command} {self.path}\n")
            for k, v in self.headers.items(): f.write(f"  {k}: {v}\n")
            f.write(f"BODY: {body!r}\n\n")
    def _send(self, obj):
        b = json.dumps(obj).encode()
        self.send_response(200); self.send_header("Content-Type","application/json")
        self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
    def do_GET(self):
        self._log(""); self._send(CARD)
    def do_POST(self):
        if self.headers.get("Transfer-Encoding","").lower() == "chunked":
            buf = b""
            while True:
                size = int(self.rfile.readline().strip(), 16)
                if size == 0:
                    self.rfile.readline(); break
                buf += self.rfile.read(size); self.rfile.readline()
            raw = buf.decode()
        else:
            n = int(self.headers.get("Content-Length") or 0)
            raw = self.rfile.read(n).decode()
        self._log(raw)
        try: rid = json.loads(raw).get("id")
        except Exception: rid = None
        self._send({"jsonrpc":"2.0","id":rid,"result":dict(CARD, name="capture-EXTENDED")})
    def log_message(self, *a): pass
HTTPServer(("127.0.0.1", PORT), H).serve_forever()
