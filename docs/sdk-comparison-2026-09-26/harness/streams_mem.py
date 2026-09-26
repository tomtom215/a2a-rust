# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Isolated memory cost of held streams: fresh server, warm-up request, then N
open SendStreamingMessage streams on long-running ('wait:') tasks.
Usage: streams_mem.py <name> <pid> <jsonrpc_url> <N>"""
import asyncio, json, sys, uuid, urllib.request
from urllib.parse import urlparse
NAME, PID, URL, N = sys.argv[1], int(sys.argv[2]), sys.argv[3], int(sys.argv[4])
U = urlparse(URL); HOST, PORT, PATH = U.hostname, U.port, U.path or '/'
def rss():
    return next(int(l.split()[1]) for l in open(f'/proc/{PID}/status') if l.startswith('VmRSS'))
def warm():
    b = json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': 'SendMessage', 'params': {'message': {
        'messageId': str(uuid.uuid4()), 'role': 'ROLE_USER', 'parts': [{'text': 'warm'}]}}}).encode()
    urllib.request.urlopen(urllib.request.Request(URL, b, {'Content-Type': 'application/json', 'A2A-Version': '1.0'})).read()
async def main():
    warm(); await asyncio.sleep(0.5); base = rss(); conns = []
    for i in range(N):
        r, w = await asyncio.open_connection(HOST, PORT)
        b = json.dumps({'jsonrpc': '2.0', 'id': i, 'method': 'SendStreamingMessage', 'params': {'message': {
            'messageId': str(uuid.uuid4()), 'role': 'ROLE_USER', 'parts': [{'text': 'wait:m'}]}}}).encode()
        w.write(f'POST {PATH} HTTP/1.1\r\nHost: {HOST}\r\nContent-Type: application/json\r\nA2A-Version: 1.0\r\n'
                f'Accept: text/event-stream\r\nContent-Length: {len(b)}\r\n\r\n'.encode() + b)
        await w.drain(); conns.append((r, w))
    got = 0
    for r, w in conns:
        try:
            if await asyncio.wait_for(r.read(64), 2): got += 1
        except asyncio.TimeoutError: pass
    await asyncio.sleep(1); held = rss()
    print(json.dumps({'server': NAME, 'streams': N, 'streams_with_data': got, 'rss_base_kib': base,
                      'rss_held_kib': held, 'kib_per_stream': round((held - base) / N, 1)}))
    for r, w in conns: w.close()
asyncio.run(main())
