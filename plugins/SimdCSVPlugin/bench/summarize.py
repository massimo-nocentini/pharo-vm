# summarize.py - the tables of docs/SimdCSV.md from the output of SCBench and
# scbench
#
# usage: python3 -I plugins/SimdCSVPlugin/bench/summarize.py TSV...
#
# Reads the LABEL.runN.tsv files of run-bench.sh and prints, in Markdown: the
# gates, the engine in GB/s, the speedups over NeoCSV and the MB/s of each VM,
# the parts of the load of 3 columns of Floats, the large file, and the spread
# between runs.
# The value of a series is the median over the runs of its median of a run (5
# samples); a speedup is the time of NeoCSV over that of the backend.
# Gates (exit status 1 when one is missed; those of a VM that did not run are
# not there).  Everywhere:
#   - every CHECK is equal, and no ERROR line;
# native-cog, at 1 and at 10 MB:
#   - rows from bytes >= 8x NeoCSV from a String (mixed, ints, quoted, utf8);
#   - typed rows of mixed >= 12x; do: on a file >= 10x (the same four data);
#   - gtfs with cached converters >= 6x, from a FileReference and from a
#     stream (without a cache: reported);
#   - count: >= 1 GB/s (regular data);
#   - rows of inches >= 3x;
# native-cog: no full garbage collection in a do: over 100 MB, and less than
# 5 MB retained; the engine: index >= 0.6x upstream's find_indexes (the best
# kernel, regular data); wasm-node, at 1 MB: rows >= 8x (the same four data).
import statistics
import sys

VMS = ('engine', 'native-cog', 'native-stack', 'wasm-node')
OPS = ('count', 'index', 'indexLines', 'rows', 'rowsString', 'rowsDo', 'columns3', 'typed', 'floats3',
       'floats3build', 'floats3rows', 'float64', 'table', 'fileDo', 'writer', 'writerOptional',
       'gtfsFile', 'gtfsFileCached', 'gtfsStream', 'gtfsStreamCached')
DATA = ('mixed', 'ints', 'quoted', 'utf8', 'floats17', 'inches', 'gtfs')
REGULAR = tuple(d for d in DATA if d != 'inches')
FOUR = ('mixed', 'ints', 'quoted', 'utf8')
BACKENDS = ('neo', 'lines', 'plugin', 'smalltalk')
KERNELS = ('upstream', 'avx2', 'neon', 'sse2', 'fallback')
CALLS = ('index', 'index+lines', 'batches', 'measure')
# The series of NeoCSV that stands for another one
SAME_IN_NEO = {'rowsString': 'rows', 'gtfsFileCached': 'gtfsFile', 'gtfsStream': 'gtfsFile', 'gtfsStreamCached': 'gtfsFile'}


def fields(rest):
    """name=value fields as a dict"""
    return dict(f.split('=', 1) for f in rest if '=' in f)


def table(header, rows):
    print('| ' + ' | '.join(header) + ' |')
    print('|' + '---|' * len(header))
    for row in rows:
        print('| ' + ' | '.join(str(c) for c in row) + ' |')
    print()


def times(x):
    return '-' if x is None else f'{x:.1f}x' if x >= 1 else f'{x:.2f}x'


def ms_text(ns):
    if ns is None:
        return '-'
    return f'{ns / 1e6:.0f} ms' if ns >= 1e8 else f'{ns / 1e6:.1f} ms' if ns >= 1e7 else f'{ns / 1e6:.2f} ms'


def main(paths):
    infos, checks, results, engine, large, errors = [], [], {}, {}, [], []
    for path in paths:
        run = None
        for line in open(path, encoding='utf-8', errors='replace'):
            f = line.rstrip('\n').split('\t')
            kind = f[0]
            if kind == 'INFO' and len(f) > 2:
                run = fields(f[2:]).get('run', '1')
                infos.append((f[1], run, f[2:]))
            elif kind == 'CHECK' and len(f) >= 7:
                checks.append(f[1:7])
            elif kind == 'RESULT' and len(f) >= 9:
                # (the size asked for, in MB: the text is less than a record longer)
                results.setdefault((f[1], f[2], f[3], round(int(f[4]) / 1e6), f[5]), {})[run] = {
                    'ns': float(f[6]), 'bytes': int(f[4]), 'gcs': int(f[8]), **fields(f[9:])}
            elif kind == 'C' and len(f) >= 6:
                engine.setdefault((f[1], f[2], f[3]), []).append((float(f[5]), f))
            elif kind == 'LARGE' and len(f) >= 8:
                large.append({'label': f[1], 'run': run, 'bytes': int(f[2]), 'backend': f[3], 'ms': float(f[4]),
                              'mbs': float(f[5]), 'gcs': int(f[6]), 'scavenges': int(f[7]), **fields(f[8:])})
            elif kind == 'ERROR':
                errors.append(' '.join(f[1:]))
    labels = sorted({i[0] for i in infos}, key=lambda l: (VMS.index(l) if l in VMS else len(VMS), l))
    vms = [l for l in labels if l != 'engine']
    sizes = sorted({k[3] for k in results})

    def runs_of(label, op, data, size, backend):
        runs = results.get((label, op, data, size, backend))
        if not runs and backend == 'neo' and op in SAME_IN_NEO:
            runs = results.get((label, SAME_IN_NEO[op], data, size, backend))
        return runs or {}

    def ns(*key):
        runs = runs_of(*key)
        return statistics.median(r['ns'] for r in runs.values()) if runs else None

    def mbs(*key):
        runs = runs_of(*key)
        return next(iter(runs.values()))['bytes'] * 1000.0 / ns(*key) if runs else None

    def speedup(label, op, data, size, backend):
        neo, own = ns(label, op, data, size, 'neo'), ns(label, op, data, size, backend)
        return neo / own if neo and own else None

    def gbs(data, kernel, call):
        lines = engine.get((data, kernel, call))
        return statistics.median(l[0] for l in lines) if lines else None

    def of_upstream(data):
        ours, theirs = gbs(data, best, 'index'), gbs(data, 'upstream', 'find_indexes')
        return ours / theirs if ours and theirs else None

    best = next((k for k in KERNELS[1:] if any(e[1] == k for e in engine)), None)
    failed, rows = [], []

    def gate(name, measured, limit, show=times):
        """measured: (what, value or None)"""
        values = [(w, v) for w, v in measured if v is not None]
        if values:
            missed = any(v < limit for w, v in values)
            if missed:
                failed.append(name)
            rows.append((name, 'missed' if missed else 'met', ', '.join(f'{w} {show(v)}' for w, v in values)))

    print('## Environment\n')
    table(['what', 'runs', 'INFO'], [
        (label, ', '.join(sorted({i[1] for i in infos if i[0] == label})),
         ' '.join(next(i[2] for i in infos if i[0] == label)[1:]).replace('|', '/'))
        for label in labels])

    print('## Gates\n')
    bad = [' '.join(c[:5]) for c in checks if c[5] != 'equal']
    rows.append((f'{len(checks)} CHECKs equal to NeoCSV', 'missed' if bad else 'met', '; '.join(bad[:5])))
    rows.append(('no ERROR line', 'missed' if errors else 'met', '; '.join(errors[:3])))
    if bad or errors:
        failed.append('checks')
    cog = 'native-cog'
    for size in (10, 1):
        mb = f'{size} MB: '
        gate(mb + 'rows >= 8x', [(d, speedup(cog, 'rows', d, size, 'plugin')) for d in FOUR], 8)
        gate(mb + 'typed rows >= 12x', [('mixed', speedup(cog, 'typed', 'mixed', size, 'plugin'))], 12)
        gate(mb + 'do: on a file >= 10x', [(d, speedup(cog, 'fileDo', d, size, 'plugin')) for d in FOUR], 10)
        gate(mb + 'gtfs cached >= 6x', [(o, speedup(cog, o, 'gtfs', size, 'plugin'))
                                        for o in ('gtfsFileCached', 'gtfsStreamCached')], 6)
        gate(mb + 'count >= 1 GB/s', [(d, mbs(cog, 'count', d, size, 'plugin')) for d in REGULAR], 1000,
             lambda v: f'{v / 1000:.2f}')
        gate(mb + 'rows of inches >= 3x', [('inches', speedup(cog, 'rows', 'inches', size, 'plugin'))], 3)
    gate('engine: index >= 0.6x upstream', [(d, of_upstream(d)) for d in REGULAR], 0.6)
    gate('wasm-node, 1 MB: rows >= 8x', [(d, speedup('wasm-node', 'rows', d, 1, 'plugin')) for d in FOUR], 8)
    for line in large:
        if line['label'] == cog and line['backend'] == 'plugin':
            ok = line['gcs'] == 0 and int(line['retained']) < 5000000
            name = f"do: over {line['bytes'] / 1e6:.0f} MB, run {line['run']}: no full GC, < 5 MB retained"
            if not ok:
                failed.append(name)
            rows.append((name, 'met' if ok else 'missed', f"{line['gcs']} full GCs, {line['retained']} bytes"))
    table(['gate', 'result', 'measured'], rows)

    if engine:
        print('## The engine, GB/s (the median of 9, then over the runs)\n')
        for call in CALLS:
            kernels = [k for k in KERNELS if any(gbs(d, k, call) for d in DATA)]
            print(f'### {call}\n')
            lines = []
            for d in DATA:
                if not gbs(d, best, call):
                    continue
                line = [d] + [f'{gbs(d, k, call):.2f}' for k in kernels]
                if call == 'index' and gbs(d, 'upstream', 'find_indexes'):
                    same = engine[(d, 'upstream', 'find_indexes')][0][1][-1] == 'same'
                    line += [f"{gbs(d, 'upstream', 'find_indexes'):.2f}" + ('' if same else ' (another index)'), f'{of_upstream(d):.2f}']
                lines.append(line)
            table(['data'] + kernels + (['upstream', f'{best} / upstream'] if len(lines[0]) > 1 + len(kernels) else []), lines)

    for size in sizes:
        print(f'## Speedup over NeoCSV at {size} MB\n')
        cols = [(l, b) for l in vms for b in ('plugin', 'smalltalk') if any(k[0] == l and k[3] == size and k[4] == b for k in results)]
        table(['op', 'data'] + [f'{l} {b}' for l, b in cols], [
            [op, d] + [times(speedup(l, op, d, size, b)) for l, b in cols]
            for op in OPS for d in DATA if any(speedup(l, op, d, size, b) for l, b in cols)])
        for label in vms:
            print(f'## MB/s at {size} MB, {label}\n')
            table(['op', 'data'] + list(BACKENDS) + ['plugin / neo', 'time of plugin', 'full GCs of the samples'], [
                [op, d] + ['-' if mbs(label, op, d, size, b) is None else f'{mbs(label, op, d, size, b):.1f}' for b in BACKENDS]
                + [times(speedup(label, op, d, size, 'plugin')), ms_text(ns(label, op, d, size, 'plugin')),
                   '/'.join(str(r['gcs']) for r in runs_of(label, op, d, size, 'plugin').values()) or '-']
                for op in OPS for d in DATA if any(ns(label, op, d, size, b) for b in ('lines', 'plugin', 'smalltalk'))])

    print('## 3 columns of Floats of mixed (columns:as: of a table made of bytes), plugin\n')
    parts = []
    for label in vms:
        for size in sizes:
            whole, build, builder = (ns(label, o, 'mixed', size, 'plugin') for o in ('floats3', 'floats3build', 'floats3rows'))
            if whole and build and builder:
                rest = whole - build - builder
                parts.append((label, size, ms_text(whole), ms_text(build), ms_text(builder), ms_text(rest), f'{rest / whole * 100:.0f}%'))
    table(['VM', 'MB', 'whole', 'table', 'rows of the builder', 'the rest: transposition', 'share'], parts)

    if large:
        print('## The large file\n')
        table(['VM', 'run', 'MB', 'backend', 'ms', 'MB/s', 'full GCs', 'scavenges', 'old space grew by', 'retained'], [
            (x['label'], x['run'], f"{x['bytes'] / 1e6:.0f}", x['backend'], f"{x['ms']:.0f}", x['mbs'], x['gcs'], x['scavenges'],
             x['grown'], x['retained']) for x in large])

    print('## Spread of a series between runs, (largest - least) / least\n')
    lines = []
    for label in vms:
        spreads = []
        for key, runs in results.items():
            values = [r['ns'] for r in runs.values()]
            if key[0] == label and len(values) > 1:
                spreads.append(((max(values) - min(values)) / min(values) * 100, ' '.join(str(k) for k in key[1:])))
        if spreads:
            worst = max(spreads)
            lines.append((label, len(spreads), f'{statistics.median(s[0] for s in spreads):.1f}%', f'{worst[0]:.1f}% ({worst[1]})'))
    table(['VM', 'series', 'median', 'largest'], lines)
    print(f"Gates: {'missed: ' + ', '.join(failed) if failed else 'met'}")
    return 1 if failed else 0


if __name__ == '__main__':
    if len(sys.argv) < 2:
        sys.stderr.write('usage: python3 -I summarize.py TSV...\n')
        sys.exit(2)
    sys.exit(main(sys.argv[1:]))
