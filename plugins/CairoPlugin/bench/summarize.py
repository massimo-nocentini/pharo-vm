#!/usr/bin/env python3
"""summarize.py - the Markdown report of the CairoPlugin benchmark (docs/CairoPlugin.md)

    python3 summarize.py [-o REPORT.md] bench-15-native-cog.txt bench-12-native-cog.txt ...

Reads the files that bench/run-bench.sh writes (or directories holding
bench-*.txt) and prints, or writes to -o, the tables:

- the scenes: per scene, size, Pharo and VM the median of the timed runs of
  each mode (#ffi: stock Athens through the FFI; #basic: the one-for-one
  primitives; #all: basic, shape and text), the speed-ups (FFI median / mode
  median), the noise figure of the #ffi runs (max - min, its share of the
  median, and the drift between the medians of the first and second halves
  of the runs), the FFI callouts of each mode, the primitive calls of the
  plugin modes, the cairo calls (the same in every mode, but the *_destroy
  ones of finalization) and the pixels (the fingerprint, the same in every
  mode);
- the micro-benchmarks: ns per call of each mode and the ratios, with the
  controls (an empty Smalltalk loop, the Smalltalk side of a void FFI
  callout, an FFI callout of an empty C function, a named primitive that does
  nothing), each with its time net of the loop's;
- with --previous, the files of an earlier pass on the same VMs and images:
  how much each median moved between the passes (the reproducibility);
- the conditions (CPU, load average before and after, cairo, image, VM and
  Smalltalk commits) and every line that flags a problem (PIXELS-DIFFER,
  CALLS-DIFFER, FALLBACK, ERROR) or a scene not run (SKIP).

The status is 1 when a line flags PIXELS-DIFFER, CALLS-DIFFER, FALLBACK or
ERROR, else 0.
"""

import argparse
import os
import re
import statistics
import sys

SCENE_ORDER = ['a-polyline', 'a2-lines', 'b-rects', 'c-bezier', 'd-transforms', 'e-text', 'e-text-wide',
               'e-text-long', 'f-roassal-first', 'f-roassal-cached', 'g-roassal-svg-first',
               'g-roassal-svg-cached', 'h-morph-frame']
SCENE_NOTES = {
    'e-text': 'e-text (drawString: users)',
    'e-text-wide': 'e-text-wide (WideString)',
    'e-text-long': 'e-text-long (2000 chars)',
    'h-morph-frame': 'h-morph-frame (800x600, display path on FFI)',
}
VM_ORDER = ['native-cog', 'wasm-node', 'wasm-web']
MODES = ['ffi', 'basic', 'all']
MICRO_ORDER = ['newPath', 'moveToX:Y:', 'primRectangleX:y:width:height:', 'primSetSourceR:g:b:a:',
               'setPathMatrix', 'fill-empty', 'setPaint:+drawShape:', 'drawString:']
CONTROLS = {
    'ctl-loop': 'empty Smalltalk loop (per iteration)',
    'ctl-void-stub-newPath': 'void-stub control: newPath with the C call skipped (Smalltalk side of a void FFI callout)',
    'ctl-empty-ffi-cairo_version': 'empty FFI callout: cairo_version()',
    'ctl-empty-prim-FilePlugin-delimitor': 'a named primitive that does nothing: FilePlugin primitiveDirectoryDelimitor',
    # the control of the runs before 2026-11-18: not empty (two memsets of the counters, about 1 KB)
    'ctl-empty-prim-resetCounters': 'a named primitive that clears the counters: CairoPlugin primResetCounters',
}


def fields(tokens):
    d = {}
    for t in tokens:
        if '=' in t:
            k, v = t.split('=', 1)
            d.setdefault(k, v)
    return d


def parse(paths):
    bench, micro, cond, flags, skips = {}, {}, {}, [], []
    for p in paths:
        with open(p, encoding='utf-8', errors='replace') as f:
            for line in f:
                line = line.rstrip('\n').replace('\r', '')
                t = line.split(' ')
                if t[0] == 'BENCH' and len(t) > 7:
                    pharo, vm, scene, size, mode = t[1], t[2], t[4], int(t[5]), t[6]
                    rest = t[7:]
                    # key=value fields first, then the flags (PIXELS-DIFFER, CALLS-DIFFER, FALLBACK what=n ...)
                    cut = next((i for i, x in enumerate(rest) if '=' not in x), len(rest))
                    kv = fields(rest[:cut])
                    rec = {
                        'version': t[3].split('=', 1)[1],
                        'median': float(kv['median_ms']), 'best': float(kv['best_ms']),
                        'spread': float(kv['spread_ms']), 'drift': float(kv.get('drift_ms', 'nan')),
                        'ffi': int(kv['ffi']), 'prims': int(kv['prims']), 'cairo': int(kv['cairo']),
                        'sha': kv['sha'], 'calls': int(kv.get('calls', '-1')), 'runs': int(kv.get('runs', '0')),
                        'flags': rest[cut:],
                    }
                    bench[(vm, pharo, scene, size, mode)] = rec
                    bad = [x for x in rest if x in ('PIXELS-DIFFER', 'CALLS-DIFFER', 'FALLBACK')]
                    if bad:
                        flags.append(line)
                elif t[0] == 'MICRO' and len(t) >= 6:
                    pharo, vm, name, mode = t[1], t[2], t[3], t[4]
                    kv = fields(t[5:])
                    micro[(vm, pharo, name, mode)] = {'ns': float(kv['ns']), 'best': float(kv.get('best_ns', kv['ns'])),
                                                      'calls': int(kv.get('calls', '0'))}
                elif t[0] == 'COND' and len(t) >= 4:
                    pharo, vm, key = t[1], t[2], t[3]
                    cond.setdefault((vm, os.path.basename(p)), []).append((pharo, key, ' '.join(t[4:])))
                elif t[0] == 'ERROR':
                    flags.append(line)
                elif t[0] == 'SKIP':
                    skips.append(line)
    return bench, micro, cond, flags, skips


def fmt(x, digits=1):
    if x is None:
        return '-'
    if x >= 100:
        return f'{x:.0f}'
    if x >= 10:
        return f'{x:.1f}'
    return f'{x:.{max(digits, 2)}f}'


def ratio(a, b):
    if a is None or b is None or b == 0:
        return '-'
    return f'{a / b:.2f}x'


def sort_key_vm(vm):
    return VM_ORDER.index(vm) if vm in VM_ORDER else len(VM_ORDER)


def scene_rows(bench):
    keys = sorted({(vm, pharo, scene, size) for (vm, pharo, scene, size, mode) in bench},
                  key=lambda k: (sort_key_vm(k[0]), -int(k[1]), -k[3] if k[3] != 800 else -999,
                                 SCENE_ORDER.index(k[2]) if k[2] in SCENE_ORDER else 99))
    lines = ['| scene | size | Pharo | VM | FFI ms | basic ms | all ms | basic × | all × | FFI noise | '
             'FFI callouts (ffi / basic / all) | prims (basic / all) | cairo calls | pixels |',
             '|---|---|---|---|---|---|---|---|---|---|---|---|---|---|']
    for vm, pharo, scene, size in keys:
        r = {m: bench.get((vm, pharo, scene, size, m)) for m in MODES}
        med = {m: (r[m]['median'] if r[m] else None) for m in MODES}
        f = r['ffi']
        if f:
            share = 100.0 * f['spread'] / f['median'] if f['median'] else 0.0
            noise = f'±{fmt(f["spread"])} ({share:.0f}%), drift {f["drift"]:+.2f}'
        else:
            noise = '-'
        callouts = ' / '.join(str(r[m]['ffi']) if r[m] else '-' for m in MODES)
        prims = ' / '.join(str(r[m]['prims']) if r[m] else '-' for m in ('basic', 'all'))
        calls = {r[m]['calls'] for m in MODES if r[m]}
        calls_s = str(calls.pop()) if len(calls) == 1 else 'DIFFER ' + ' / '.join(str(r[m]['calls']) if r[m] else '-' for m in MODES)
        shas = {r[m]['sha'] for m in MODES if r[m]}
        pixels = ('equal `' + next(iter(shas)) + '`') if len(shas) == 1 else '**DIFFER**'
        fl = sorted({x for m in MODES if r[m] for x in r[m]['flags']})
        if any(x == 'FALLBACK' for x in fl):
            pixels += ' **FALLBACK**'
        size_s = '800×600' if scene == 'h-morph-frame' else str(size)
        lines.append(f'| {SCENE_NOTES.get(scene, scene)} | {size_s} | {pharo} | {vm} | {fmt(med["ffi"])} | {fmt(med["basic"])} | '
                     f'{fmt(med["all"])} | {ratio(med["ffi"], med["basic"])} | {ratio(med["ffi"], med["all"])} | {noise} | '
                     f'{callouts} | {prims} | {calls_s} | {pixels} |')
    return lines


def micro_rows(micro):
    out = []
    groups = sorted({(vm, pharo) for (vm, pharo, name, mode) in micro}, key=lambda k: (sort_key_vm(k[0]), -int(k[1])))
    for vm, pharo in groups:
        out.append(f'**Pharo {pharo}, {vm}** (median ns per call)')
        out.append('')
        out.append('| call | FFI ns | basic ns | all ns | FFI / basic | FFI / all | calls per run |')
        out.append('|---|---|---|---|---|---|---|')
        names = [n for n in MICRO_ORDER if any((vm, pharo, n, m) in micro for m in MODES)]
        names += sorted({n for (v, p, n, m) in micro if v == vm and p == pharo and n not in MICRO_ORDER and n not in CONTROLS})
        for n in names:
            v = {m: (micro[(vm, pharo, n, m)]['ns'] if (vm, pharo, n, m) in micro else None) for m in MODES}
            calls = max((micro[(vm, pharo, n, m)]['calls'] for m in MODES if (vm, pharo, n, m) in micro), default=0)
            out.append(f'| `{n}` | {fmt(v["ffi"])} | {fmt(v["basic"])} | {fmt(v["all"])} | {ratio(v["ffi"], v["basic"])} | '
                       f'{ratio(v["ffi"], v["all"])} | {calls} |')
        ctl = [(n, m) for (v, p, n, m) in micro if v == vm and p == pharo and n in CONTROLS]
        if ctl:
            out.append('')
            loop = next((micro[(vm, pharo, cn, m)]['ns'] for (cn, m) in ctl if cn == 'ctl-loop'), None)
            out.append('| control | ns (the loop included) | ns, net of the loop |')
            out.append('|---|---|---|')
            for n in CONTROLS:
                for (cn, m) in ctl:
                    if cn == n:
                        ns = micro[(vm, pharo, n, m)]['ns']
                        net = '-' if n == 'ctl-loop' or loop is None else fmt(ns - loop)
                        out.append(f'| {CONTROLS[n]} | {fmt(ns)} | {net} |')
        out.append('')
    return out


def summary_rows(bench):
    """One line per VM and Pharo: the geometric mean of the speed-ups at size 1000 and at size 1"""
    out = []
    groups = sorted({(vm, pharo) for (vm, pharo, s, z, m) in bench}, key=lambda k: (sort_key_vm(k[0]), -int(k[1])))
    for vm, pharo in groups:
        parts = []
        for size in sorted({z for (v, p, s, z, m) in bench if v == vm and p == pharo}, reverse=True):
            for mode in ('basic', 'all'):
                rs = []
                for (v, p, s, z, m), rec in bench.items():
                    if v == vm and p == pharo and z == size and m == mode:
                        f = bench.get((v, p, s, z, 'ffi'))
                        if f and rec['median'] > 0:
                            rs.append(f['median'] / rec['median'])
                if rs:
                    parts.append(f'{mode} {statistics.geometric_mean(rs):.2f}x at size {size} ({len(rs)} scenes)')
        if parts:
            out.append(f'- Pharo {pharo}, {vm}: ' + '; '.join(parts))
    return out


def expand(paths):
    files = []
    for p in paths:
        if os.path.isdir(p):
            files += sorted(os.path.join(p, f) for f in os.listdir(p) if f.startswith('bench-') and f.endswith('.txt'))
        else:
            files.append(p)
    return files


def repeat_rows(bench, previous):
    lines = ['| scene | size | Pharo | VM | FFI ms (earlier → this) | basic ms | all ms | all × (earlier → this) |',
             '|---|---|---|---|---|---|---|---|']
    moves = []
    # (only the same workload: the same cairo calls in both passes)
    keys = sorted({(vm, pharo, scene, size) for (vm, pharo, scene, size, mode) in bench
                   if (vm, pharo, scene, size, 'ffi') in previous and (vm, pharo, scene, size, 'ffi') in bench
                   and previous[(vm, pharo, scene, size, 'ffi')]['calls'] == bench[(vm, pharo, scene, size, 'ffi')]['calls']},
                  key=lambda k: (sort_key_vm(k[0]), -int(k[1]), -k[3] if k[3] != 800 else -999,
                                 SCENE_ORDER.index(k[2]) if k[2] in SCENE_ORDER else 99))
    for vm, pharo, scene, size in keys:
        cells = []
        for m in MODES:
            a, b = previous.get((vm, pharo, scene, size, m)), bench.get((vm, pharo, scene, size, m))
            if a and b and a['median'] > 0:
                d = (b['median'] - a['median']) / a['median']
                moves.append(abs(d))
                cells.append(f'{fmt(a["median"])} → {fmt(b["median"])} ({100 * d:+.0f}%)')
            else:
                cells.append('-')
        sx = ratio(previous[(vm, pharo, scene, size, 'ffi')]['median'], previous[(vm, pharo, scene, size, 'all')]['median']) \
            if (vm, pharo, scene, size, 'all') in previous else '-'
        sy = ratio(bench[(vm, pharo, scene, size, 'ffi')]['median'], bench[(vm, pharo, scene, size, 'all')]['median']) \
            if (vm, pharo, scene, size, 'all') in bench and (vm, pharo, scene, size, 'ffi') in bench else '-'
        size_s = '800×600' if scene == 'h-morph-frame' else str(size)
        lines.append(f'| {scene} | {size_s} | {pharo} | {vm} | ' + ' | '.join(cells) + f' | {sx} → {sy} |')
    if moves:
        moves.sort()
        lines += ['', f'Medians compared: {len(moves)}; the median move {100 * moves[len(moves) // 2]:.1f}%, '
                      f'90% of them within {100 * moves[int(0.9 * (len(moves) - 1))]:.1f}%, the largest {100 * moves[-1]:.1f}%.']
    return lines


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('-o', '--output')
    ap.add_argument('--previous', action='append', default=[],
                    help='the files (or directories) of an earlier pass: a table of how much each median moved')
    ap.add_argument('paths', nargs='+')
    a = ap.parse_args()
    files = expand(a.paths)
    bench, micro, cond, flags, skips = parse(files)
    out = ['# CairoPlugin benchmark', '',
           'Generated by `plugins/CairoPlugin/bench/summarize.py` from ' + ', '.join(f'`{os.path.basename(f)}`' for f in files) + '.',
           '',
           'Modes, in one image and one VM, interleaved per repetition: **FFI** = stock Athens through uFFI; '
           '**basic** = the one-for-one primitives; **all** = basic + shape + text. Times are the median of the timed runs, '
           'each after a mode switch, an untimed warm-up and a garbage collection. "×" = FFI median / mode median. '
           'FFI noise = max − min of the FFI runs (its share of the median) and the drift between the medians of their '
           'first and second halves. Size 1 is the 1 px control (the same calls, nothing to rasterize). '
           'Cairo calls = FFI callouts + the plugin\'s cairo calls, without the `*_destroy` calls of finalization; '
           'pixels = the fingerprint of the surface (SHA-256 of the CRC-32s of its 64 KiB chunks), the same in every mode.',
           '']
    s = summary_rows(bench)
    if s:
        out += ['## Summary (geometric mean of the scene speed-ups)', ''] + s + ['']
    out += ['## Scenes', ''] + scene_rows(bench) + ['']
    out += ['## Micro-benchmarks', ''] + micro_rows(micro)
    if a.previous:
        previous = parse(expand(a.previous))[0]
        out += ['## Reproducibility (an earlier pass of the same runs)', '',
                'Earlier pass: ' + ', '.join(f'`{os.path.basename(os.path.dirname(os.path.abspath(f)))}/{os.path.basename(f)}`'
                                             for f in expand(a.previous))
                + ' (scenes whose workload changed since, by their cairo calls, are left out).', '']
        out += repeat_rows(bench, previous) + ['']
    if flags or skips:
        out += ['## Flags and skips', '']
        out += ['- `' + x + '`' for x in flags + skips]
        out.append('')
    out += ['## Conditions', '']
    for (vm, name), items in sorted(cond.items(), key=lambda k: (sort_key_vm(k[0][0]), k[0][1])):
        out.append(f'**{name}** ({vm})')
        out.append('')
        for pharo, key, value in items:
            if key in ('scenes',):
                continue
            out.append(f'- {key}: {value}')
        out.append('')
    text = '\n'.join(out) + '\n'
    if a.output:
        with open(a.output, 'w', encoding='utf-8') as f:
            f.write(text)
    else:
        sys.stdout.write(text)
    bad = [x for x in flags if 'PIXELS-DIFFER' in x or 'CALLS-DIFFER' in x or 'FALLBACK' in x or x.startswith('ERROR')]
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main())
