#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""A TCP proxy for the PostgreSQL wire protocol that records each round trip
a client makes: every Sync ('S') or simple Query ('Q') it sends waits for the
server. Logs one line per round trip: time, connection, and the statements in
it (Parse text, or the prepared statement a Bind names). SIGUSR1 marks a line."""
import json, os, socket, struct, sys, threading, time, signal
LAT = float(os.environ.get("PG_LAT_MS", "0")) / 1000  # added to each server->client chunk
LISTEN, UP, OUT = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3]
log = open(OUT, "w", buffering=1); lock = threading.Lock(); nconn = [0]
def emit(rec):
    with lock: log.write(json.dumps(rec) + "\n")
signal.signal(signal.SIGUSR1, lambda *_: emit({"mark": time.time()}))
def pump(src, dst, lat=0.0):
    try:
        while (b := src.recv(65536)):
            if lat: time.sleep(lat)
            dst.sendall(b)
    except OSError: pass
    finally:
        for s in (src, dst):
            try: s.shutdown(socket.SHUT_RDWR)
            except OSError: pass
def client(c):
    nconn[0] += 1; cid = nconn[0]
    u = socket.create_connection(("127.0.0.1", UP))
    threading.Thread(target=pump, args=(u, c, LAT), daemon=True).start()
    buf = b""; started = False; names = {}; pending = []; types = []
    def recv():
        nonlocal buf
        b = c.recv(65536)
        if not b: raise EOFError
        u.sendall(b); buf += b
    try:
        while True:
            if not started:
                while len(buf) < 4: recv()
                n = struct.unpack("!I", buf[:4])[0]
                while len(buf) < n: recv()
                code = struct.unpack("!I", buf[4:8])[0]; buf = buf[n:]
                if code != 80877103: started = True
                continue
            while len(buf) < 5: recv()
            t = buf[:1]; n = struct.unpack("!I", buf[1:5])[0]
            while len(buf) < 1 + n: recv()
            body = buf[5:1 + n]; buf = buf[1 + n:]
            types.append(t.decode())
            if t == b"P":
                name, rest = body.split(b"\0", 1); q = rest.split(b"\0", 1)[0].decode(errors="replace")
                names[name] = q; pending.append(q)
            elif t == b"B":
                portal, rest = body.split(b"\0", 1); stmt = rest.split(b"\0", 1)[0]
                if stmt in names and (not pending or pending[-1] != names[stmt]): pending.append(names[stmt])
            elif t in (b"S", b"Q"):
                if t == b"Q": pending.append(body.rstrip(b"\0").decode(errors="replace"))
                emit({"t": time.time(), "conn": cid, "kind": t.decode(), "msgs": "".join(types), "stmts": [" ".join(p.split())[:160] for p in pending]})
                pending = []; types = []
    except (EOFError, OSError): pass
srv = socket.socket(); srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("127.0.0.1", LISTEN)); srv.listen(64)
while True:
    c, _ = srv.accept(); threading.Thread(target=client, args=(c,), daemon=True).start()
