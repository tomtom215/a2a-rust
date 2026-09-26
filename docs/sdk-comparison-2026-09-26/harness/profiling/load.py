# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
import http.client, json, sys
host, port, path, n = sys.argv[1], int(sys.argv[2]), sys.argv[3], int(sys.argv[4])
c = http.client.HTTPConnection(host, port, timeout=120)
body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "SendMessage", "params": {"message": {"messageId": "m", "role": "ROLE_USER", "parts": [{"text": "hello"}]}}})
for _ in range(n):
    c.request('POST', path, body, {'Content-Type': 'application/json', 'A2A-Version': '1.0'})
    r = c.getresponse(); d = r.read()
    assert r.status == 200 and b'"result"' in d, d[:200]
print('ok', n)
