#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Check docs/program mechanically, and keep status.json in step with plan/*.md.

    python3 docs/program/tools/plan_check.py .           # check; exit 1 on an error
    python3 docs/program/tools/plan_check.py . --next    # what is ready to start now
    python3 docs/program/tools/plan_check.py . --write   # rewrite ids, titles, deps and gates in
                                                         # status.json; status, landed and decisions
                                                         # are kept as they are

Errors: a package with no Gates, Depends or Accept field; a dependency, gate or id that is not
defined; a dependency cycle; README section 4's gate column disagreeing with the packages;
status.json disagreeing with plan/*.md; a cited line beyond the end of its file.
Reported, not an error: path-like tokens that do not resolve (a package may name a file it creates).
"""
import json, re, sys, pathlib, collections
root = pathlib.Path(sys.argv[1]); prog = root/'docs/program'
write = '--write' in sys.argv
WS = {'FND':'foundation','COR':'correctness','SWM':'swarm','SPC':'spec','PRF':'performance','OBS':'observability','SCL':'scale','DVX':'devex','ECO':'ecosystem'}
pk = {}; errs = []
for f in sorted((prog/'plan').glob('*.md')):
    text = f.read_text()
    parts = re.split(r'^## ([A-Z]{3}-\d\d): (.+)$', text, flags=re.M)
    for i in range(1, len(parts), 3):
        pid, title, body = parts[i], parts[i+1].strip(), parts[i+2]
        flat = ' '.join(body.split())
        def field(name):
            m = re.search(r'\*\*%s\.\*\*(.*?)(?=\*\*[A-Z][A-Za-z ]+\.\*\*|$)' % name, flat)
            return m.group(1) if m else None
        g, d, r = field('Gates'), field('Depends'), field('Release')
        if g is None or d is None: errs.append(f'{pid}: no Gates/Depends field')
        for need in ('Accept',):
            if field(need) is None: errs.append(f'{pid}: no {need}')
        rel = 'unstated'
        if r:
            m = re.search(r'\[0\.16\.x\]|\[0\.17\]', r); rel = m.group(0).strip('[]') if m else 'none'
        pk[pid] = dict(id=pid, workstream=WS[pid[:3]], title=title, file=f'plan/{f.name}',
            deps=sorted(set(re.findall(r'[A-Z]{3}-\d\d', d or '')) - {pid}),
            gates=sorted(set(re.findall(r'DEC-\d\d', g or ''))),
            release=rel, breaking=bool(r and re.search(r'\*\*breaking\*\*|breaking\*\*:', r)),
            status='todo', landed=None)
readme = (prog/'README.md').read_text()
decs = set(re.findall(r'^\| (DEC-\d\d) \|', readme, flags=re.M))
for p in pk.values():
    for d in p['deps']:
        if d not in pk: errs.append(f"{p['id']}: dep {d} undefined")
    for g in p['gates']:
        if g not in decs: errs.append(f"{p['id']}: gate {g} undefined")
# cycles
state = {}
def visit(n, path):
    if state.get(n) == 2: return
    if state.get(n) == 1: errs.append('cycle: ' + ' -> '.join(path+[n])); return
    state[n] = 1
    for d in pk[n]['deps']:
        if d in pk: visit(d, path+[n])
    state[n] = 2
for n in pk: visit(n, [])
# every id mentioned anywhere in program docs exists
for f in [prog/'README.md', prog/'target-sheet.md', *sorted((prog/'plan').glob('*.md'))]:
    for m in set(re.findall(r'\b((?:FND|COR|SWM|SPC|PRF|OBS|SCL|DVX|ECO)-\d\d)\b', f.read_text())):
        if m not in pk: errs.append(f'{f.name}: mentions undefined {m}')
    for m in set(re.findall(r'\bDEC-\d\d\b', f.read_text())):
        if m not in decs: errs.append(f'{f.name}: mentions undefined {m}')
# README gate column agrees with packages
for row in re.findall(r'^\| (DEC-\d\d) \|.*\| ([^|]*) \|$', readme, flags=re.M):
    dec, col = row
    claimed = set()
    for m in re.finditer(r'([A-Z]{3})-(\d\d)(?: to [A-Z]{3}-(\d\d))?', col):
        lo = int(m.group(2)); hi = int(m.group(3) or lo)
        claimed |= {f'{m.group(1)}-{n:02d}' for n in range(lo, hi+1)}
    actual = {p['id'] for p in pk.values() if dec in p['gates']}
    if 'every SPC' in col: continue
    if claimed != actual: errs.append(f'README {dec}: table says {sorted(claimed)}, packages say {sorted(actual)}')
# paths
ABBR = {'S/':'crates/a2a-protocol-server/src/','C/':'crates/a2a-protocol-client/src/','T/':'crates/a2a-protocol-types/src/'}
checked = 0; missing = []
files = [prog/'README.md', *sorted((prog/'plan').glob('*.md'))]
for f in files:
    for tok in set(re.findall(r'`([^`\s]+)`', f.read_text())):
        t = tok
        for a, full in ABBR.items():
            if t.startswith(a): t = full + t[len(a):]
        m = re.match(r'^([A-Za-z0-9_.\-/]+?)(?::(\d+))?/?$', t)
        if not m or '/' not in t and not re.search(r'\.(md|toml|json|yml|sh|py|rs|cff)$', t): continue
        p, line = m.group(1), m.group(2)
        if p.startswith(('a2a_protocol', 'http', 'target/', 'origin/', 'tests/', 'a2a-rust.com', 'UPSTREAM')) or '::' in tok: continue
        if p.startswith(('plan/','evidence/')) or p in ('status.json','target-sheet.md','README.md') and f.parent==prog: cand = prog/p
        elif p.startswith('../'): cand = (f.parent/p)
        else: cand = root/p
        checked += 1
        if not cand.exists():
            missing.append((f.name, tok))
        elif line and cand.is_file() and int(line) > sum(1 for _ in cand.open(errors='replace')):
            errs.append(f'{f.name}: {tok} line beyond file')
out = dict(schema=1, written='2026-11-25', base_commit='2058058d',
    statuses=['todo','in-progress','partial','done','dropped'],
    note='Status lives here only. Edit status, landed and decisions by hand; ids, titles, deps and gates mirror plan/*.md and are rewritten by docs/program/tools/plan_check.py --write.',
    decisions={d: 'undecided' for d in sorted(decs)}, packages=[pk[k] for k in sorted(pk)])
if write:
    sj = prog/'status.json'
    if sj.exists():
        old = json.loads(sj.read_text()); keep = {p['id']: p for p in old.get('packages', [])}
        for p in out['packages']:
            if p['id'] in keep: p['status'], p['landed'] = keep[p['id']]['status'], keep[p['id']].get('landed')
        out['decisions'].update({k: v for k, v in old.get('decisions', {}).items() if k in out['decisions']})
    sj.write_text(json.dumps(out, indent=2) + '\n')
else:
    cur = json.loads((prog/'status.json').read_text())
    a = {p['id']:(p['deps'],p['gates'],p['title']) for p in cur['packages']}; b = {k:(v['deps'],v['gates'],v['title']) for k,v in pk.items()}
    if a != b: errs.append('status.json differs from plan/*.md')
if '--next' in sys.argv:
    cur = json.loads((prog/'status.json').read_text())
    st = {p['id']: p for p in cur['packages']}; dec = cur.get('decisions', {})
    WAIT = {'SPC-10': 'an upstream A2A 1.1 tag that is not a release candidate', 'ECO-07': 'Goose serving A2A'}
    done = lambda i: st[i]['status'] in ('done', 'dropped')
    ready, waiting, held = [], [], []
    for i, p in sorted(st.items()):
        if done(i): continue
        undecided = [g for g in p['gates'] if not str(dec.get(g, '')).startswith('decided')]
        open_deps = [d for d in p['deps'] if not done(d)]
        if undecided: held.append(f"  {i}  undecided {', '.join(undecided)}")
        elif open_deps: continue
        elif i in WAIT: waiting.append(f"  {i}  waits on {WAIT[i]}")
        else: ready.append(f"  {i}  [{p['status']}]  {p['title']}  ({p['file']})")
    print(f"ready to start ({len(ready)}):"); print('\n'.join(ready))
    print(f"ready but for an upstream event ({len(waiting)}):"); print('\n'.join(waiting))
    if held: print(f"held by an undecided gate ({len(held)}):"); print('\n'.join(held))
    print(f"done: {sum(1 for i in st if done(i))} of {len(st)}")
    sys.exit(0)
print(f'packages {len(pk)}; by workstream {dict(collections.Counter(p["workstream"] for p in pk.values()))}')
print(f'decisions {len(decs)}; gated packages {sum(1 for p in pk.values() if p["gates"])}; ungated with no deps {sum(1 for p in pk.values() if not p["gates"] and not p["deps"])}')
print(f'paths checked {checked}; missing {len(missing)}')
for m in sorted(missing): print('  MISSING', m)
for e in errs: print('  ERR', e)
sys.exit(1 if errs else 0)
