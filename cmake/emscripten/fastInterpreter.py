#!/usr/bin/env python3
"""fastInterpreter.py INTERP.c OUT.c DIR - the interpreter of WASM_FAST_INTERP=ON
Rewrites the generated interp.c: the memory accesses of interpret() get constant offsets and typed addresses (iaddr,
32-bit in the web VM), the method cache has 4096 entries of 64 bytes that hold what an activation needs of the header,
the at cache 64 entries, and interpret() becomes interpretGeneric(), behind DIR/interpret.inc."""
import re,sys
src=open(sys.argv[1]).read()
s=src.index('\nsqInt\ninterpret(void)\n{')+1
e=src.index('\n}\n',s)+3
head,body,tail=src[:s],src[s:e],src[e:]
def sub1(old,new,n=1,where='body'):
    global body,head,tail
    t=globals()[where]; c=t.count(old)
    if n is not None and c!=n: raise SystemExit(f'expected {n} found {c}: {old[:120]}')
    globals()[where]=t.replace(old,new)
CONST=dict(re.findall(r'#define (\w+) (-?(?:0x[0-9A-Fa-f]+|\d+))\n',src))
CONST={k:int(v,0) for k,v in CONST.items()}
CONST.update(BytesPerWord=8,BytesPerOop=8,BaseHeaderSize=8)
BIAS=128
# ---- linear forms ----
TOK=re.compile(r'\s*(0x[0-9A-Fa-f]+U?L*|\d+U?L*|\w+|->|<<|>>|==|!=|<=|>=|\+\+|--|&&|\|\||.)',re.S)
def toks(x):
    r=[];i=0
    x=x.strip()
    while i<len(x):
        m=TOK.match(x,i); r.append((m.group(1),m.start(1),m.end(1))); i=m.end()
        while i<len(x) and x[i].isspace(): i+=1
    return r
def strip_outer(x):
    x=x.strip()
    while x.startswith('('):
        d=0
        for i,c in enumerate(x):
            if c=='(': d+=1
            elif c==')':
                d-=1
                if d==0: break
        if i==len(x)-1: x=x[1:-1].strip()
        else: break
    return x
def split_top(x,ops):
    "split at top-level binary operators in ops (one precedence level), left to right: list of (op,text)"
    d=0; out=[]; last=0; op='+'; T=toks(x); prev=None
    for t,a,b in T:
        if t in '([': d+=1
        elif t in ')]': d-=1
        elif d==0 and t in ops and prev not in (None,'(') and not re.fullmatch(r'[-+*/<>&|^=!~?:,]|<<|>>',prev or ''):
            out.append((op,x[last:a])); op=t; last=b
        prev=t
    out.append((op,x[last:])); return out
CAST=re.compile(r'^\((?:sqInt|usqInt|usqLong|sqLong)\)\s*')
def lin(x):
    "linear form: (const, [(coef,text)])"
    x=strip_outer(x)
    m=CAST.match(x)
    while m: x=strip_outer(x[m.end():]); m=CAST.match(x)
    x=re.sub(r'\bshiftForWord\(\)','3',x)
    if re.fullmatch(r'(0x[0-9A-Fa-f]+|\d+)U?L*',x): return int(x.rstrip('UL'),0),[]
    if x in CONST: return CONST[x],[]
    # reject anything with low-precedence operators at top level
    for ops in (['?',':',','],['||'],['&&'],['|'],['^'],['&'],['==','!='],['<','>','<=','>='],['=']):
        if len(split_top(x,ops))>1: return 0,[(1,x)]
    p=split_top(x,['<<'])
    if len(p)==2 and len(split_top(x,['>>']))==1:
        c,t=lin(p[1][1])
        if not t:
            c0,t0=lin(p[0][1]); return c0<<c,[(k<<c,y) for k,y in t0]
        return 0,[(1,x)]
    if len(p)>1 or len(split_top(x,['>>']))>1: return 0,[(1,x)]
    p=split_top(x,['+','-'])
    if len(p)>1:
        c=0; ts=[]
        for op,y in p:
            k,t=lin(y); sg=1 if op=='+' else -1
            c+=sg*k; ts+=[(sg*a,z) for a,z in t]
        return c,ts
    p=split_top(x,['*'])
    if len(p)==2:
        a=lin(p[0][1]); b=lin(p[1][1])
        if not a[1]: a,b=b,a
        if not b[1]: return a[0]*b[0],[(k*b[0],t) for k,t in a[1]]
    return 0,[(1,x)]
def emit(c,ts):
    r=[]
    out=[]
    for k,t in ts:
        if t=='local_framePointer' and k==1: c+=BIAS; t='lfpb'
        out.append((k,t))
    out.sort(key=lambda kt:0 if kt[1] in('lfpb','local_stackPointer') else 1)
    st=''
    for k,t in out:
        if re.search(r'[^\w>().\[\]-]',t) : t='('+t+')'
        term=t if abs(k)==1 else f'({t} * {abs(k)})'
        st+=(' + ' if st else '')+term if k>0 else (' - ' if st else '-')+term
    if c<0: st=f'{st} - {-c}'; c=0
    return st or '0',c
def call_args(t,i):
    "t[i] is '(' ; returns list of argument strings and end index after ')'"
    d=0; args=[]; a=i+1
    for j in range(i,len(t)):
        ch=t[j]
        if ch=='(': d+=1
        elif ch==')':
            d-=1
            if d==0: args.append(t[a:j]); return args,j+1
        elif ch==',' and d==1: args.append(t[a:j]); a=j+1
FN={'unsignedLongAt':'LDQ','longAt':'LDS','long64At':'LDS','byteAt':'LDB','byteAtPointer':'LDB','unsignedLongAtput':'STQ','longAtput':'STQ','uint32AtPointer':'LDW'}
FNRE=re.compile(r'\b('+'|'.join(sorted(FN,key=len,reverse=True))+r')\(')
def addrpass(t):
    out='';i=0
    while True:
        m=FNRE.search(t,i)
        if not m: return out+t[i:]
        args,j=call_args(t,m.end()-1)
        out+=t[i:m.start()]
        if m.start()>0 and t[:m.start()].rstrip().endswith('sizeof') : pass
        a=addrpass(args[0]); rest=[addrpass(x) for x in args[1:]]
        ma=re.fullmatch(r'\s*\((_sp\w*) = (.*)\)\s*',a,re.S)
        f=FN[m.group(1)]
        if ma or '++' in a or '/*' in a or re.search(r'[^=!<>]=[^=]',a):
            p,c=a,0
        else:
            k,ts=lin(a); p,c=emit(k,ts)
        out+=f'{f}({p}, {c}'+''.join(', '+r.strip() for r in rest)+')'
        i=j
MACROS='''
#ifdef IA32	/* the web VM: every address is below 4 GB, so addresses are 32-bit locals */
typedef uint32_t iaddr;
#else
typedef uint64_t iaddr;
#endif
#define IPTR(p,c) (((char *)(uintptr_t)(iaddr)(p)) + (c))
typedef usqInt __attribute__((may_alias)) usqInt_a;
typedef sqInt __attribute__((may_alias)) sqInt_a;
typedef uint32_t __attribute__((may_alias)) uint32_a;
typedef iaddr __attribute__((may_alias)) iaddr_a;
#define LDQ(p,c) (*(usqInt_a *)IPTR(p,c))
#define LDS(p,c) (*(sqInt_a *)IPTR(p,c))
#define LDA(p,c) (*(iaddr_a *)IPTR(p,c))	/* an oop, or a frame address, to be used as an address */
#define LDW(p,c) ((sqInt)*(uint32_a *)IPTR(p,c))
#define LDB(p,c) ((sqInt)*(unsigned char *)IPTR(p,c))
#define STQ(p,c,v) (*(usqInt_a *)IPTR(p,c) = (usqInt)(v))
#define LIKELY(x) __builtin_expect(!!(x),1)
#define UNLIKELY(x) __builtin_expect(!!(x),0)
'''
def feat_addr():
    global body
    # signed comparisons of longAt results: keep the types (longAt signed -> used only with masks)
    body=addrpass(body)
    sub1('\tsqInt local_framePointer;\n\tsqInt local_instructionPointer;\n\tsqInt local_stackPointer;\n','\tiaddr lfpb;\n\tiaddr local_instructionPointer;\n\tiaddr local_stackPointer;\n')
    body=re.sub(r'\blocal_framePointer = ([^;]*);',r'lfpb = (iaddr)(\1) - %d;'%BIAS,body)
    body=re.sub(r'\blocal_framePointer\b','(lfpb + %d)'%BIAS,body)
    body=MACROS+body


def feat_split():
    global body,tail,head
    sub1('\tsqInt currentBytecode;\n','\tuint32_t currentBytecode;\n')
    sub1('sqInt\ninterpret(void)\n{','static GENATTR sqInt\ninterpretGeneric(sqInt hotEntry, sqInt hotArg)\n{')
    sub1('\tcurrentBytecode = LDB(++local_instructionPointer, 0);\n\t/* end fetchByte */\n\t/* end fetchNextBytecode */\n\twhile (1) {','''	currentBytecode = LDB(local_instructionPointer, 0);
	if (hotEntry == HE_SEND) { lkupClassTag = hotArg; goto commonSendOrdinary; }
	if (hotEntry == HE_EXECUTE) goto hotExecuteNewMethod;
	if (hotEntry == HE_ACTIVATE) goto hotActivateNewMethod;
	if (hotEntry == HE_RETURN) { localReturnValue = hotArg; goto commonReturn; }
	if (hotEntry == HE_CALLERRETURN) { localReturnValue = hotArg; goto commonCallerReturn; }
	while (1) {
		hotGenericExt:''')
    for lab,nxt in (('extABytecode','case 225:'),('extBBytecode','case 226:')):
        i=body.index(f'VM_LABEL({lab})'); j=body.index('\t\t\tbreak;\n\t\t\t'+nxt,i)
        body=body[:j]+'\t\t\tgoto hotGenericExt;\n\t\t\t'+nxt+body[j+len('\t\t\tbreak;\n\t\t\t'+nxt):]
    sub1('\t\t\t\t\t\t\t\tl591:\n\t\t\t\t\t\t\t\t;\n','\t\t\t\t\t\t\t\tl591:\n\t\t\t\t\t\t\t\t;\n\t\t\t\t\t\t\t\thotExecuteNewMethod:\n\t\t\t\t\t\t\t\t;\n')
    k=body.index('hotExecuteNewMethod:'); a=body.index('/* begin activateNewMethod */',k)
    body=body[:a]+'hotActivateNewMethod:\n'+body[a:]
    i=body.rindex('\t\t\tbreak;\n\t\t}\n\t}\n\tlocal_instructionPointer -= 1;')
    body=body[:i]+'\t\t\tbreak;\n\t\t}\n\t\tGIV(framePointer) = (char *)(uintptr_t)(lfpb + 128); GIV(instructionPointer) = local_instructionPointer; GIV(stackPointer) = (char *)(uintptr_t)local_stackPointer;\n\t\treturn 0;\n\t}\n\tlocal_instructionPointer -= 1;'+body[i+len('\t\t\tbreak;\n\t\t}\n\t}\n\tlocal_instructionPointer -= 1;'):]
    # the fast interpreter precedes; MACROS first
    i=body.index('static GENATTR sqInt'); body=body[:i]+'#include "%s/interpret.inc"\n'%inc+body[i:]
    # the method cache
    n=4096
    for k,v in dict(MethodCacheEntries=hex(n),MethodCacheEntrySize='8',MethodCacheMask=hex((n-1)*8),MethodCacheSize=str(n*8)).items():
        head=re.sub(r'#define %s .*'%k,f'#define {k} {v}',head)
    head=head.replace('_iss sqIntptr_t methodCache[','__attribute__((aligned(64))) _iss sqIntptr_t methodCache[')
    na=64
    head=re.sub(r'#define AtCacheMask .*','#define AtCacheMask '+hex((na-1)*4),head)
    head=re.sub(r'#define AtPutBase .*','#define AtPutBase %d'%(na*4),head)
    head=re.sub(r'#define AtCacheTotalSize .*','#define AtCacheTotalSize %d'%(na*8),head)
    head=head.replace('#define AtCacheFmt 3','#define ATIX(r) (((r) >> 1) & AtCacheMask)\n#define AtCacheFmt 3')
    sub1('_rcvr & AtCacheMask','ATIX(_rcvr)',2)
    H=HASH
    head=head.replace('#define MethodCacheSelector 1\n','#define MethodCacheSelector 1\n'+H)
    old='_selector ^ ((sqInt) (((usqInt) _classTag ) << 2) )'
    old2='GIV(messageSelector) ^ ((sqInt) (((usqInt) _classTag ) << 2) )'
    assert tail.count(old)==2 and tail.count(old2)==1,(tail.count(old),tail.count(old2))
    tail=tail.replace(old2,'MCHASH((uint32_t)GIV(messageSelector), (uint32_t)_classTag)').replace(old,'MCHASH((uint32_t)_selector, (uint32_t)_classTag)')
    # fill the activation words
    i=tail.index('\naddNewMethodToCache(sqInt _classObj)\n{'); j=tail.index('\n}\n',i)
    f=tail[i:j]
    for ind in ('\t\t\t','\t'):
        f=f.replace('\n'+ind+'GIV(methodCache)[_probe + MethodCachePrimFunction] = ((sqIntptr_t) primitiveFunctionPointer );','\n'+ind+'GIV(methodCache)[_probe + MethodCachePrimFunction] = ((sqIntptr_t) primitiveFunctionPointer );\n'+ind+'hotFillEntry(&GIV(methodCache)[_probe], GIV(newMethod));')
    assert f.count('hotFillEntry')==2
    tail=tail[:i]+f+tail[j:]
    # what caches what a header says: a method replaced by become: (two-way become: swaps contents in place), or its header stored
    assert tail.count('/* end followForwardedMethodsInMethodCache */')==1
    tail=tail.replace('/* end followForwardedMethodsInMethodCache */','hotRefillMethodCache();\n\t\t\t/* end followForwardedMethodsInMethodCache */')
    i=tail.index('\nprimitiveObjectAtPut(void)\n{') if '\nprimitiveObjectAtPut(void)\n{' in tail else -1
    if i>=0:
        j=tail.index('\n}\n',i); tail=tail[:j]+'\n\tif (_index == 1) hotRefillMethodCache();'+tail[j:]
    else:
        i=head.index('\nprimitiveObjectAtPut(void)\n{'); j=head.index('\n}\n',i); head=head[:j]+'\n\tif (_index == 1) hotRefillMethodCache();'+head[j:]
    fill='''
static inline __attribute__((always_inline)) double hotSmallFloatValue(sqInt oop) {
	usqLong rot = (usqLong)oop >> 3; double d;
	if (rot > 1) rot += (usqLong)896 << 53;
	rot = (rot << 63) | (rot >> 1);
	memcpy(&d, &rot, 8); return d; }
static void hotFillEntry(sqIntptr_t *e, sqInt m)
{
	uint32_t *w = (uint32_t *)e, nn = 255, na = 0, nt; usqInt hd = 0;
	if (((m & 7) == 0) && ((((usqInt)longAt(m)) >> 24) & 31) >= 24) {
		hd = longAt(m + 8); na = (hd >> 27) & 15; nt = (hd >> 21) & 63;
		if (!(hd & AlternateHeaderHasPrimFlag)) nn = nt > na ? nt - na : 0;
	}
	w[10] = (uint32_t)(hd & 0x3FFF8) + 15; w[11] = nn; w[12] = 1 + (na << 8);
}
static __attribute__((noinline)) void hotRefillMethodCache(void)
{
	sqInt i;
	for (i = 0; i < MethodCacheSize; i += MethodCacheEntrySize)
		if (GIV(methodCache)[i + MethodCacheSelector] && GIV(methodCache)[i + MethodCacheMethod])
			hotFillEntry(&GIV(methodCache)[i], GIV(methodCache)[i + MethodCacheMethod]);
}
'''
    pass
    i=body.index("#include"); body=body[:i]+fill+body[i:]
HASH='#define MCHASH(s,t) ((s) ^ ((t) << 3))\n#define MCPROBE0(h) (((h) & MethodCacheMask) << 3)\n#define MCPROBE1(h) ((((h) >> 1) & MethodCacheMask) << 3)\n#define MCPROBE2(h) ((((h) >> 2) & MethodCacheMask) << 3)\n'


out=sys.argv[2]; inc=sys.argv[3]
feat_addr(); feat_split()
text=head+body+tail
try: old=open(out).read()
except OSError: old=None
if old!=text: open(out,'w').write(text)
