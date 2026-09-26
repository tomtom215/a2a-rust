#!/usr/bin/env python3
"""From a clippy log, count unique diagnostics whose primary location is in a library crate's src/ (given prefixes)."""
import sys,re,collections
log=sys.argv[1]; prefixes=sys.argv[2:]
lines=open(log,errors='replace').read().split('\n'); uniq=set()
for i,l in enumerate(lines):
    m=re.match(r'^warning(\[\w+\])?: (.*)$',l)
    if not m or re.search(r'generated \d+ warning',l): continue
    for j in range(i+1,min(i+6,len(lines))):
        mm=re.match(r'^\s*--> (\S+):\d+:\d+',lines[j])
        if mm: uniq.add((m.group(2),mm.group(0).strip())); break
c=collections.Counter()
for msg,loc in uniq:
    p=loc.split()[1]
    for pre in prefixes:
        if p.startswith(pre): c[pre]+=1; break
    else: c['<other: tests/examples/benches/tools/generated>']+=1
for k,v in c.most_common(): print(f"  {k}: {v}")
