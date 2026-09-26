#!/usr/bin/env python3
"""Count `unsafe` keyword tokens in Rust sources, excluding // and /* */ comments
(incl. doc comments), string/char literals and raw strings. Usage: unsafe_scan.py <dir>...
Scans src/**/*.rs and build.rs under each dir. Prints each match with classification."""
import sys, os, re

def strip(src):
    out=[]; i=0; n=len(src)
    while i<n:
        c=src[i]
        if src.startswith('//',i):
            j=src.find('\n',i); j=n if j<0 else j
            out.append(' '*(j-i)); i=j; continue
        if src.startswith('/*',i):
            depth=1; j=i+2
            while j<n and depth:
                if src.startswith('/*',j): depth+=1; j+=2
                elif src.startswith('*/',j): depth-=1; j+=2
                else: j+=1
            out.append(re.sub(r'[^\n]',' ',src[i:j])); i=j; continue
        m=re.match(r'b?r(#*)"',src[i:i+20])
        if m and (i==0 or not (src[i-1].isalnum() or src[i-1]=='_')):
            end='"'+m.group(1); j=src.find(end,i+len(m.group(0))); j=n if j<0 else j+len(end)
            out.append(re.sub(r'[^\n]',' ',src[i:j])); i=j; continue
        if c=='"':
            j=i+1
            while j<n and src[j]!='"':
                j+= 2 if src[j]=='\\' else 1
            j+=1; out.append(re.sub(r'[^\n]',' ',src[i:j])); i=j; continue
        if c=="'":
            m=re.match(r"'(\\.[^']*|[^\\'])'",src[i:i+12])
            if m: out.append(' '*len(m.group(0))); i+=len(m.group(0)); continue
        out.append(c); i+=1
    return ''.join(out)

def main():
    total=0
    for root in sys.argv[1:]:
        files=[]
        for base,_,fs in os.walk(os.path.join(root,'src')):
            files+= [os.path.join(base,f) for f in fs if f.endswith('.rs')]
        if os.path.exists(os.path.join(root,'build.rs')): files.append(os.path.join(root,'build.rs'))
        cnt={}
        for f in sorted(files):
            raw=open(f,encoding='utf-8',errors='replace').read(); s=strip(raw)
            rl=raw.split('\n')
            for ln,line in enumerate(s.split('\n'),1):
                for m in re.finditer(r'\bunsafe\b',line):
                    rest=line[m.end():].lstrip()
                    kind='block' if rest.startswith('{') else 'fn' if rest.startswith(('fn','extern')) else 'impl' if rest.startswith('impl') else 'trait' if rest.startswith('trait') else 'attr/other'
                    cnt[kind]=cnt.get(kind,0)+1
                    print(f"MATCH {os.path.relpath(f,root)}:{ln}: [{kind}] {rl[ln-1].strip()}")
        t=sum(cnt.values()); total+=t
        print(f"CRATE {os.path.basename(root)} files={len(files)} unsafe_total={t} {cnt}")
        # also forbid/deny attrs in raw
        for f in sorted(files):
            for ln,l in enumerate(open(f,encoding='utf-8',errors='replace'),1):
                if re.search(r'#!\[(forbid|deny)\([^)]*unsafe_code',l): print(f"  ATTR {os.path.relpath(f,root)}:{ln}: {l.strip()}")
    print("TOTAL",total)

if __name__=='__main__':
    main()
