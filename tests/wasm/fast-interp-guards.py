#!/usr/bin/env python3
"""fast-interp-guards.py - the guards of the fast interpreter, against a generated interpreter that is changed on purpose

usage: fast-interp-guards.py SRCDIR GEN WORK CC...

GEN holds the generated sources (vm/src/interp.c, vm/include/interp.h), WORK is a scratch directory, and CC a command
that compiles for WebAssembly, with the include directories and the defines of the VM.

The generated interpreter is first made into the fast one (cmake/emscripten/fastInterpreter.py) and compiled, with
64-bit and with 32-bit addresses: both must work.  Then copies of it with one change each, of something that the fast
interpreter repeats or assumes, are, and each change must stop the script, or the compiler, with the message of its
guard.  Prints a line per group of changes, and exits 1 when one passed unseen.  Lane 78 runs it."""
import concurrent.futures, os, re, subprocess, sys

srcdir, gen, work = sys.argv[1:4]
cc = sys.argv[4:]
script = os.path.join(srcdir, 'cmake/emscripten/fastInterpreter.py')
inc = os.path.join(srcdir, 'src/emscripten/fastinterp')
interp = open(os.path.join(gen, 'vm/src/interp.c')).read()
header = open(os.path.join(gen, 'vm/include/interp.h')).read()
checks = open(os.path.join(inc, 'checks.inc')).read()
os.makedirs(work, exist_ok=True)

def run(command):
    r = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, universal_newlines=True)
    return r.returncode, r.stdout

def build(name, text, interp_h=None, flags=(), compile_it=True):
    "the status and the output of the script, then of the compiler, for the generated interpreter text"
    d = os.path.join(work, name)
    os.makedirs(d, exist_ok=True)
    with open(os.path.join(d, 'interp.c'), 'w') as f:
        f.write(text)
    status, output = run([sys.executable, '-I', script, os.path.join(d, 'interp.c'), os.path.join(d, 'fast.c'), inc])
    if status or not compile_it:
        return 'script' if status else '', output
    first = []
    if interp_h is not None:
        with open(os.path.join(d, 'interp.h'), 'w') as f:
            f.write(interp_h)
        first = ['-I' + d]
    status, output = run(cc[:1] + first + cc[1:] + list(flags) + ['-w', '-Wno-int-conversion', '-ferror-limit=0', '-fsyntax-only', os.path.join(d, 'fast.c')])
    return 'compiler' if status else '', output

# ---- the changes: (group, name, interp.c, interp.h or None, flags, what the message must have in it)

changes = []
def change(group, name, text, message, interp_h=None, flags=()):
    changes.append((group, name, text, interp_h, flags, message))
def replaced(text, old, new, count=1):
    if old not in text:
        raise SystemExit('fast-interp-guards: the generated code has not %r (the guard of it is not tested)' % old[:200])
    return text.replace(old, new, count)

s = interp.index('\nsqInt\ninterpret(void)\n{')
def inCase(n, old, new):
    "interp.c with old become new in the case of the bytecode n"
    i = interp.index('\t\t\tcase %d:\n' % n, s)
    j = interp.index('\n\t\t\tbreak;\n', i)
    return interp[:i] + replaced(interp[i:j], old, new) + interp[j:]
def inFunction(f, old, new):
    m = re.search(r'^%s\([^;{}]*\)\n\{' % f, interp, re.M)
    i, j = m.start(), interp.index('\n}\n', m.end())
    return interp[:i] + replaced(interp[i:j], old, new) + interp[j:]

def names(*numbers):
    "whether a message says that the code of these bytecodes has changed"
    def test(output):
        m = re.search(r'has changed: bytecodes ((?:\d+(?:-\d+)?(?:, (?=\d))?)+)', output)
        ranges = [r.split('-') for r in m.group(1).split(', ')] if m else []
        return all(any(int(r[0]) <= n <= int(r[-1]) for r in ranges) for n in numbers)
    return test

# every constant of checks.inc
for name, value in re.findall(r'^FASTINTERP_IS\((.+), ([^,]+)\);$', checks, re.M):
    what = 'assumes that %s is' % name
    if name.startswith('sizeof'):
        continue
    if name in ('IMMUTABILITY',):
        change('constants', name, interp, what, flags=['-U' + name, '-D%s=0' % name])
        continue
    define = re.compile(r'^(#define %s )(.*)$' % re.escape(name), re.M)
    other = lambda m: m.group(1) + '(%s + 1)' % m.group(2) if not re.fullmatch(r'-?\w+', m.group(2)) else m.group(1) + str(int(m.group(2), 0) + 1)
    if define.search(interp):
        # (a define that the script rewrites, or that its own arithmetic uses, stops the script)
        change('constants', name, define.sub(other, interp), re.compile(r'(assumes that |has not |rewrites:\n#define )%s(?!\w)' % re.escape(name)))
    elif define.search(header):
        change('constants', name, interp, what, interp_h=define.sub(other, header))
    else:
        # a define of the headers of the VM
        change('constants', name, interp, what, flags=['-include', os.path.join(work, 'platform.h'), '-DCHANGE_' + re.sub(r'\W', '', name)])
with open(os.path.join(work, 'platform.h'), 'w') as f:
    f.write('#include "sq.h"\n#ifdef CHANGE_VMBIGENDIAN\n# undef VMBIGENDIAN\n# define VMBIGENDIAN 1\n#endif\n'
            '#ifdef CHANGE_POLL\n# undef PHARO_POLL_HEARTBEAT\n# define PHARO_POLL_HEARTBEAT() ioHeartbeatPoll()\n#endif\n')
change('constants', 'the poll of the heartbeat', interp, 'poll of the heartbeat', flags=['-include', os.path.join(work, 'platform.h'), '-DCHANGE_POLL'])

# the bytecodes that handlers.inc handles
handled = [int(n) for n in re.findall(r'^\w+  bytecode (\d+)$', open(os.path.join(inc, 'generated.txt')).read(), re.M)]
for n in handled:
    change('bytecodes', 'bytecode %d' % n, inCase(n, 'VM_LABEL(', 'changed(); VM_LABEL('), names(n))
text = interp[:s] + re.sub(r'(?m)^\t\t\tcase (96|97):$', lambda m: '\t\t\tcase %d:' % (193 - int(m.group(1))), interp[s:])
change('bytecodes', 'plus and minus exchanged', text, names(96, 97))
change('special selectors', 'another selector of bytecode 115', inCase(115, '(19 * 2)', '(20 * 2)'), names(115))
change('write barrier', 'the store of a slot', inCase(207, 'newSpaceMask', 'oldSpaceMask'), names(200, 207))
change('write barrier', 'the store into a temporary vector', inCase(253, 'possibleOldObjectStoreInto: */', 'possibleOldObjectStoreInto: */ changed();'), names(253))
change('write barrier', 'the start of perm space', interp.replace('0x20000000000LL', '0x40000000000LL'), 'the start of perm space')
change('frames', 'the fixed part of a frame', inCase(234, 'unsignedLongAtput((_sp22 = local_stackPointer - BytesPerWord), GIV(newMethod));', 'unsignedLongAtput((_sp22 = local_stackPointer - BytesPerWord), GIV(nilObj));'), names(234))
change('frames', 'a return', inCase(88, 'VM_LABEL(commonReturn)', 'changed(); VM_LABEL(commonReturn)'), 'shared commonReturn')

# the primitives and the other functions
for f in re.findall(r'^\w+  function (\w+)$', open(os.path.join(inc, 'generated.txt')).read(), re.M):
    change('functions', f, inFunction(f, '\n{', '\n{ changed();'), re.compile(r'has changed: [^.]*function %s' % f))
    change('functions', f + ', gone', re.sub(r'(?m)^%s\(' % f, f + 'Other(', interp), re.compile(r'no function %s|has changed|has \d+ times|there is no|other functions|there are 0' % f))
for f, old, new in (('isSmallFloatValue', '_exponent <= (0xFF +', '_exponent <= (0x7F +'),
                    ('initializeHeaderOfStartAddressnumSlotsformatclassIndexpinned', '_pinFlag = ((sqInt) _isPinned );', '_pinFlag = ((sqInt) _isPinned ) + 2;'),
                    ('fetchClassOfNonImm', '_classTablePage == GIV(nilObj)', '_classTablePage == GIV(trueObj)')):
    change('functions that the others call', f, inFunction(f, old, new), re.compile(r'has changed: function %s\.' % f))
change('primitive numbers', 'a longer range of quick primitives', replaced(interp, '#define MaxQuickPrimitiveIndex 519', '#define MaxQuickPrimitiveIndex 520'), 'assumes that MaxQuickPrimitiveIndex')

# the caches
change('caches', 'another user of the method cache', inFunction('primitiveIdentityHash', '{', '{ GIV(methodCache)[1] = 0;'), 'other functions use the methodCache')
change('caches', 'another user of the at cache', inFunction('primitiveIdentityHash', '{', '{ GIV(atCache)[1] = 0;'), 'other functions use the atCache')
change('caches', 'a bigger method cache', replaced(interp, '#define MethodCacheEntries 0x400', '#define MethodCacheEntries 0x800'), 'MethodCacheEntries')
change('caches', 'a bigger at cache', replaced(interp, '#define AtCacheMask 0x1C', '#define AtCacheMask 0x3C'), 'AtCacheMask')
change('caches', 'another hash', interp.replace('(usqInt) _classTag ) << 2)', '(usqInt) _classTag ) << 3)'), 'has changed')
change('caches', 'become: without the method cache', replaced(interp, '/* end followForwardedMethodsInMethodCache */', ''), 'followForwardedMethodsInMethodCache')

# the rewriting
change('rewriting', 'no interpret()', replaced(interp, '\nsqInt\ninterpret(void)\n{', '\nsqInt\ninterpretIt(void)\n{'), 'interpret() functions')
change('rewriting', 'a bytecode without a case', replaced(interp, '\t\t\tcase 224:\n', '\t\t\tcase 324:\n'), '256 bytecodes')
change('rewriting', 'another dispatch', interp[:s] + replaced(interp[s:], '\tsqInt currentBytecode;\n', '\tusqInt currentBytecode;\n'), 'that it rewrites')
change('rewriting', 'another send', interp.replace('VM_LABEL(commonSendOrdinary)', 'VM_LABEL(commonSend)'), re.compile('commonSendOrdinary|has changed'))

# ---- the check

bad = 0
for name, flags in (('as it is', []), ('as it is, with 32-bit addresses', ['-DPHARO_WASM_ADDRESS32=1'])):
    where, output = build(re.sub(r'\W+', '-', name), interp, flags=flags)
    if where:
        print('FAIL the generated interpreter %s: the %s stops\n%s' % (name, where, output[-3000:]))
        bad += 1
if bad:
    sys.exit(1)
print('ok   the generated interpreter as it is, with 64-bit and with 32-bit addresses')

def check(c):
    group, name, text, interp_h, flags, message = c
    if text == interp and interp_h in (None, header) and not flags:
        return 'nothing was changed'
    d = re.sub(r'\W+', '-', group + '-' + name)
    where, output = build(d, text, interp_h, flags)
    for f in ('interp.c', 'fast.c'):
        try:
            os.remove(os.path.join(work, d, f))
        except OSError:
            pass
    if not where:
        return 'passed unseen'
    found = message in output if isinstance(message, str) else message(output) if callable(message) else message.search(output)
    return None if found else 'the %s stopped, but not with its message: %s' % (where, output[:400])

jobs = min(16, os.cpu_count() or 2)
groups = {}
with concurrent.futures.ThreadPoolExecutor(jobs) as pool:
    for c, problem in zip(changes, pool.map(check, changes)):
        groups.setdefault(c[0], []).append((c[1], problem))
for group, l in groups.items():
    failed = [(n, p) for n, p in l if p]
    for n, p in failed:
        print('FAIL %s: %s: %s' % (group, n, p))
    bad += len(failed)
    if not failed:
        print('ok   %s: %d changes, each stopped' % (group, len(l)))
sys.exit(1 if bad else 0)
