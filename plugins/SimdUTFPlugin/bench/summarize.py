# summarize.py - the tables of docs/SimdUTF.md from the output of SUBench
#
# usage: python3 -I plugins/SimdUTFPlugin/bench/summarize.py TSV...
#
# Reads the LABEL.runN.tsv files of run-bench.sh (the INFO line of each gives
# its VM and run) and prints, in Markdown: the gates, the speedups over Zinc at
# 1 MiB on every VM, the cost of a call (the 52-byte path), every series of
# each VM, where the library overtakes the Smalltalk backend (the threshold
# SimdUTFLibrary minimumUnits), and the spread of the medians between runs.
# The value of a series is the median of its run values (each the best sample
# of a run); a speedup is the Zinc time over the backend's.
# Gates (exit status 1 when one fails):
#   - every CHECK is equal, and no ERROR line;
#   - the plugin at least 10x Zinc natively, 30x in WebAssembly, on every
#     conversion of 1 MiB (encode, decode, base64; ascii, latin and wide);
#   - the library at least 5x Zinc on every conversion of 1 MiB (natively);
#   - utf8Encode and utf8Decode of the 52-byte path through the plugin at
#     least 3x Zinc natively;
#   - the Smalltalk backend at least 0.5x Zinc on every series.
import math
import statistics
import sys

VMS = ('native-cog', 'native-stack', 'wasm-node', 'wasm-web-node')
NATIVE = ('native-cog', 'native-stack')
OPS = ('utf8Encode', 'utf8Decode', 'utf8SizeOf', 'utf8CharacterCountOf', 'isValidUTF8', 'isAscii',
       'utf16LEEncode', 'utf16LEDecode', 'base64Encode', 'base64Encode76', 'base64Decode')
CONVERSIONS = ('utf8Encode', 'utf8Decode', 'utf16LEEncode', 'utf16LEDecode',
               'base64Encode', 'base64Encode76', 'base64Decode')
DATA = ('path', 'ascii', 'latin', 'wide')
BACKENDS = ('zinc', 'plugin', 'ffi', 'smalltalk')
MIB = 1048576
GATE_PLUGIN_NATIVE, GATE_PLUGIN_WASM, GATE_FFI, GATE_CALL, GATE_SMALLTALK = 10.0, 30.0, 5.0, 3.0, 0.5


def fields(rest):
    """name=value fields as a dict"""
    return dict(f.split('=', 1) for f in rest if '=' in f)


def read(paths):
    infos, checks, results, errors, skips = [], [], {}, [], []
    for path in paths:
        run = None
        for line in open(path, encoding='utf-8', errors='replace'):
            f = line.rstrip('\n').split('\t')
            kind = f[0]
            if kind == 'INFO' and len(f) > 2:
                run = fields(f[2:]).get('run', '1')
                infos.append((f[1], run, f[2:], path))
            elif kind == 'CHECK' and len(f) >= 7:
                checks.append({'label': f[1], 'run': run, 'op': f[2], 'data': f[3], 'size': int(f[4]),
                               'backend': f[5], 'equal': f[6] == 'equal'})
            elif kind == 'RESULT' and len(f) >= 9:
                results.setdefault((f[1], f[2], f[3], int(f[4]), f[5]), {})[run] = {
                    'ns': float(f[6]), 'mbs': float(f[7]), 'gcs': int(f[8]), **fields(f[9:])}
            elif kind == 'ERROR':
                errors.append((path, '\t'.join(f[1:])))
            elif kind == 'SKIP':
                skips.append((path, '\t'.join(f[1:])))
    return infos, checks, results, errors, skips


def table(header, rows):
    print('| ' + ' | '.join(header) + ' |')
    print('|' + '---|' * len(header))
    for row in rows:
        print('| ' + ' | '.join(str(c) for c in row) + ' |')
    print()


def ns_text(x):
    if x is None:
        return '-'
    if x >= 1e6:
        return f'{x / 1e6:.2f} ms'
    if x >= 1e3:
        return f'{x / 1e3:.2f} us'
    return f'{x:.0f} ns'


def times(x):
    return '-' if x is None else f'{x:.1f}x' if x >= 1 else f'{x:.2f}x'


def main(paths):
    infos, checks, results, errors, skips = read(paths)
    labels = sorted({i[0] for i in infos}, key=lambda l: (VMS.index(l) if l in VMS else len(VMS), l))
    sizes = sorted({k[3] for k in results})

    def ns(label, op, data, size, backend):
        runs = results.get((label, op, data, size, backend))
        return statistics.median(r['ns'] for r in runs.values()) if runs else None

    def units(label, op, data, size):
        for backend in BACKENDS:
            runs = results.get((label, op, data, size, backend))
            if runs:
                return int(next(iter(runs.values())).get('units', size))
        return None

    def speedup(label, op, data, size, backend):
        z, b = ns(label, op, data, size, 'zinc'), ns(label, op, data, size, backend)
        return z / b if z is not None and b else None

    failed = []
    print('## Environment\n')
    table(['VM', 'runs', 'INFO'], [
        (label, ', '.join(sorted({i[1] for i in infos if i[0] == label})),
         ' '.join(next(i[2] for i in infos if i[0] == label)[1:]).replace('|', '/'))
        for label in labels])

    print('## Gates\n')

    def gate(name, problems):
        if problems:
            failed.append(name)
        shown = '; '.join(problems[:8]) + (f' ... ({len(problems)} in all)' if len(problems) > 8 else '')
        return (name, 'FAIL' if problems else 'pass', shown)

    def below(label, ops, data, size, backend, limit):
        bad = []
        for op in ops:
            for d in data:
                s = speedup(label, op, d, size, backend)
                if s is not None and s < limit:
                    bad.append(f'{label} {op} {d} {size}: {s:.1f}x')
        return bad

    plugin_bad, ffi_bad, call_bad, st_bad = [], [], [], []
    for label in labels:
        limit = GATE_PLUGIN_NATIVE if label in NATIVE else GATE_PLUGIN_WASM
        plugin_bad += below(label, CONVERSIONS, ('ascii', 'latin', 'wide'), MIB, 'plugin', limit)
        if label in NATIVE:
            ffi_bad += below(label, CONVERSIONS, ('ascii', 'latin', 'wide'), MIB, 'ffi', GATE_FFI)
            call_bad += below(label, ('utf8Encode', 'utf8Decode'), ('path',), 52, 'plugin', GATE_CALL)
        for size in sizes:
            st_bad += below(label, OPS, DATA, size, 'smalltalk', GATE_SMALLTALK)
    bad_checks = [f"{c['label']} {c['op']} {c['data']} {c['size']} {c['backend']}" for c in checks if not c['equal']]
    table(['gate', 'result', 'detail'], [
        gate(f'{len(checks)} CHECKs equal to Zinc', bad_checks),
        gate('no ERROR line', [e[1] for e in errors]),
        gate(f'plugin >= {GATE_PLUGIN_NATIVE:.0f}x Zinc natively, {GATE_PLUGIN_WASM:.0f}x in WebAssembly, '
             'every conversion of 1 MiB', plugin_bad),
        gate(f'ffi >= {GATE_FFI:.0f}x Zinc, every conversion of 1 MiB', ffi_bad),
        gate(f'plugin >= {GATE_CALL:.0f}x Zinc on the 52-byte path, utf8Encode and utf8Decode, natively', call_bad),
        gate(f'smalltalk >= {GATE_SMALLTALK}x Zinc on every series', st_bad),
    ])

    print('## Speedup over Zinc at 1 MiB\n')
    for backend in ('plugin', 'ffi', 'smalltalk'):
        cols = [l for l in labels if any(ns(l, op, d, MIB, backend) for op in OPS for d in DATA)]
        if not cols:
            continue
        print(f'### {backend}\n')
        table(['op', 'data'] + cols, [
            [op, d] + [times(speedup(l, op, d, MIB, backend)) for l in cols]
            for op in OPS for d in ('ascii', 'latin', 'wide')
            if any(ns(l, op, d, MIB, backend) for l in cols)])

    print('## MB/s at 1 MiB (over the bytes of UTF-8)\n')
    for label in labels:
        print(f'### {label}\n')
        rows = []
        for op in OPS:
            for d in ('ascii', 'latin', 'wide'):
                cells = [MIB * 1000.0 / v if (v := ns(label, op, d, MIB, b)) else None for b in BACKENDS]
                if any(c is not None for c in cells):
                    rows.append([op, d] + ['-' if c is None else f'{c:.0f}' for c in cells])
        table(['op', 'data'] + list(BACKENDS), rows)

    print('## One call on the 52-byte path\n')
    table(['op'] + [f'{l} {b}' for l in labels for b in BACKENDS if ns(l, 'utf8Encode', 'path', 52, b)], [
        [op] + [ns_text(ns(l, op, 'path', 52, b)) for l in labels for b in BACKENDS
                if ns(l, 'utf8Encode', 'path', 52, b)]
        for op in OPS if any(ns(l, op, 'path', 52, b) for l in labels for b in BACKENDS)])

    print('## Where the library overtakes the Smalltalk backend\n')
    print('The smallest source (in units: characters of a String, bytes of a ByteArray) from which '
          'ffi is as fast as smalltalk at every larger size measured; - when it never is.\n')
    for label in [l for l in labels if l in NATIVE]:
        series = {}
        for (l, op, d, size, backend) in results:
            if l == label and backend in ('ffi', 'smalltalk'):
                f, s = ns(l, op, d, size, 'ffi'), ns(l, op, d, size, 'smalltalk')
                if f is not None and s is not None:
                    series.setdefault((op, d), {})[units(l, op, d, size)] = (f, s)
        if not series:
            continue
        print(f'### {label}\n')
        rows = []
        for (op, d) in sorted(series, key=lambda k: (OPS.index(k[0]) if k[0] in OPS else 99, k[1])):
            points = sorted(series[(op, d)].items())
            cross = None
            for i, (u, _) in enumerate(points):
                if all(f <= s for _, (f, s) in points[i:]):
                    cross = u
                    break
            rows.append([op, d, '-' if cross is None else cross] +
                        [f'{u}: {s / f:.2f}' for u, (f, s) in points])
        table(['op', 'data', 'from', 'units: smalltalk/ffi time ...'], [r[:3] + [' '.join(r[3:])] for r in rows])
        thresholds = sorted({u for v in series.values() for u in v} | {64})
        rows = []
        for t in thresholds:
            regrets = []
            for points in series.values():
                for u, (f, s) in points.items():
                    chosen = f if u >= t else s
                    regrets.append(chosen / min(f, s))
            rows.append((t, f'{max(regrets):.2f}', f'{math.exp(statistics.mean(math.log(r) for r in regrets)):.3f}'))
        print('With `minimumUnits` = T the facade takes the library from T units on: the time it then '
              'takes over the faster of the two, at worst and on average (geometric mean) over the series.\n')
        table(['T', 'worst', 'mean'], rows)

    print('## Every series (median ns of one call; speedup over Zinc)\n')
    for label in labels:
        print(f'### {label}\n')
        rows = []
        for op in OPS:
            for d in DATA:
                for size in sizes:
                    cells = [ns(label, op, d, size, b) for b in BACKENDS]
                    if any(c is not None for c in cells):
                        rows.append([op, d, size] + [
                            ns_text(c) + ('' if b == 'zinc' or c is None else f' ({times(speedup(label, op, d, size, b))})')
                            for b, c in zip(BACKENDS, cells)])
        table(['op', 'data', 'bytes'] + list(BACKENDS), rows)

    print('## Spread of the medians between runs\n')
    rows = []
    for label in labels:
        spreads = []
        for (l, op, d, size, b), runs in results.items():
            values = [r['ns'] for r in runs.values()]
            if l == label and len(values) > 1 and min(values) > 0:
                spreads.append(((max(values) - min(values)) / min(values) * 100, f'{op} {d} {size} {b}'))
        if spreads:
            worst = max(spreads)
            rows.append((label, len(spreads), f'{statistics.median(s[0] for s in spreads):.1f}%',
                         f'{worst[0]:.1f}% ({worst[1]})'))
        else:
            rows.append((label, 0, '-', '-'))
    table(['VM', 'series', 'median spread', 'largest'], rows)

    if skips:
        print(f'{len(skips)} SKIP lines, the first: {skips[0][1]}\n')
    print(f"Gates: {'FAIL (' + ', '.join(failed) + ')' if failed else 'pass'}")
    return 1 if failed else 0


if __name__ == '__main__':
    if len(sys.argv) < 2:
        sys.stderr.write('usage: python3 -I summarize.py TSV...\n')
        sys.exit(2)
    sys.exit(main(sys.argv[1:]))
