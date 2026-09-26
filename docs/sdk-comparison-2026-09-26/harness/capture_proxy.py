# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Logging reverse proxy for HTTP/1.1 A2A traffic (JSON-RPC, HTTP+JSON, SSE).

Usage: capture_proxy.py <listen_port> <target_host:port> <log.jsonl> [<from_port>=<to_port> ...]

Every request/response pair is appended to the log as one JSON line. Agent-card
responses are rewritten so interface URLs on <from_port> point at <to_port>
(i.e. at the proxy), which is how a client that discovers its endpoints from
the card is steered through the proxy without changing the client. Streaming
(SSE) responses are relayed as they arrive and closed when the upstream ends.
"""
import http.client, json, sys, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LISTEN, TARGET, LOG = int(sys.argv[1]), sys.argv[2], sys.argv[3]
REWRITES = [tuple(a.split('=')) for a in sys.argv[4:]]
HOST, PORT = TARGET.split(':')
lock = threading.Lock()


def log(rec):
    with lock, open(LOG, 'a') as f:
        f.write(json.dumps(rec) + '\n')


class H(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *a):
        pass

    def _any(self):
        n = int(self.headers.get('Content-Length') or 0)
        body = self.rfile.read(n) if n else b''
        rec = {'t': time.time(), 'listen': LISTEN, 'method': self.command, 'path': self.path,
               'req_headers': {k.lower(): v for k, v in self.headers.items()},
               'req_body': body.decode('utf-8', 'replace')[:2000000]}
        up = http.client.HTTPConnection(HOST, int(PORT), timeout=60)
        hdrs = {k: v for k, v in self.headers.items() if k.lower() not in ('host', 'connection')}
        up.request(self.command, self.path, body=body or None, headers=hdrs)
        r = up.getresponse()
        rec.update(status=r.status, resp_headers={k.lower(): v for k, v in r.getheaders()})
        ctype = r.getheader('Content-Type', '')
        if 'text/event-stream' in ctype:
            self.send_response(r.status)
            for k, v in r.getheaders():
                if k.lower() not in ('transfer-encoding', 'content-length', 'connection'):
                    self.send_header(k, v)
            self.send_header('Connection', 'close')
            self.end_headers()
            got = b''
            while True:
                chunk = r.read1(65536) if hasattr(r, 'read1') else r.read(1)
                if not chunk:
                    break
                got += chunk
                try:
                    self.wfile.write(chunk); self.wfile.flush()
                except Exception:
                    break
            rec['resp_body'] = got.decode('utf-8', 'replace')[:2000000]
            self.close_connection = True
        else:
            data = r.read()
            if self.path.startswith('/.well-known/') and REWRITES:
                s = data.decode()
                for a, b in REWRITES:
                    s = s.replace(f'127.0.0.1:{a}', f'127.0.0.1:{b}')
                data = s.encode()
            rec['resp_body'] = data.decode('utf-8', 'replace')[:2000000]
            self.send_response(r.status)
            for k, v in r.getheaders():
                if k.lower() not in ('transfer-encoding', 'content-length', 'connection', 'etag', 'last-modified'):
                    self.send_header(k, v)
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        up.close()
        log(rec)

    do_GET = do_POST = do_PUT = do_DELETE = do_PATCH = _any


ThreadingHTTPServer(('127.0.0.1', LISTEN), H).serve_forever()
