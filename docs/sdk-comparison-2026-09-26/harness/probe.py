# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Raw-wire spec probes, identical for both servers. Mirrors specific ACTS checks
but runs against the neutral bench agents (same config on both SDKs), so a
failure here is attributable to the SDK, not to an ITK agent's choices."""
import json, sys, urllib.request, uuid

def call(url, body=None, headers=None, raw=None, method='POST'):
    h = {'Content-Type': 'application/json', 'A2A-Version': '1.0'}
    h.update(headers or {})
    h = {k: v for k, v in h.items() if v is not None}
    data = raw.encode() if raw is not None else (json.dumps(body).encode() if body is not None else None)
    req = urllib.request.Request(url, data=data, headers=h, method=method)
    try:
        with urllib.request.urlopen(req, timeout=10) as r:
            return r.status, dict(r.headers), r.read().decode()
    except urllib.error.HTTPError as e:
        return e.code, dict(e.headers), e.read().decode()

def rpc(url, method, params, rid=1, headers=None):
    return call(url, {'jsonrpc': '2.0', 'id': rid, 'method': method, 'params': params}, headers)

def msg(text, **kw):
    m = {'messageId': str(uuid.uuid4()), 'role': 'ROLE_USER', 'parts': [{'text': text}]}
    m.update(kw); return m

def main(name, card_base, jsonrpc, rest):
    out = []
    def rec(pid, ok, detail): out.append({'server': name, 'probe': pid, 'ok': ok, 'detail': detail[:200]})
    s, h, b = call(card_base + '/.well-known/agent-card.json', method='GET')
    rec('P01_card_cache_headers(SHOULD)', any(k.lower() in ('cache-control', 'etag') for k in h), f"status={s} headers={sorted(k for k in h)}")
    s, h, b = call(jsonrpc, raw='{this is not valid json')
    j = json.loads(b) if b.startswith('{') else {}
    rec('P02_parse_error_-32700_http200(MUST)', s == 200 and j.get('error', {}).get('code') == -32700, f"status={s} body={b[:120]}")
    s, h, b = call(jsonrpc, {'jsonrpc': '2.0', 'id': 1, 'params': {}})
    j = json.loads(b) if b.startswith('{') else {}
    rec('P03_missing_method_-32600_http200(MUST)', s == 200 and j.get('error', {}).get('code') in (-32600, -32602), f"status={s} body={b[:120]}")
    s, h, b = rpc(jsonrpc, 'SendMessage', {'message': msg('first')})
    t1 = json.loads(b)['result']['task']
    s, h, b = rpc(jsonrpc, 'SendMessage', {'message': msg('again', taskId=t1['id'])})
    rec('P04_send_to_terminal_task_rejected(MUST)', 'error' in json.loads(b), b[:160])
    s, h, b = rpc(jsonrpc, 'SendMessage', {'message': msg('second')})
    t2 = json.loads(b)['result']['task']
    s, h, b = rpc(jsonrpc, 'SendMessage', {'message': msg('x', taskId=t1['id'], contextId=t2['contextId'])})
    rec('P05_task_context_mismatch_rejected(MUST)', 'error' in json.loads(b), b[:160])
    s, h, b = rpc(jsonrpc, 'ListTasks', {'contextId': t1['contextId']})
    r = json.loads(b).get('result', {})
    rec('P06_list_nextPageToken_present(MUST)', isinstance(r.get('nextPageToken'), str), b[:160])
    s, h, b = rpc(jsonrpc, 'SendMessage', {'message': msg('v')}, headers={'A2A-Version': '99.0'})
    j = json.loads(b)
    rec('P07_unsupported_version_-32009(MUST)', j.get('error', {}).get('code') == -32009, b[:160])
    s, h, b = rpc(jsonrpc, 'GetTask', {'id': '00000000-0000-0000-0000-000000000000'})
    e = json.loads(b).get('error', {})
    rec('P08_error_has_message_and_ErrorInfo(MUST)', isinstance(e.get('message'), str) and any('ErrorInfo' in str(d.get('@type', '')) for d in (e.get('data') or []) if isinstance(d, dict)), b[:200])
    s, h, b = rpc(jsonrpc, 'SendMessage', {'message': msg('ts')})
    ts = json.loads(b)['result']['task']['status'].get('timestamp')
    rec('P09_status_timestamp_present(MUST)', bool(ts), f"timestamp={ts}")
    s, h, b = call(rest + '/message:send', {'message': msg('ct')})
    rec('P10_rest_content_type_a2a+json(SHOULD)', h.get('Content-Type', h.get('content-type', '')).startswith('application/a2a+json'), f"status={s} ct={h.get('Content-Type')}")
    s, h, b = rpc(jsonrpc, 'SendMessage', {'message': msg('hist')}, )
    t3 = json.loads(b)['result']['task']
    s, h, b = rpc(jsonrpc, 'SendMessage', {'message': msg('h0'), 'configuration': {'historyLength': 0}})
    hist = json.loads(b)['result']['task'].get('history')
    rec('P11_historyLength0_no_history(SHOULD)', hist in (None, []), f"history_len={None if hist is None else len(hist)}")
    s, h, b = rpc(jsonrpc, 'SubscribeToTask', {'id': t3['id']})
    e = (json.loads(b) if b.startswith('{') else {}).get('error', {})
    rec('P12_subscribe_terminal_UnsupportedOperation(MUST)', e.get('code') == -32004, f"status={s} {b[:160]}")
    s, h, b = call(jsonrpc, {'jsonrpc': '2.0', 'id': 9, 'method': 'GetExtendedAgentCard'})
    rec('P13_extcard_params_omitted_accepted(spec §9.4 example)', 'result' in (json.loads(b) if b.startswith('{') else {}), b[:120])
    s, h, b = rpc(jsonrpc, 'SendMessage', {'message': {'messageId': str(uuid.uuid4()), 'role': 'ROLE_USER', 'parts': [{'data': {'v': 1}, 'mediaType': 'application/x-unsupported-type-12345'}]}})
    e = json.loads(b).get('error', {})
    rec('P14_unsupported_content_type_-32005(MUST)', e.get('code') == -32005, b[:160])
    req = urllib.request.Request(jsonrpc, data=json.dumps({'jsonrpc': '2.0', 'id': 15, 'method': 'SendStreamingMessage',
        'params': {'message': msg('stream-first')}}).encode(), headers={'Content-Type': 'application/json',
        'A2A-Version': '1.0', 'Accept': 'text/event-stream'}, method='POST')
    first = None
    with urllib.request.urlopen(req, timeout=10) as r:
        for line in r:
            line = line.decode().strip()
            if line.startswith('data: '):
                first = list(json.loads(line[6:]).get('result', {}).keys()); break
    rec('P15_stream_begins_with_Task(MUST §3.1.2)', first == ['task'], f"first_event_keys={first}")
    for o in out: print(json.dumps(o))

main(*sys.argv[1:5])
