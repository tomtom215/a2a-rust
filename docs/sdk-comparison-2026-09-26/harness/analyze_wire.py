# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Spec audit of captured traffic (capture_proxy.py logs).

Usage: analyze_wire.py <log.jsonl>...   (file names: client-<c>--server-<s>.jsonl)

Client-side rules, per request:
  C-VER   data-plane requests carry `A2A-Version: 1.0` (spec 3.6.2)
  C-ENV   JSON-RPC envelope: jsonrpc "2.0", id is a string or number, method is
          one of the eleven v1.0 names (JSON-RPC 2.0 section 4; A2A 9.4)
  C-PAR   JSON-RPC params absent, an object, or an array (JSON-RPC 2.0 4.2)
  C-ROUTE REST (method, path) matches the section 11 route table; for
          SubscribeToTask both POST (11 prose table) and GET (a2a.proto http
          annotation) are accepted because the two normative sources disagree
  C-QUERY REST query parameter names are the camelCase proto field names
Server-side rules, per response:
  S-RPC   JSON-RPC responses: HTTP 200, jsonrpc "2.0", id echoes the request,
          exactly one of result / error, error.code integer + error.message string
  S-SSE   streaming responses are text/event-stream; every data line is JSON
          (JSON-RPC: a response object with the request's id)
  S-REST  REST errors (status >= 400) use the AIP-193 shape
          {"error":{"code":int,"status":str,"message":str}}; A2A-specific
          errors (details reason set) carry ErrorInfo with domain a2a-protocol.org
  S-CT    REST success Content-Type (SHOULD application/a2a+json)
"""
import collections, json, re, sys
from urllib.parse import urlsplit, parse_qs

METHODS = {'SendMessage', 'SendStreamingMessage', 'GetTask', 'ListTasks', 'CancelTask', 'SubscribeToTask',
           'CreateTaskPushNotificationConfig', 'GetTaskPushNotificationConfig', 'ListTaskPushNotificationConfigs',
           'DeleteTaskPushNotificationConfig', 'GetExtendedAgentCard'}
ROUTES = [('POST', r'/message:send'), ('POST', r'/message:stream'), ('GET', r'/tasks/[^/:]+'), ('GET', r'/tasks'),
          ('POST', r'/tasks/[^/:]+:cancel'), ('POST', r'/tasks/[^/:]+:subscribe'), ('GET', r'/tasks/[^/:]+:subscribe'),
          ('POST', r'/tasks/[^/:]+/pushNotificationConfigs'), ('GET', r'/tasks/[^/:]+/pushNotificationConfigs/[^/]+'),
          ('GET', r'/tasks/[^/:]+/pushNotificationConfigs'), ('DELETE', r'/tasks/[^/:]+/pushNotificationConfigs/[^/]+'),
          ('GET', r'/extendedAgentCard')]
QUERY = {'historyLength', 'contextId', 'status', 'pageSize', 'pageToken', 'statusTimestampAfter', 'includeArtifacts', 'tenant'}


def rest_path(p):
    p = urlsplit(p).path
    return re.sub(r'^/rest', '', p)  # a2a-rs mounts HTTP+JSON under /rest


def audit(f):
    viol = collections.defaultdict(list)
    counts = collections.Counter()
    for line in open(f):
        r = json.loads(line)
        path, meth, h = r['path'], r['method'], r['req_headers']
        if path.startswith('/.well-known/'):
            continue
        body = r['req_body']
        is_rpc = meth == 'POST' and body.lstrip().startswith('{') and '"jsonrpc"' in body
        kind = 'jsonrpc' if is_rpc else 'rest'
        counts[kind] += 1
        tag = f"{meth} {urlsplit(path).path}"
        if h.get('a2a-version') != '1.0':
            viol['C-VER'].append(f"{tag} a2a-version={h.get('a2a-version')!r}")
        if is_rpc:
            try:
                j = json.loads(body)
            except ValueError:
                viol['C-ENV'].append(f'{tag} unparseable body'); continue
            label = j.get('method')
            if j.get('jsonrpc') != '2.0' or not isinstance(j.get('id'), (str, int)) or j.get('method') not in METHODS:
                viol['C-ENV'].append(f"{label}: jsonrpc={j.get('jsonrpc')!r} id={j.get('id')!r}")
            if 'params' in j and not isinstance(j['params'], (dict, list)):
                viol['C-PAR'].append(f"{label}: params={json.dumps(j['params'])}")
            # server side
            if 'text/event-stream' in r['resp_headers'].get('content-type', ''):
                for d in (l[6:] for l in r['resp_body'].splitlines() if l.startswith('data: ')):
                    try:
                        e = json.loads(d)
                    except ValueError:
                        viol['S-SSE'].append(f'{label}: non-JSON data line'); continue
                    if e.get('jsonrpc') != '2.0' or e.get('id') != j.get('id'):
                        viol['S-SSE'].append(f"{label}: event id {e.get('id')!r} != request id {j.get('id')!r}")
            else:
                if r['status'] != 200:
                    viol['S-RPC'].append(f"{label}: HTTP {r['status']}")
                try:
                    e = json.loads(r['resp_body'])
                    ok = e.get('jsonrpc') == '2.0' and e.get('id') == j.get('id') and (('result' in e) != ('error' in e))
                    if 'error' in e:
                        ok = ok and isinstance(e['error'].get('code'), int) and isinstance(e['error'].get('message'), str)
                    if not ok:
                        viol['S-RPC'].append(f"{label}: {r['resp_body'][:120]}")
                except ValueError:
                    viol['S-RPC'].append(f"{label}: non-JSON body HTTP {r['status']}: {r['resp_body'][:80]}")
        else:
            p = rest_path(path)
            p = re.sub(r'^/[^/]+(?=/tasks|/message|/extendedAgentCard)', '', p) if not re.match(r'^/(tasks|message|extendedAgentCard)', p) else p
            if not any(meth == m and re.fullmatch(rx, p) for m, rx in ROUTES):
                viol['C-ROUTE'].append(tag)
            for k in parse_qs(urlsplit(path).query):
                if k not in QUERY:
                    viol['C-QUERY'].append(f'{tag} ?{k}')
            ct = r['resp_headers'].get('content-type', '')
            if r['status'] >= 400:
                try:
                    e = json.loads(r['resp_body'])['error']
                    if not (isinstance(e.get('code'), int) and isinstance(e.get('status'), str) and isinstance(e.get('message'), str)):
                        raise ValueError
                    reasons = [d for d in e.get('details', []) if 'ErrorInfo' in d.get('@type', '')]
                    if any(d.get('domain') != 'a2a-protocol.org' for d in reasons):
                        viol['S-REST'].append(f'{tag} ErrorInfo domain')
                except (ValueError, KeyError, TypeError, AttributeError):
                    viol['S-REST'].append(f"{tag} HTTP {r['status']} body={r['resp_body'][:100]!r}")
            elif r['status'] < 300 and 'event-stream' not in ct and r['resp_body'] and not ct.startswith('application/a2a+json'):
                viol['S-CT'].append(f'{tag} {ct}')
    return counts, viol


for f in sys.argv[1:]:
    counts, viol = audit(f)
    print(f"== {f.split('/')[-1]}  requests: {dict(counts)}")
    for rule in sorted(viol):
        uniq = collections.Counter(viol[rule])
        print(f"  {rule}: {len(viol[rule])} occurrence(s), {len(uniq)} distinct")
        for v, n in uniq.most_common(6):
            print(f"     x{n}  {v}")
