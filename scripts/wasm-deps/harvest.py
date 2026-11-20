#!/usr/bin/env python3
# harvest.py - turn a meson or CMake (Ninja) build of a library for
# Emscripten into what cmake/emscripten/deps builds it from: a source list
# with the compile flags of each static library, and the files that the
# build generated
#
#   python3 -I harvest.py --src <source dir> --build <build dir>
#       --out <dir in cmake/emscripten/deps> --name <library>
#       --target <archive name>=<VARIABLE> [--target ...]
#       [--keep-define <regex>]
#
# The build must have been made with Ninja, so that it has
# compile_commands.json and the dependencies of its objects (ninja -t deps).
# For each --target, the archive whose compile commands write objects under
# <archive name>.p/ (meson) or <archive name>.dir/ (CMake), it writes into
# <out>/sources.cmake
#
#     set(PHARO_WASM_<VARIABLE>_SOURCES ...)      the sources
#     set(PHARO_WASM_<VARIABLE>_<LANG>_DEFINES ...)   (LANG: C, CXX)
#     set(PHARO_WASM_<VARIABLE>_<LANG>_INCLUDES ...)
#     set(PHARO_WASM_<VARIABLE>_<LANG>_OPTIONS ...)
#
# with the paths under the source directory as ${src}/..., and those under
# the build directory as ${gen}/...: the generated sources, and the
# generated headers that an object of a target included (ninja -t deps),
# are copied into <out>/gen, keeping their paths.  Include directories
# outside both (the headers of the libraries it depends on, installed in a
# prefix) are left out: cmake/emscripten/deps gives them through the
# targets it links.  So are the flags of the cross file and of the
# toolchain (-m64, -s..., optimisation, debugging, warnings, -fPIC,
# -pthread): the deps directory sets its own.  Every object of a target
# must have the same flags, per language, or it stops.
import argparse, collections, json, os, re, shlex, shutil, subprocess, sys

ap = argparse.ArgumentParser()
ap.add_argument('--src', required=True)
ap.add_argument('--build', required=True)
ap.add_argument('--out', required=True)
ap.add_argument('--name', required=True)
ap.add_argument('--target', action='append', required=True)
ap.add_argument('--ninja', default='ninja')
ap.add_argument('--note', action='append', default=[])
ap.add_argument('--redact', action='append', default=[],
                help='a directory whose path the copied files must not keep (the work directory)')
args = ap.parse_args()

src = os.path.realpath(args.src)
build = os.path.realpath(args.build)
out = os.path.abspath(args.out)
gendir = os.path.join(out, 'gen')
targets = collections.OrderedDict()
for t in args.target:
    name, var = t.split('=')
    targets[name] = var

DROP = re.compile(r'^-(W.*|w|O.*|g.*|m64|s[A-Z_].*|MD|MMD|pipe|fdiagnostics.*|fcolor.*|fPIC|fpic|pthread|'
                  r'fstack-protector.*|fno-omit-frame-pointer|D_FORTIFY_SOURCE.*|U_FORTIFY_SOURCE|'
                  r'fmacro-prefix-map=.*|ffile-prefix-map=.*|fdebug-prefix-map=.*)$')
WITH_ARG = {'-o', '-MF', '-MQ', '-MT', '-include', '-isystem', '-I', '-D', '-U', '-x', '-target', '--target'}

def where(path, directory):
    p = os.path.realpath(os.path.join(directory, path))
    if p == src or p.startswith(src + os.sep):
        return '${src}' + p[len(src):], p
    if p == build or p.startswith(build + os.sep):
        return '${gen}' + p[len(build):], p
    return None, p

def target_of(output):
    for name in targets:
        for mark in (name + '.p/', name + '.dir/'):
            if output.startswith(mark) or ('/' + mark) in output:
                return name
    return None

cc = json.load(open(os.path.join(build, 'compile_commands.json')))
per = collections.OrderedDict((name, {'sources': [], 'flags': {}, 'objects': []}) for name in targets)
for e in cc:
    words = shlex.split(e['command']) if 'command' in e else list(e['arguments'])
    output = e.get('output')
    if output is None:
        output = words[words.index('-o') + 1]
    name = target_of(output)
    if name is None:
        continue
    lang = 'CXX' if re.search(r'\.(cc|cpp|cxx|C)$', e['file']) else 'C'
    defines, includes, options = [], [], []
    i = 1
    sourcepath = os.path.realpath(os.path.join(e['directory'], e['file']))
    while i < len(words):
        w = words[i]
        arg = None
        if w in WITH_ARG and i + 1 < len(words):
            arg = words[i + 1]
            i += 2
        else:
            i += 1
        if w in ('-c',):
            continue
        if arg is None and not w.startswith('-'):
            if os.path.realpath(os.path.join(e['directory'], w)) == sourcepath:
                continue
            sys.exit(f'{e["file"]}: unexpected word {w}')
        if w == '-o' or w in ('-MF', '-MQ', '-MT', '-target', '--target'):
            continue
        if w == '-x':
            continue
        if w == '-D' or w.startswith('-D'):
            d = arg if w == '-D' else w[2:]
            if not re.match(r'_FORTIFY_SOURCE', d):
                defines.append(d)
            continue
        if w == '-U' or w.startswith('-U'):
            options.append('-U' + (arg if w == '-U' else w[2:]))
            continue
        if w in ('-I', '-isystem') or w.startswith('-I') or w.startswith('-isystem'):
            d = arg if arg is not None else (w[2:] if w.startswith('-I') else w[len('-isystem'):])
            mapped, _ = where(d, e['directory'])
            if mapped is not None:
                includes.append(mapped)
            continue
        if w == '-include':
            mapped, real = where(arg, e['directory'])
            options.append('-include')
            options.append(mapped if mapped else real)
            continue
        if DROP.match(w):
            continue
        options.append(w)
    key = (tuple(defines), tuple(includes), tuple(options))
    flags = per[name]['flags']
    if lang in flags and flags[lang] != key:
        sys.exit(f'{name}: {e["file"]} has other {lang} flags than the objects before it:\n  {key}\n  {flags[lang]}')
    flags[lang] = key
    mapped, real = where(sourcepath, '/')
    if mapped is None:
        sys.exit(f'{name}: {real} is neither in the sources nor in the build')
    per[name]['sources'].append((mapped, real))
    per[name]['objects'].append(output)

for name, p in per.items():
    if not p['sources']:
        sys.exit(f'no compile command writes objects of {name}')

# The generated files: the generated sources, and every header under the
# build directory that an object of a target depends on
deps = subprocess.run([args.ninja, '-C', build, '-t', 'deps'], capture_output=True, text=True, check=True).stdout
objects = {o for p in per.values() for o in p['objects']}
generated = set()
current = None
for line in deps.splitlines():
    if line and not line.startswith(' '):
        current = line.split(':')[0].strip()
        continue
    if current in objects and line.strip():
        mapped, real = where(line.strip(), build)
        if mapped and mapped.startswith('${gen}'):
            generated.add(real)
for p in per.values():
    for mapped, real in p['sources']:
        if mapped.startswith('${gen}'):
            generated.add(real)
if os.path.isdir(gendir):
    shutil.rmtree(gendir)
for real in sorted(generated):
    dest = os.path.join(gendir, os.path.relpath(real, build))
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    shutil.copyfile(real, dest)
    # paths of the machine that made them, left in comments, are not ours
    try:
        text = open(dest, encoding='utf-8').read()
    except UnicodeDecodeError:
        continue
    for path, label in ((build, '<build>'), (src, '<source>')):
        text = text.replace(path, label)
    # the tools that made them (gperf), by their names
    text = re.sub(r'/[^\s\'"]*/bin/([A-Za-z0-9_.+-]+)', r'\1', text)
    for path in args.redact:
        if os.path.realpath(path) in text or os.path.abspath(path) in text:
            sys.exit(f'{dest} still names {path}')
    open(dest, 'w', encoding='utf-8').write(text)

def present(inc):
    if inc.startswith('${gen}'):
        return os.path.isdir(os.path.join(gendir, inc[len('${gen}/'):] if inc != '${gen}' else ''))
    return True

def cmake_list(name, items):
    if not items:
        return f'set({name})\n'
    body = ''.join(f'\n    {cmake_quote(i)}' for i in items)
    return f'set({name}{body})\n'

def cmake_quote(s):
    if re.match(r'^[A-Za-z0-9_./${}=:+-]+$', s):
        return s
    return '"' + s.replace('\\', '\\\\').replace('"', '\\"') + '"'

lines = [f'# The sources of {args.name} for the Emscripten VM, and their flags: generated by\n',
         f'# scripts/wasm-deps/harvest.py from a build of it for Emscripten\n',
         f'# (scripts/wasm-deps/regen-text-stack.sh).  Do not edit.  ${{src}} is the\n',
         f'# directory of the unpacked and patched archive, ${{gen}} the gen/ directory\n',
         f'# next to this file (what that build generated).\n']
for n in args.note:
    lines.append(f'# {n}\n')
for name, var in targets.items():
    p = per[name]
    lines.append(f'\n# {name}\n')
    lines.append(cmake_list(f'PHARO_WASM_{var}_SOURCES', [m for m, _ in p['sources']]))
    for lang, (defines, includes, options) in p['flags'].items():
        lines.append(cmake_list(f'PHARO_WASM_{var}_{lang}_DEFINES', list(defines)))
        lines.append(cmake_list(f'PHARO_WASM_{var}_{lang}_INCLUDES', [i for i in includes if present(i)]))
        lines.append(cmake_list(f'PHARO_WASM_{var}_{lang}_OPTIONS', list(options)))
os.makedirs(out, exist_ok=True)
open(os.path.join(out, 'sources.cmake'), 'w').write(''.join(lines))
print(f'{args.name}: ' + ', '.join(f'{n} {len(per[n]["sources"])} sources' for n in targets) +
      f', {len(generated)} generated files')
