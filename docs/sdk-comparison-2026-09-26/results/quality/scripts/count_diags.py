#!/usr/bin/env python3
"""Count unique compiler/clippy/rustdoc diagnostics in a cargo log: unique (level, message, file:line:col).
Excludes summary lines ('generated N warnings', 'build failed', ...). Breakdown by top path component and
separately flags diagnostics located in generated code (src/gen/ or target OUT_DIR)."""
import sys,re,collections
for f in sys.argv[1:]:
    lines=open(f,errors='replace').read().split('\n'); uniq=set(); nolc=0
    for i,l in enumerate(lines):
        m=re.match(r'^(warning|error)(\[\w+\])?: (.*)$',l)
        if not m: continue
        msg=m.group(3)
        if re.search(r'generated \d+ warnings?|warnings? emitted|build failed|could not compile|aborting due|^\d+ warnings? emitted|Compiling|could not document|failed to run custom build',msg): continue
        loc=None
        for j in range(i+1,min(i+6,len(lines))):
            mm=re.match(r'^\s*--> (\S+)',lines[j])
            if mm: loc=mm.group(1); break
        if loc is None: nolc+=1; loc='<noloc>'
        uniq.add((m.group(1),msg,loc))
    by=collections.Counter(); gen=0; lints=collections.Counter()
    for lv,msg,loc in uniq:
        by[loc.split('/')[0] if not loc.startswith('/') else ('OUT_DIR' if '/out/' in loc else loc.split('/')[3])]+=1
        if 'src/gen/' in loc or '/out/' in loc: gen+=1
    print(f"{f}: unique_diags={len(uniq)} in_generated_code={gen} no_location={nolc} by_path={dict(by.most_common())}")
