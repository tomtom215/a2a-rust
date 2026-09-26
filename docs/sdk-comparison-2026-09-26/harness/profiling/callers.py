# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
import re,sys,collections
f=sys.argv[1]; target=sys.argv[2]
names={}; cur=None; cfn=None; agg=collections.Counter(); calls=collections.Counter(); expect=False
def nm(tok):
    m=re.match(r'\((\d+)\)(?: (.*))?',tok)
    if m.group(2): names[m.group(1)]=m.group(2)
    return names.get(m.group(1),m.group(1))
for line in open(f):
    line=line.rstrip('\n')
    if line.startswith('fn='): cur=nm(line[3:]); continue
    if line.startswith('cfn='): cfn=nm(line[4:]); continue
    if line.startswith('calls='):
        n=int(line.split('=')[1].split()[0]); expect=True; continue
    if expect:
        expect=False
        if cfn and target in cfn:
            parts=line.split()
            agg[cur]+=int(parts[-1]); calls[cur]+=n
tot=sum(agg.values())
print('total inclusive to',target,f'{tot:,}')
for k,v in agg.most_common(12): print(f'{v:>14,} {100*v/tot:5.1f}% calls={calls[k]:>8,}  {k[:150]}')
