# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Time-to-first-token and total time: direct llama-server vs through each SDK's
JSON-RPC streaming path (server agent in llm mode, raw SSE client — no SDK client,
so only the server SDK is on the path). Order rotated each iteration; prompt
cache disabled. Usage: ttft.py <iterations>"""
import json, sys, time, http.client, uuid, statistics
N = int(sys.argv[1])
PROMPTS = [f"In one sentence, what is {x}?" for x in
           ["the capital of France", "photosynthesis", "a prime number", "TCP", "gravity",
            "a compiler", "the Moon", "DNA", "an atom", "the internet", "a volcano", "Rust"]]

def direct(prompt):
    c = http.client.HTTPConnection('127.0.0.1', 8080, timeout=300)
    body = json.dumps({"model": "qwen3-0.6b", "messages": [
        {"role": "system", "content": "You are a concise assistant. Answer in one or two sentences."},
        {"role": "user", "content": prompt}], "stream": True, "max_tokens": 48, "temperature": 0.0,
        "seed": 42, "cache_prompt": False, "chat_template_kwargs": {"enable_thinking": False}})
    t = time.monotonic(); c.request('POST', '/v1/chat/completions', body, {'Content-Type': 'application/json'})
    r = c.getresponse(); first = None; text = ''
    for line in r:
        line = line.decode().strip()
        if not line.startswith('data: ') or line == 'data: [DONE]': continue
        d = json.loads(line[6:])['choices'][0]['delta'].get('content')
        if d:
            first = first or time.monotonic() - t; text += d
    return first, time.monotonic() - t, text

def via(port, path, prompt):
    c = http.client.HTTPConnection('127.0.0.1', port, timeout=300)
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "SendStreamingMessage", "params": {"message": {
        "messageId": str(uuid.uuid4()), "role": "ROLE_USER", "parts": [{"text": prompt}]}}})
    t = time.monotonic()
    c.request('POST', path, body, {'Content-Type': 'application/json', 'A2A-Version': '1.0', 'Accept': 'text/event-stream'})
    r = c.getresponse(); first = None; text = ''; state = None
    for line in r:
        line = line.decode().strip()
        if not line.startswith('data: '): continue
        res = json.loads(line[6:]).get('result', {})
        if 'artifactUpdate' in res:
            a = res['artifactUpdate']
            chunk = ''.join(p.get('text', '') for p in a['artifact']['parts'])
            if chunk: first = first or time.monotonic() - t
            text = text + chunk if a.get('append') else chunk
        for k in ('statusUpdate', 'task'):
            if k in res: state = res[k]['status']['state']
        if state in ('TASK_STATE_COMPLETED', 'TASK_STATE_FAILED'): break
    return first, time.monotonic() - t, text

paths = {'direct': direct, 'a2a-rust': lambda p: via(7101, '/', p), 'a2a-rs': lambda p: via(7201, '/jsonrpc', p)}
names = list(paths); res = {k: [] for k in names}; texts = {k: [] for k in names}
for i in range(N):
    p = PROMPTS[i % len(PROMPTS)]
    order = names[i % 3:] + names[:i % 3]
    for k in order:
        f, tot, txt = paths[k](p); res[k].append((f, tot)); texts[k].append(txt)
for k in names:
    fs = [x[0] * 1000 for x in res[k]]; ts = [x[1] * 1000 for x in res[k]]
    print(json.dumps({'path': k, 'n': N, 'ttft_ms_median': statistics.median(fs), 'ttft_ms_min': min(fs), 'ttft_ms_max': max(fs),
                      'total_ms_median': statistics.median(ts), 'total_ms_min': min(ts), 'total_ms_max': max(ts)}))
same = sum(texts['direct'][i] == texts['a2a-rust'][i] == texts['a2a-rs'][i] for i in range(N))
print(json.dumps({'identical_text_all_three_paths': f'{same}/{N}', 'sample': texts['a2a-rs'][0]}))
