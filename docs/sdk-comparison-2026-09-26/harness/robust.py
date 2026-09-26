# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Robustness battery — identical against both neutral bench agents (echo mode).

Usage: robust.py <name> <pid> <jsonrpc_url>
Each probe records: HTTP status / outcome, whether the server still answers a
normal SendMessage afterwards (liveness), and the server's VmRSS in KiB.
"""
import asyncio, json, socket, sys, time, uuid, urllib.request, urllib.error
from urllib.parse import urlparse

NAME, PID, URL = sys.argv[1], int(sys.argv[2]), sys.argv[3]
U = urlparse(URL)
HOST, PORT, PATH = U.hostname, U.port, (U.path or '/')

def rss():
    for l in open(f'/proc/{PID}/status'):
        if l.startswith('VmRSS'):
            return int(l.split()[1])
    return None

def body_msg(text, **cfg):
    b = {'jsonrpc': '2.0', 'id': 1, 'method': 'SendMessage',
         'params': {'message': {'messageId': str(uuid.uuid4()), 'role': 'ROLE_USER', 'parts': [{'text': text}]}}}
    if cfg:
        b['params']['configuration'] = cfg
    return json.dumps(b).encode()

def post(data, timeout=30, extra=None):
    h = {'Content-Type': 'application/json', 'A2A-Version': '1.0'}
    h.update(extra or {})
    req = urllib.request.Request(URL, data=data, headers=h, method='POST')
    t = time.monotonic()
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, r.read()[:300].decode('utf-8', 'replace'), time.monotonic() - t
    except urllib.error.HTTPError as e:
        return e.code, e.read()[:300].decode('utf-8', 'replace'), time.monotonic() - t
    except Exception as e:
        return f'EXC {type(e).__name__}: {e}', '', time.monotonic() - t

def alive():
    s, b, dt = post(body_msg('ping'), timeout=5)
    return s == 200 and 'Echo: ping' in b, round(dt * 1000, 1)

results = []
def rec(probe, **kw):
    ok, ms = alive()
    kw.update(probe=probe, server=NAME, alive_after=ok, alive_ms=ms, rss_kib=rss())
    results.append(kw)
    print(json.dumps(kw), flush=True)

rec('R00_baseline')

# R01: accepted size — largest of 64 KiB..16 MiB text message that succeeds
limits = {}
for kib in (64, 256, 1024, 4096, 16384):
    s, b, dt = post(body_msg('x' * (kib * 1024)))
    limits[kib] = s
rec('R01_message_size_ladder_KiB', statuses=limits)

# R02: 256 MiB body announced and streamed — must be refused without buffering it
def big_upload(total=256 * 1024 * 1024):
    sk = socket.create_connection((HOST, PORT), timeout=30)
    hdr = (f'POST {PATH} HTTP/1.1\r\nHost: {HOST}\r\nContent-Type: application/json\r\n'
           f'A2A-Version: 1.0\r\nContent-Length: {total}\r\n\r\n').encode()
    sk.sendall(hdr)
    chunk = b'x' * (1024 * 1024)
    sent = 0
    t = time.monotonic()
    peak = 0
    try:
        while sent < total:
            sk.sendall(chunk)
            sent += len(chunk)
            if sent % (32 * 1024 * 1024) == 0:
                peak = max(peak, rss() or 0)
    except Exception as e:
        err = f'{type(e).__name__}'
    else:
        err = None
    sk.settimeout(10)
    try:
        resp = sk.recv(200).decode('utf-8', 'replace').split('\r\n')[0]
    except Exception as e:
        resp = f'recv {type(e).__name__}'
    sk.close()
    return {'sent_MiB': sent // (1024 * 1024), 'send_err': err, 'status_line': resp,
            'peak_rss_kib_during': peak, 'secs': round(time.monotonic() - t, 2)}
rec('R02_256MiB_body', **big_upload())

# R03: deeply nested JSON in metadata (depth 100k)
depth = 100000
nested = '[' * depth + ']' * depth
raw = ('{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{"messageId":"m","role":"ROLE_USER",'
       '"parts":[{"text":"n"}]},"metadata":{"x":' + nested + '}}}').encode()
s, b, dt = post(raw)
rec('R03_nested_depth_100k', status=s, body=b[:120])

# R04: invalid UTF-8 inside a JSON string
raw = b'{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{"messageId":"m","role":"ROLE_USER","parts":[{"text":"\xff\xfe"}]}}}'
s, b, dt = post(raw)
rec('R04_invalid_utf8', status=s, body=b[:120])

# R05: 200 slowloris connections (headers trickled), then a normal request must still be served
async def slowloris(n=200, hold=15):
    conns = []
    for _ in range(n):
        try:
            r, w = await asyncio.open_connection(HOST, PORT)
            w.write(f'POST {PATH} HTTP/1.1\r\nHost: {HOST}\r\n'.encode())
            await w.drain()
            conns.append((r, w))
        except Exception:
            break
    t0 = time.monotonic()
    mid = await asyncio.to_thread(alive)
    closed_by_server = 0
    end = time.monotonic() + hold
    while time.monotonic() < end:
        for r, w in conns:
            try:
                w.write(b'X-a: b\r\n')
                await w.drain()
            except Exception:
                pass
        await asyncio.sleep(1)
    for r, w in conns:
        if r.at_eof():
            closed_by_server += 1
        w.close()
    return {'opened': len(conns), 'normal_request_during': mid, 'closed_by_server_after_s': closed_by_server}
rec('R05_slowloris_200', **asyncio.run(slowloris()))

# R06: header-only idle connections — does the server time out a client that never sends a request?
async def idle(n=200, wait=40):
    conns = []
    for _ in range(n):
        r, w = await asyncio.open_connection(HOST, PORT)
        conns.append((r, w))
    await asyncio.sleep(wait)
    closed = 0
    for r, w in conns:
        try:
            d = await asyncio.wait_for(r.read(1), 0.05)
            if d == b'':
                closed += 1
        except asyncio.TimeoutError:
            pass
        w.close()
    return {'opened': n, 'closed_by_server_within_s': wait, 'closed': closed}
rec('R06_idle_connections_200', **asyncio.run(idle()))

# R07: 1000 concurrent long-running tasks held open via SSE streams, then abandoned
async def many_streams(n=1000):
    conns = []
    for i in range(n):
        try:
            r, w = await asyncio.open_connection(HOST, PORT)
        except Exception as e:
            return {'opened': i, 'err': type(e).__name__}
        b = json.dumps({'jsonrpc': '2.0', 'id': i, 'method': 'SendStreamingMessage',
                        'params': {'message': {'messageId': str(uuid.uuid4()), 'role': 'ROLE_USER',
                                               'parts': [{'text': 'wait:stream'}]}}}).encode()
        w.write(f'POST {PATH} HTTP/1.1\r\nHost: {HOST}\r\nContent-Type: application/json\r\nA2A-Version: 1.0\r\n'
                f'Accept: text/event-stream\r\nContent-Length: {len(b)}\r\n\r\n'.encode() + b)
        await w.drain()
        conns.append((r, w))
    await asyncio.sleep(2)
    got = 0
    for r, w in conns:
        try:
            d = await asyncio.wait_for(r.read(64), 0.2)
            if d:
                got += 1
        except asyncio.TimeoutError:
            pass
    held_rss = rss()
    mid = await asyncio.to_thread(alive)
    for r, w in conns:
        w.close()
    await asyncio.sleep(3)
    return {'opened': len(conns), 'streams_with_data': got, 'rss_while_held_kib': held_rss,
            'normal_request_while_held': mid}
rec('R07_1000_streams_then_disconnect', **asyncio.run(many_streams()))

# R08: task accumulation — 20k completed tasks via blocking SendMessage (8 in flight)
async def accumulate(n=20000, par=8):
    before = rss()
    sem = asyncio.Semaphore(par)
    t = time.monotonic()
    async def one():
        async with sem:
            await asyncio.to_thread(post, body_msg('acc'), 10)
    await asyncio.gather(*(one() for _ in range(n)))
    return {'tasks': n, 'rss_before_kib': before, 'rss_after_kib': rss(), 'secs': round(time.monotonic() - t, 1)}
rec('R08_20k_tasks_rss', **asyncio.run(accumulate()))

import os
out = os.environ.get('RESULTS', '/opt/bench/results') + '/robust'
os.makedirs(out, exist_ok=True)
json.dump(results, open(f'{out}/{NAME}.json', 'w'), indent=1)
