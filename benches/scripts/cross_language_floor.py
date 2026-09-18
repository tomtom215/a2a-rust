#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F.
"""The floor target for `cross_language_bench.py`.

Split out of the harness because it is a test double rather than part of the
measurement: it answers with a constant so the report can separate the cost of
the client, the loopback stack and the kernel from the cost of an A2A server.
Keeping it here also keeps its deliberate crudeness from reading as a defect in
the harness proper.
"""

from __future__ import annotations

import socket
import threading


class FloorServer(threading.Thread):
    """A server that returns one pre-baked response and does no A2A work.

    Its purpose is to put a number on everything that is *not* the SDK: the
    client loop, the loopback stack, and the kernel. No real server can beat
    it, so it is a floor, not a competitor. It is intentionally the crudest
    possible implementation — one thread per connection, a constant reply —
    because anything smarter would start measuring the floor server instead.
    """

    daemon = True

    def __init__(self, response_len: int) -> None:
        super().__init__()
        body = b'{"jsonrpc":"2.0","id":1,"result":{}}'
        body = body + b" " * max(0, response_len - len(body))
        self.response = (
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n"
            b"Content-Length: " + str(len(body)).encode() + b"\r\n\r\n" + body
        )
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.sock.bind(("127.0.0.1", 0))
        self.sock.listen(128)
        self.port = self.sock.getsockname()[1]

    def run(self) -> None:
        while True:
            try:
                client, _ = self.sock.accept()
            except OSError:
                return
            threading.Thread(target=self._serve, args=(client,), daemon=True).start()

    def _serve(self, client: socket.socket) -> None:
        client.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        buf = b""
        try:
            while True:
                chunk = client.recv(65536)
                if not chunk:
                    return
                buf += chunk
                # Count complete requests by their terminator; the body is a
                # fixed size so this is exact for our single fixed payload.
                while b"\r\n\r\n" in buf:
                    head, rest = buf.split(b"\r\n\r\n", 1)
                    want = 0
                    for line in head.split(b"\r\n"):
                        if line.lower().startswith(b"content-length:"):
                            want = int(line.split(b":", 1)[1])
                    if len(rest) < want:
                        break
                    buf = rest[want:]
                    client.sendall(self.response)
        except OSError:
            return
        finally:
            client.close()
