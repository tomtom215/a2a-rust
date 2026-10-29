#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Connection round trip to a TCP port, from 20 SSLRequest exchanges: the
latency a proxy adds. Usage: rtt.py PORT"""
import socket, struct, sys, time
port = int(sys.argv[1]); best = []
for _ in range(20):
    s = socket.create_connection(("127.0.0.1", port)); t = time.perf_counter()
    s.sendall(struct.pack("!II", 8, 80877103)); s.recv(1); best.append((time.perf_counter() - t) * 1000); s.close()
best.sort(); print(f"port {port}: round trip min {best[0]:.2f} ms, median {best[10]:.2f} ms")
