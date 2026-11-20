#!/usr/bin/env python3
# castscan.py - the function pointer casts of a build that trap in WebAssembly
#
#   python3 -I castscan.py <compile_commands.json> [<file filter>]
#
# WebAssembly checks the type of every call through a function pointer
# (call_indirect): a function called through a pointer of another type
# traps ("null function or function signature mismatch"), where natively
# the extra arguments are ignored.  GLib and its users cast functions to
# the types of its callbacks (GDestroyNotify, GFunc, GCompareFunc...), and
# so they trap where the two types differ as WebAssembly signatures.  This
# script compiles each source of the compile commands (whose file name
# contains <file filter>) with -fsyntax-only and clang's -Wcast-function-type
# alone (emcc and em++ must be on the PATH), and prints the casts whose
# source and target types differ as wasm64 signatures: another number of
# parameters, a void and a non-void result, or a parameter of another
# WebAssembly type, with the two signatures (v void, i i32, j i64, f f32,
# d f64; '?' a type it does not know).  Casts to GCallback
# (void (*)(void)) are left out: G_CALLBACK casts every signal handler,
# which the marshaller calls with the signal's types (check those by running
# the code).  A function stored with a cast and cast back to its own type
# before the call is harmless, and printed too: each line must be read.
# The patches of cmake/emscripten/deps/*/patches fix those that are called.
import concurrent.futures as cf, json, re, shlex, subprocess, sys

ANSI = re.compile(r'\x1b\[[0-9;]*m')
ENUMS = {'GIOCondition', 'GParamFlags', 'GSignalFlags', 'GConnectFlags'}


def warnings(entry):
    words = shlex.split(entry['command']) if 'command' in entry else list(entry['arguments'])
    command, skip = [], False
    for word in words:
        if skip:
            skip = False
            continue
        if word == '-o':
            skip = True
            continue
        if word == '-c' or word.startswith('-W') or word == '-w':
            continue
        command.append(word)
    if command[0].endswith('-nopthread'):
        command[0] = 'em++' if '++' in command[0] else 'emcc'
    command += ['-fsyntax-only', '-Wno-everything', '-Wcast-function-type']
    run = subprocess.run(command, cwd=entry['directory'], capture_output=True, text=True)
    return [ANSI.sub('', line) for line in run.stderr.splitlines() if 'warning:' in line]


def split_parameters(text):
    out, depth, current = [], 0, ''
    for c in text:
        if c in '([':
            depth += 1
        if c in ')]':
            depth -= 1
        if c == ',' and depth == 0:
            out.append(current.strip())
            current = ''
        else:
            current += c
    if current.strip():
        out.append(current.strip())
    return out


def function_type(text):
    """'RET (*)(PARAMS)' as (RET, [PARAMS])"""
    i = text.find('(*)')
    if i < 0:
        return None
    result, rest = text[:i].strip(), text[i + 3:].strip()
    if not rest.startswith('('):
        return None
    depth = 0
    for j, c in enumerate(rest):
        if c == '(':
            depth += 1
        elif c == ')':
            depth -= 1
            if depth == 0:
                break
    parameters = split_parameters(rest[1:j])
    return result, ([] if parameters == ['void'] else parameters)


def wasm_type(text):
    t = re.sub(r'\b(const|volatile|struct|enum|union|signed)\b', '', text).strip()
    if t == 'void':
        return 'v'
    if '*' in t or '(' in t or '[' in t:
        return 'j'
    if t == '...':
        return '...'
    if t == 'double':
        return 'd'
    if t == 'float':
        return 'f'
    if t == 'long double':
        return 'q'
    if re.search(r'\blong\b', t) or t in ('size_t', 'ssize_t', 'gsize', 'gssize', 'int64_t', 'uint64_t',
                                         'intptr_t', 'uintptr_t'):
        return 'j'
    if re.search(r'\b(int|char|short|_Bool|bool|unsigned)\b', t) or t in ENUMS or t.startswith('_cairo_'):
        return 'i'
    return '?' + t


def signature(f):
    return wasm_type(f[0]) + ''.join(wasm_type(p) for p in f[1])


def main():
    entries = json.load(open(sys.argv[1]))
    only = sys.argv[2] if len(sys.argv) > 2 else ''
    found = set()
    with cf.ThreadPoolExecutor(32) as pool:
        for lines in pool.map(warnings, [e for e in entries if only in e['file']]):
            found.update(lines)
    for line in sorted(found):
        m = re.search(r"cast from '([^']*)'(?: \(aka '([^']*)'\))? to '([^']*)'(?: \(aka '([^']*)'\))?", line)
        if not m:
            continue
        a, b = m.group(2) or m.group(1), m.group(4) or m.group(3)
        if b == 'void (*)(void)':
            continue
        fa, fb = function_type(a), function_type(b)
        if not fa or not fb:
            print('UNPARSED', line)
            continue
        sa, sb = signature(fa), signature(fb)
        if sa != sb:
            print(f"{sa} -> {sb}  {line.split(': warning')[0]}  [{a} -> {b}]")


main()
