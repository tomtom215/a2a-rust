#!/usr/bin/env python3
"""Approximate lines of inline unit-test code inside published src/: lines within `#[cfg(test)] mod x { ... }`
blocks, plus whole files pulled in by `#[cfg(test)] mod x;` (resolved as x.rs or x/mod.rs next to the parent),
plus files named tests.rs / *_tests.rs under src/. Comments/strings are stripped before brace matching."""
import sys,os,re
sys.path.insert(0,os.path.dirname(__file__))
from unsafe_scan import strip
def run(root):
    src=os.path.join(root,'src'); total=0; test=0; testfiles=set()
    files=[os.path.join(b,f) for b,_,fs in os.walk(src) for f in fs if f.endswith('.rs')]
    for f in files: total+=sum(1 for _ in open(f,errors='replace'))
    for f in files:
        s=strip(open(f,errors='replace').read())
        for m in re.finditer(r'#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*([;{])',s):
            if m.group(2)=='{':
                i=m.end(); depth=1
                while i<len(s) and depth:
                    depth+= {'{':1,'}':-1}.get(s[i],0); i+=1
                test+=s[m.start():i].count('\n')+1
            else:
                d=os.path.dirname(f); stem=os.path.splitext(os.path.basename(f))[0]
                base=d if stem in('mod','lib','main') else os.path.join(d,stem)
                for cand in (os.path.join(base,m.group(1)+'.rs'),os.path.join(base,m.group(1),'mod.rs')):
                    if os.path.exists(cand): testfiles.add(cand)
    for f in testfiles: test+=sum(1 for _ in open(f,errors='replace'))
    return total,test,len(testfiles)
for r in sys.argv[1:]:
    t,x,n=run(r); print(f"{os.path.basename(r):30} src_lines={t:6} cfg_test_lines~={x:6} ({100*x/max(t,1):.0f}%) cfg_test_files={n}")
