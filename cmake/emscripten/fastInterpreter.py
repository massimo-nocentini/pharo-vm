#!/usr/bin/env python3
"""fastInterpreter.py - the interpreter of WASM_FAST_INTERP=ON

    fastInterpreter.py INTERP.c OUT.c DIR      writes OUT.c
    fastInterpreter.py --accept INTERP.c DIR   records the generated code that DIR repeats
    fastInterpreter.py --pieces INTERP.c DIR   prints that code, normalised

OUT.c is the generated INTERP.c, rewritten: the memory accesses of interpret() get constant offsets and typed addresses
(iaddr, 32-bit in a VM with a 32-bit memory), the method cache has 4096 entries of 64 bytes that hold what an activation
needs of the header of a method, the at cache 64 entries, and interpret() becomes interpretGeneric(), behind
DIR/interpret.inc, which is written by hand (with DIR/handlers.inc) and repeats the generated code of the commonest
bytecodes, of the sends and returns and of some primitives.

What DIR repeats must not change unseen, so the script stops, with status 1 and a message, when
  - the generated code has not the text that it rewrites (or has it another number of times);
  - the method cache or the at cache have another shape, or other functions use them;
  - the generated code of something that DIR repeats is not the one of DIR/generated.txt, the hashes that --accept
    records: the bytecodes that handlers.inc handles (as it says itself), the functions of FUNCTIONS, and the defines
    and the special cases below.
DIR/checks.inc, included by interpret.inc, checks the constants (the layout of frames, headers, methods, caches and
classes) when the interpreter is compiled."""
import hashlib, re, sys

def fail(message):
    raise SystemExit('fastInterpreter.py: ' + message + '\n'
        '  The fast interpreter of the WebAssembly VMs (src/emscripten/fastinterp) does not fit this generated interpreter:\n'
        '  change it (docs/WebAssembly.md, Faster interpreters), or build with WASM_FAST_INTERP=OFF.')

args = sys.argv[1:]
mode = args.pop(0) if args[:1] in (['--accept'], ['--pieces']) else None
if len(args) != (2 if mode else 3):
    raise SystemExit(__doc__)
interp, inc = args[0], args[-1]
src = open(interp).read()
START = '\nsqInt\ninterpret(void)\n{'
if src.count(START) != 1:
    fail('%s has %d interpret() functions' % (interp, src.count(START)))
s = src.index(START) + 1
e = src.index('\n}\n', s) + 3
head, body, tail = src[:s], src[s:e], src[e:]

# ---- what DIR repeats

# the generated functions that interpret.inc repeats (inlines), those that they and the bytecodes only call but that it
# repeats too, and those whose effects on the caches it relies on
FUNCTIONS = '''
addNewMethodToCache findNewMethodInClassTag inlineLookupInMethodCacheSelclassTag lookupInMethodCacheSelclassTag
rewriteMethodCacheEntryForExternalPrimitiveToFunction primitiveFlushCache primitiveFlushCacheByMethod
primitiveFlushCacheBySelector primitiveAdoptInstance primitiveChangeClass primitiveObjectAtPut
executeQuickPrimitive methodHeaderHasPrimitive
primitiveAt primitiveAtPut primitiveStringAt primitiveStringAtPut primitiveNew instantiateClassisPinnedisOldSpace
primitiveClass primitiveIdentical primitiveFullClosureValue floatObjectOf floatValueOf
isSmallFloatValue initializeHeaderOfStartAddressnumSlotsformatclassIndexpinned fetchClassOfNonImm
numSlotsOf fetchPointerofObject splObj rawHeaderOf literalCountOfMethodHeader
'''.split()
# the functions that may use the caches: (method cache, at cache)
CACHE_USERS = {
    'methodCache': 'primitiveFlushCache primitiveFlushExternalPrimitives primitiveUnloadModule fullGC scavengingGCTenuringIf '
        'addNewMethodToCache inlineLookupInMethodCacheSelclassTag lookupInMethodCacheSelclassTag postBecomeAction '
        'printMethodCacheFor rewriteMethodCacheEntryForExternalPrimitiveToFunction primitiveFlushCacheByMethod '
        'primitiveFlushCacheBySelector readImageNamed',
    'atCache': 'interpret primitiveAdoptInstance primitiveChangeClass primitiveFlushCache primitiveFlushExternalPrimitives '
        'primitiveUnloadModule fullGC scavengingGCTenuringIf postBecomeAction primitiveFlushCacheByMethod '
        'primitiveFlushCacheBySelector readImageNamed'}

functions = {}
for m in re.finditer(r'^(\w+)\(([^;{}]*)\)\n\{', src, re.M):
    functions.setdefault(m.group(1), src[m.start():src.index('\n}\n', m.end()) + 3])

def normal(text):
    "the text of a piece without its comments, white space, and the numbers that depend on the rest of the interpreter"
    text = re.sub(r'/\*.*?\*/', ' ', text, flags=re.S)
    text = re.sub(r'\bl\d+\b', 'l', text)
    text = re.sub(r'VM_LABEL\((\w*?)\d*\)', r'VM_LABEL(\1)', text)
    return re.sub(r'\s+', ' ', text).strip()

def handledBytecodes():
    "the bytecodes that handlers.inc handles"
    h = re.sub(r'/\*.*?\*/', '', open(inc + '/handlers.inc').read(), flags=re.S)
    r = {int(n) for n in re.findall(r'\bHC\((\d+)\)', h)} | {int(n) for n in re.findall(r'\bCCMP\((\d+),', h)}
    for macro, base in re.findall(r'#define (\w+)\(n\) case HC\((?:(\d+) \+ )?n\)', h):
        r |= {int(base or 0) + int(n) for n in re.findall(r'\b%s\((\d+)\)' % macro, h)}
    return sorted(r)

def bytecodeCases():
    "bytecode -> the text of its case in interpret(), and of the shared code that is in it"
    r = {}
    i = body.index('\t\tswitch (currentBytecode) {\n')
    parts = re.split(r'\n((?:\t\t\tcase \d+:\n)+)', body[i:])
    for labels, text in zip(parts[1::2], parts[2::2]):
        for n in re.findall(r'\d+', labels):
            r[int(n)] = text
    if sorted(r) != list(range(256)):
        fail('interpret() has not a case for each of the 256 bytecodes')
    return r

def pieces():
    r = {}
    cases = bytecodeCases()
    for n in handledBytecodes():
        r['bytecode %d' % n] = normal(cases[n])
    for label in ('commonReturn', 'commonCallerReturn', 'normalSend', 'commonSendOrdinary'):
        # what the generic entries of interpret.inc jump to, in the cases that hold them
        holders = [n for n in cases if 'VM_LABEL(%s)' % label in cases[n]]
        if not holders:
            fail('interpret() has no %s' % label)
        r['shared ' + label] = normal(cases[min(holders)])
    for f in FUNCTIONS:
        if f not in functions:
            fail('the generated code has no function ' + f)
        r['function ' + f] = normal(functions[f])
    # where permanent space begins, for the write barrier
    r['the start of perm space'] = ' '.join(sorted(set(re.findall(r'>= (0x[0-9A-Fa-f]+)LL\b', src))))
    return r

def digest(text):
    return hashlib.sha256(text.encode()).hexdigest()[:16]

def ranges(numbers):
    out, l = [], sorted(numbers)
    while l:
        n = 1
        while n < len(l) and l[n] == l[0] + n:
            n += 1
        out.append(str(l[0]) if n == 1 else '%d-%d' % (l[0], l[n - 1]))
        l = l[n:]
    return ', '.join(out)

def describe(names):
    codes = [int(x.split()[1]) for x in names if x.startswith('bytecode ')]
    r = ['bytecodes ' + ranges(codes)] if codes else []
    return ', '.join(r + [x for x in names if not x.startswith('bytecode ')])

found = pieces()
for name, users in CACHE_USERS.items():
    now = sorted(f for f in functions if re.search(r'GIV\(%s\)' % name, functions[f]))
    if now != sorted(users.split()):
        fail('other functions use the %s than it knows: %s' % (name, ', '.join(sorted(set(now) ^ set(users.split())))))

recorded_file = inc + '/generated.txt'
if mode == '--pieces':
    for k, v in found.items():
        print('%s\n%s\n' % (k, v))
    raise SystemExit(0)
if mode == '--accept':
    with open(recorded_file, 'w') as f:
        f.write('# The generated code that the fast interpreter repeats: what it is, and a hash of its text.\n'
                '# GENERATED BY cmake/emscripten/fastInterpreter.py --accept: only after the changes of that code were made here too.\n')
        for k, v in found.items():
            f.write('%s  %s\n' % (digest(v), k))
    raise SystemExit(0)
recorded = {}
for line in open(recorded_file):
    if not line.startswith('#'):
        h, k = line.rstrip('\n').split('  ', 1)
        recorded[k] = h
changed = [k for k in found if recorded.get(k) != digest(found[k])] + [k for k in recorded if k not in found]
if changed:
    fail('the generated code of what the fast interpreter repeats has changed: %s.\n'
         '  See what changed (fastInterpreter.py --pieces, of this interp.c and of the one before), make the same change in\n'
         '  interpret.inc and handlers.inc, and record the new code: python3 cmake/emscripten/fastInterpreter.py --accept\n'
         '  %s %s' % (describe(changed), interp, inc))

# ---- rewriting

def rewrite(old, new, count=1, where='body'):
    "old, which must be there count times, becomes new"
    t = globals()[where]
    if t.count(old) != count:
        fail('the generated code has %d times, not %d, the text that it rewrites:\n%s' % (t.count(old), count, old.strip('\n')[:300]))
    globals()[where] = t.replace(old, new)

def redefine(name, old, new):
    "the define of name, whose value must be old, becomes new"
    global head
    m = re.findall(r'^#define %s (.*)$' % name, head, re.M)
    if m != [old]:
        fail('the generated code has not %s defined as %s, once, but as %s' % (name, old, ' and '.join(m) or 'nothing'))
    head = re.sub(r'^(#define %s ).*$' % name, lambda m: m.group(1) + new, head, flags=re.M)

CONST = dict(re.findall(r'#define (\w+) (-?(?:0x[0-9A-Fa-f]+|\d+))\n', src))
CONST = {k: int(v, 0) for k, v in CONST.items()}
CONST.update(BytesPerWord=8, BytesPerOop=8, BaseHeaderSize=8)	# (checks.inc checks these)
BIAS = 128

# -- linear forms: an address as a constant and a sum of terms
TOK = re.compile(r'\s*(0x[0-9A-Fa-f]+U?L*|\d+U?L*|\w+|->|<<|>>|==|!=|<=|>=|\+\+|--|&&|\|\||.)', re.S)
def toks(x):
    r = []; i = 0
    x = x.strip()
    while i < len(x):
        m = TOK.match(x, i); r.append((m.group(1), m.start(1), m.end(1))); i = m.end()
        while i < len(x) and x[i].isspace(): i += 1
    return r
def strip_outer(x):
    x = x.strip()
    while x.startswith('('):
        d = 0
        for i, c in enumerate(x):
            if c == '(': d += 1
            elif c == ')':
                d -= 1
                if d == 0: break
        if i == len(x) - 1: x = x[1:-1].strip()
        else: break
    return x
def split_top(x, ops):
    "split at top-level binary operators in ops (one precedence level), left to right: list of (op,text)"
    d = 0; out = []; last = 0; op = '+'; T = toks(x); prev = None
    for t, a, b in T:
        if t in '([': d += 1
        elif t in ')]': d -= 1
        elif d == 0 and t in ops and prev not in (None, '(') and not re.fullmatch(r'[-+*/<>&|^=!~?:,]|<<|>>', prev or ''):
            out.append((op, x[last:a])); op = t; last = b
        prev = t
    out.append((op, x[last:])); return out
CAST = re.compile(r'^\((?:sqInt|usqInt|usqLong|sqLong)\)\s*')
def lin(x):
    "linear form: (const, [(coef,text)])"
    x = strip_outer(x)
    m = CAST.match(x)
    while m: x = strip_outer(x[m.end():]); m = CAST.match(x)
    x = re.sub(r'\bshiftForWord\(\)', '3', x)
    if re.fullmatch(r'(0x[0-9A-Fa-f]+|\d+)U?L*', x): return int(x.rstrip('UL'), 0), []
    if x in CONST: return CONST[x], []
    # reject anything with low-precedence operators at top level
    for ops in (['?', ':', ','], ['||'], ['&&'], ['|'], ['^'], ['&'], ['==', '!='], ['<', '>', '<=', '>='], ['=']):
        if len(split_top(x, ops)) > 1: return 0, [(1, x)]
    p = split_top(x, ['<<'])
    if len(p) == 2 and len(split_top(x, ['>>'])) == 1:
        c, t = lin(p[1][1])
        if not t:
            c0, t0 = lin(p[0][1]); return c0 << c, [(k << c, y) for k, y in t0]
        return 0, [(1, x)]
    if len(p) > 1 or len(split_top(x, ['>>'])) > 1: return 0, [(1, x)]
    p = split_top(x, ['+', '-'])
    if len(p) > 1:
        c = 0; ts = []
        for op, y in p:
            k, t = lin(y); sg = 1 if op == '+' else -1
            c += sg*k; ts += [(sg*a, z) for a, z in t]
        return c, ts
    p = split_top(x, ['*'])
    if len(p) == 2:
        a = lin(p[0][1]); b = lin(p[1][1])
        if not a[1]: a, b = b, a
        if not b[1]: return a[0]*b[0], [(k*b[0], t) for k, t in a[1]]
    return 0, [(1, x)]
def emit(c, ts):
    out = []
    for k, t in ts:
        if t == 'local_framePointer' and k == 1: c += BIAS; t = 'lfpb'
        out.append((k, t))
    out.sort(key=lambda kt: 0 if kt[1] in ('lfpb', 'local_stackPointer') else 1)
    st = ''
    for k, t in out:
        if re.search(r'[^\w>().\[\]-]', t): t = '(' + t + ')'
        term = t if abs(k) == 1 else f'({t} * {abs(k)})'
        st += (' + ' if st else '') + term if k > 0 else (' - ' if st else '-') + term
    if c < 0: st = f'{st} - {-c}'; c = 0
    return st or '0', c
def call_args(t, i):
    "t[i] is '(' ; returns list of argument strings and end index after ')'"
    d = 0; args = []; a = i + 1
    for j in range(i, len(t)):
        ch = t[j]
        if ch == '(': d += 1
        elif ch == ')':
            d -= 1
            if d == 0: args.append(t[a:j]); return args, j + 1
        elif ch == ',' and d == 1: args.append(t[a:j]); a = j + 1
FN = {'unsignedLongAt': 'LDQ', 'longAt': 'LDS', 'long64At': 'LDS', 'byteAt': 'LDB', 'byteAtPointer': 'LDB', 'unsignedLongAtput': 'STQ', 'longAtput': 'STQ', 'uint32AtPointer': 'LDW'}
FNRE = re.compile(r'\b(' + '|'.join(sorted(FN, key=len, reverse=True)) + r')\(')
def addrpass(t):
    "the loads and stores of t as LDQ(address, constant) and so on"
    out = ''; i = 0
    while True:
        m = FNRE.search(t, i)
        if not m: return out + t[i:]
        args, j = call_args(t, m.end() - 1)
        out += t[i:m.start()]
        a = addrpass(args[0]); rest = [addrpass(x) for x in args[1:]]
        ma = re.fullmatch(r'\s*\((_sp\w*) = (.*)\)\s*', a, re.S)
        f = FN[m.group(1)]
        if ma or '++' in a or '/*' in a or re.search(r'[^=!<>]=[^=]', a):
            p, c = a, 0
        else:
            k, ts = lin(a); p, c = emit(k, ts)
        out += f'{f}({p}, {c}' + ''.join(', ' + r.strip() for r in rest) + ')'
        i = j
MACROS = '''
#ifdef PHARO_WASM_ADDRESS32	/* a VM with a 32-bit memory: every address is below 4 GB, so addresses are 32-bit locals */
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
def addresses():
    global body
    # signed comparisons of longAt results: keep the types (longAt signed -> used only with masks)
    body = addrpass(body)
    rewrite('\tsqInt local_framePointer;\n\tsqInt local_instructionPointer;\n\tsqInt local_stackPointer;\n',
            '\tiaddr lfpb;\n\tiaddr local_instructionPointer;\n\tiaddr local_stackPointer;\n')
    body = re.sub(r'\blocal_framePointer = ([^;]*);', r'lfpb = (iaddr)(\1) - %d;' % BIAS, body)
    body = re.sub(r'\blocal_framePointer\b', '(lfpb + %d)' % BIAS, body)
    body = MACROS + body

def generic():
    "interpret() becomes interpretGeneric(), which runs one bytecode (with its extensions) or finishes one send, and returns"
    global body
    rewrite('\tsqInt currentBytecode;\n', '\tuint32_t currentBytecode;\n')
    rewrite('sqInt\ninterpret(void)\n{', 'static GENATTR sqInt\ninterpretGeneric(sqInt hotEntry, sqInt hotArg)\n{')
    rewrite('\tcurrentBytecode = LDB(++local_instructionPointer, 0);\n\t/* end fetchByte */\n\t/* end fetchNextBytecode */\n\twhile (1) {', '''	currentBytecode = LDB(local_instructionPointer, 0);
	if (hotEntry == HE_SEND) { lkupClassTag = hotArg; goto commonSendOrdinary; }
	if (hotEntry == HE_EXECUTE) goto hotExecuteNewMethod;
	if (hotEntry == HE_ACTIVATE) goto hotActivateNewMethod;
	if (hotEntry == HE_RETURN) { localReturnValue = hotArg; goto commonReturn; }
	if (hotEntry == HE_CALLERRETURN) { localReturnValue = hotArg; goto commonCallerReturn; }
	while (1) {
		hotGenericExt:''')
    for lab, nxt in (('extABytecode', 'case 225:'), ('extBBytecode', 'case 226:')):
        if body.count(f'VM_LABEL({lab})') != 1:
            fail(f'interpret() has {body.count(f"VM_LABEL({lab})")} {lab}, not 1')
        i = body.index(f'VM_LABEL({lab})'); j = body.find('\t\t\tbreak;\n\t\t\t' + nxt, i)
        if j < 0:
            fail(f'{lab} does not end before {nxt}')
        body = body[:j] + '\t\t\tgoto hotGenericExt;\n\t\t\t' + nxt + body[j + len('\t\t\tbreak;\n\t\t\t' + nxt):]
    # executeNewMethod, and activateNewMethod in it, of the ordinary sends
    m = re.findall(r'\n\t+(l\d+):\n\t+;\n\t+/\* end findNewMethodInClassTag:ifFound: \*/\n\t+/\* begin executeNewMethod: \*/\n', body)
    k = body.index('VM_LABEL(commonSendOrdinary)') if body.count('VM_LABEL(commonSendOrdinary)') == 1 else -1
    end = '\t/* end findNewMethodInClassTag:ifFound: */\n'
    i = body.find(end, k)
    if k < 0 or len(m) != 1 or i < 0:
        fail('commonSendOrdinary does not execute the method that it found')
    i += len(end)
    body = body[:i] + '\t\t\t\t\t\t\t\thotExecuteNewMethod:\n\t\t\t\t\t\t\t\t;\n' + body[i:]
    a = body.find('/* begin activateNewMethod */', i)
    if a < 0 or body.find('VM_LABEL(', i) < a:
        fail('commonSendOrdinary does not activate the method that it found')
    body = body[:a] + 'hotActivateNewMethod:\n' + body[a:]
    loop = '\t\t\tbreak;\n\t\t}\n\t}\n\tlocal_instructionPointer -= 1;'
    rewrite(loop, '\t\t\tbreak;\n\t\t}\n\t\tGIV(framePointer) = (char *)(uintptr_t)(lfpb + %d); GIV(instructionPointer) = local_instructionPointer; GIV(stackPointer) = (char *)(uintptr_t)local_stackPointer;\n\t\treturn 0;\n\t}\n\tlocal_instructionPointer -= 1;' % BIAS)

HASH = '#define MCHASH(s,t) ((s) ^ ((t) << 3))\n#define MCPROBE0(h) (((h) & MethodCacheMask) << 3)\n#define MCPROBE1(h) ((((h) >> 1) & MethodCacheMask) << 3)\n#define MCPROBE2(h) ((((h) >> 2) & MethodCacheMask) << 3)\n'
FILL = '''
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
def caches():
    global head, body, tail
    n = 4096	# entries of 8 words
    redefine('MethodCacheEntries', '0x400', hex(n))
    redefine('MethodCacheEntrySize', '4', '8')
    redefine('MethodCacheMask', '0xFFC', hex((n - 1)*8))
    redefine('MethodCacheSize', '4096', str(n*8))
    rewrite('_iss sqIntptr_t methodCache[', '__attribute__((aligned(64))) _iss sqIntptr_t methodCache[', 1, 'head')
    na = 64
    redefine('AtCacheMask', '0x1C', hex((na - 1)*4))
    redefine('AtPutBase', '32', str(na*4))
    redefine('AtCacheTotalSize', '64', str(na*8))
    rewrite('#define AtCacheFmt 3\n', '#define ATIX(r) (((r) >> 1) & AtCacheMask)\n#define AtCacheFmt 3\n', 1, 'head')
    rewrite('_rcvr & AtCacheMask', 'ATIX(_rcvr)', 2)
    rewrite('#define MethodCacheSelector 1\n', '#define MethodCacheSelector 1\n' + HASH, 1, 'head')
    for where in ('head', 'body'):
        if '(usqInt) _classTag ) << 2)' in globals()[where]:
            fail('the method cache is hashed before interpret() or in it')
    rewrite('GIV(messageSelector) ^ ((sqInt) (((usqInt) _classTag ) << 2) )', 'MCHASH((uint32_t)GIV(messageSelector), (uint32_t)_classTag)', 1, 'tail')
    rewrite('_selector ^ ((sqInt) (((usqInt) _classTag ) << 2) )', 'MCHASH((uint32_t)_selector, (uint32_t)_classTag)', 2, 'tail')
    # fill the activation words of an entry
    i = tail.find('\naddNewMethodToCache(sqInt _classObj)\n{')
    if i < 0:
        fail('there is no addNewMethodToCache() after interpret()')
    j = tail.index('\n}\n', i)
    f = tail[i:j]
    old = 'GIV(methodCache)[_probe + MethodCachePrimFunction] = ((sqIntptr_t) primitiveFunctionPointer );'
    if f.count(old) != 2:
        fail('addNewMethodToCache() fills %d entries, not 2' % f.count(old))
    f = re.sub(r'\n(\t+)' + re.escape(old), lambda m: m.group(0) + '\n' + m.group(1) + 'hotFillEntry(&GIV(methodCache)[_probe], GIV(newMethod));', f)
    tail = tail[:i] + f + tail[j:]
    # what caches what a header says: a method replaced by become: (two-way become: swaps contents in place), or its header stored
    rewrite('/* end followForwardedMethodsInMethodCache */', 'hotRefillMethodCache();\n\t\t\t/* end followForwardedMethodsInMethodCache */', 1, 'tail')
    f = '\nprimitiveObjectAtPut(void)\n{'
    if (head + tail).count(f) != 1:
        fail('there are %d primitiveObjectAtPut()' % (head + tail).count(f))
    where = 'tail' if f in tail else 'head'
    t = globals()[where]; i = t.index(f); j = t.index('\n}\n', i)
    globals()[where] = t[:j] + '\n\tif (_index == 1) hotRefillMethodCache();' + t[j:]

addresses(); generic(); caches()
i = body.index('static GENATTR sqInt')
body = body[:i] + FILL + '#include "%s/interpret.inc"\n' % inc + body[i:]
text = head + body + tail
out = args[1]
try:
    old = open(out).read()
except OSError:
    old = None
if old != text:
    open(out, 'w').write(text)
