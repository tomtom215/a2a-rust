#!/usr/bin/env python3
"""Sum all 'test result:' lines in a cargo test log; list failures."""
import sys,re
for f in sys.argv[1:]:
    t={'passed':0,'failed':0,'ignored':0,'measured':0,'filtered out':0}; n=0
    txt=open(f,errors='replace').read()
    for m in re.finditer(r'test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out',txt):
        n+=1
        for k,v in zip(['passed','failed','ignored','measured','filtered out'],m.groups()[1:]): t[k]+=int(v)
    fails=sorted(set(re.findall(r'^test (\S+) \.\.\. FAILED',txt,re.M)))
    ex=re.findall(r'^EXIT: (\d+)',txt,re.M)
    print(f"{f}: binaries={n} {t} exit={ex} failures={fails}")
