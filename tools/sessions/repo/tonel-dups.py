#!/usr/bin/env python3
# tonel-dups.py ROOT... - the methods that more than one package defines, across
# the Tonel trees given (each the src/ of a checkout, or a checkout): an extension
# that one repository makes and another repeats or overrides, which Metacello
# loads in an order of its own.  Prints (Class, selector) and the
# <repository>:<package> of each definition.  Written for the
# Containers-Essentials review, against RoassalMN, RoassalLayouts and others:
#   tonel-dups.py ../Containers-Essentials ../Roassal/src ../RoassalLayouts/src
import sys,os,re,collections
roots=sys.argv[1:]
defs=collections.defaultdict(list)
pat=re.compile(r'^([A-Za-z_][\w]*(?: class)?) >> (\S+(?: \S+)*?) \[\s*$')
for root in roots:
    for dp,dn,fn in os.walk(root):
        if '/.git' in dp: continue
        for f in fn:
            if not f.endswith('.st') or f=='package.st': continue
            p=os.path.join(dp,f)
            pkg=os.path.basename(dp)
            try: txt=open(p,encoding='utf-8',errors='replace').read()
            except: continue
            if not re.search(r'^(Class|Extension|Trait) \{', txt, re.M): continue
            for line in txt.split('\n'):
                m=re.match(r'^([A-Za-z_]\w*(?: class)?) >> (.+?) \[\s*$', line)
                if m:
                    cls,sig=m.groups()
                    sel=''.join(re.findall(r'(\w+:)', sig)) or sig.split()[0]
                    defs[(cls,sel)].append((root.split('/')[-2] if root.endswith('src') else root.split('/')[-1])+':'+pkg)
for k,v in sorted(defs.items()):
    if len(set(v))>1: print(k, v)
