# summarize.py - the tables of docs/SimdJSON.md from the output of SJBench
#
# usage: python3 -I plugins/SimdJSONPlugin/bench/summarize.py TSV...
#
# Reads the LABEL.runN.tsv files of run-bench.sh (the INFO line of each gives
# its VM and run) and prints, in Markdown: the gates, the targets, the
# speedups over NeoJSON on every file and VM, the series of each VM, the
# plugin's parse and build times, and the spread of the medians between runs.
# The value of a series is the median of its run medians.
# Gates (exit status 1 when one fails):
#   - every CHECK is equal, with 0 policy differences;
#   - no ERROR line;
#   - sj-string is at least 8x faster than neo-string on every corpus file and VM.
# Targets are reported, not checked.
import statistics
import sys

GATE_SPEEDUP = 8.0
GENERATED = ('ndjson-20k.ndjson', 'small-200.json', 'small-2k.json')
VMS = ('native-cog', 'native-stack', 'wasm-node', 'wasm-web-node', 'browser-chromium')
SERIES = ('neo-string', 'sj-string', 'sj-bytes', 'sj-tree', 'sj-float', 'sj-prim',
          'neo-symbols', 'sj-symbols', 'neo-objects', 'sj-objects', 'utf8Encoded')
MAPPING = ('neo-map', 'sj-map', 'neo-map-symbols', 'sj-map-symbols')
NDJSON = ('neo-ndjson-next', 'sj-ndjson-next', 'neo-ndjson-upToEnd', 'sj-ndjson-upToEnd')


def fields(rest):
    """name=value fields as a dict"""
    return dict(f.split('=', 1) for f in rest if '=' in f)


def read(paths):
    infos, checks, results, csplits, stats, errors, skips = [], [], {}, {}, [], [], []
    for path in paths:
        label, run = None, None
        for line in open(path, encoding='utf-8', errors='replace'):
            f = line.rstrip('\n').split('\t')
            kind = f[0]
            if kind == 'INFO' and len(f) > 2:
                label, run = f[1], fields(f[2:]).get('run', '1')
                infos.append((label, run, f[2:], path))
            elif kind == 'CHECK' and len(f) >= 9:
                checks.append({'label': f[1], 'run': run, 'file': f[2], 'kind': f[3],
                               'equal': f[4] == 'equal', **fields(f[5:])})
            elif kind == 'RESULT' and len(f) >= 11:
                results.setdefault((f[1], f[2], f[4]), {})[run] = {
                    'bytes': int(f[3]), 'n': f[5], 'median': float(f[6]), 'min': float(f[7]),
                    'mbs': float(f[8]), **fields(f[9:])}
            elif kind == 'CSPLIT' and len(f) >= 6:
                csplits.setdefault((f[1], f[2], f[3]), {})[run] = fields(f[4:])
            elif kind == 'STATS' and len(f) >= 5:
                stats.append({'label': f[1], 'run': run, 'file': f[2], 'series': f[3], **fields(f[4:])})
            elif kind == 'ERROR':
                errors.append((path, '\t'.join(f[1:])))
            elif kind == 'SKIP':
                skips.append((path, '\t'.join(f[1:])))
    return infos, checks, results, csplits, stats, errors, skips


def median(results, label, file, series):
    runs = results.get((label, file, series))
    return statistics.median(r['median'] for r in runs.values()) if runs else None


def ratio(a, b):
    return a / b if a is not None and b else None


def ms(x):
    if x is None:
        return '-'
    return f'{x:.1f}' if x >= 100 else f'{x:.2f}' if x >= 1 else f'{x:.3f}'


def times(x):
    return '-' if x is None else f'{x:.1f}x'


def table(header, rows):
    print('| ' + ' | '.join(header) + ' |')
    print('|' + '---|' * len(header))
    for row in rows:
        print('| ' + ' | '.join(str(c) for c in row) + ' |')
    print()


def main(paths):
    infos, checks, results, csplits, stats, errors, skips = read(paths)
    labels = sorted({i[0] for i in infos}, key=lambda l: (VMS.index(l) if l in VMS else len(VMS), l))
    files = []
    for (_, file, _) in results:
        if file not in files:
            files.append(file)
    corpus = [f for f in files if f not in GENERATED]
    failed = []

    print('## Environment\n')
    table(['VM', 'runs', 'INFO'], [
        (label, ', '.join(sorted({i[1] for i in infos if i[0] == label})),
         ' '.join(next(i[2] for i in infos if i[0] == label)[1:]).replace('|', '/'))
        for label in labels])

    print('## Gates\n')
    bad = [c for c in checks if not c['equal']]
    policy = [c for c in checks if c.get('policy', '0') != '0']
    slow = []
    for label in labels:
        for file in corpus:
            r = ratio(median(results, label, file, 'neo-string'), median(results, label, file, 'sj-string'))
            if r is not None and r < GATE_SPEEDUP:
                slow.append(f'{label} {file} {r:.1f}x')
    missing = [label for label in labels
               if not any(median(results, label, f, 'sj-string') for f in corpus)]

    def gate(name, problems, detail):
        if problems:
            failed.append(name)
        return (name, 'FAIL' if problems else 'pass', detail(problems) if problems else '')

    first = lambda items, show: '; '.join(show(x) for x in items[:8]) + (' ...' if len(items) > 8 else '')
    table(['gate', 'result', 'detail'], [
        gate(f'{len(checks)} CHECKs equal', bad,
             lambda p: first(p, lambda c: f"{c['label']} {c['file']} {c['kind']} at {c.get('firstDiff')}")),
        gate('0 policy differences', policy,
             lambda p: first(p, lambda c: f"{c['label']} {c['file']} {c['kind']} policy={c['policy']}")),
        gate('no ERROR line', errors, lambda p: first(p, lambda e: e[1])),
        gate(f'sj-string >= {GATE_SPEEDUP:.0f}x neo-string', slow + [f'{l}: no sj-string' for l in missing],
             lambda p: first(p, str)),
    ])

    print('## Targets\n')

    def val(label, file, series):
        return median(results, label, file, series)

    def target(name, value, ok, show):
        return (name, show(value), '-' if value is None else ('yes' if ok(value) else 'no'))

    def stack_cog(file):
        return ratio(val('native-stack', file, 'sj-string'), val('native-cog', file, 'sj-string'))

    rows = []
    for file, limit in (('twitter.json', 3.0), ('citm_catalog.json', 5.0), ('canada.json', 7.5)):
        rows.append(target(f'Cog sj-string <= {limit} ms on {file}', val('native-cog', file, 'sj-string'),
                           lambda v, limit=limit: v <= limit, lambda v: ms(v) + ' ms' if v is not None else '-'))
    for file in ('twitter.json', 'citm_catalog.json', 'canada.json'):
        rows.append(target(f'Stack sj-string within 15% of Cog on {file}', stack_cog(file),
                           lambda v: v <= 1.15, lambda v: '-' if v is None else f'{v:.2f}x Cog'))
    tw = ratio(val('wasm-node', 'twitter.json', 'neo-string'), val('wasm-node', 'twitter.json', 'sj-string'))
    rows.append(target('wasm node sj-string >= 40x neo-string on twitter.json', tw, lambda v: v >= 40, times))
    rows.append(target('wasm node sj-string >= 60x neo-string on twitter.json (stretch)', tw, lambda v: v >= 60, times))
    rows.append(target('Cog sj-map >= 5x neo-map on twitter.json',
                       ratio(val('native-cog', 'twitter.json', 'neo-map'), val('native-cog', 'twitter.json', 'sj-map')),
                       lambda v: v >= 5, times))
    rows.append(target('Cog sj-map-symbols >= 10x neo-map on twitter.json',
                       ratio(val('native-cog', 'twitter.json', 'neo-map'),
                             val('native-cog', 'twitter.json', 'sj-map-symbols')),
                       lambda v: v >= 10, times))
    for label in labels:
        f = ratio(val(label, 'marine_ik.json', 'sj-string'), val(label, 'marine_ik.json', 'sj-float'))
        rows.append(target(f'{label} sj-string within 10% of sj-float on marine_ik.json', f,
                           lambda v: v <= 1.10, lambda v: '-' if v is None else f'{v:.2f}x sj-float'))
    table(['target', 'measured', 'met'], rows)

    print('## neo-string / sj-string, median ms (speedup)\n')
    table(['file', 'bytes'] + labels, [
        [file, next(iter(results.get((labels[0], file, 'neo-string'), {}).values()), {}).get('bytes', '-')
         if labels else '-'] +
        [f"{ms(val(l, file, 'neo-string'))} / {ms(val(l, file, 'sj-string'))} "
         f"({times(ratio(val(l, file, 'neo-string'), val(l, file, 'sj-string')))})" for l in labels]
        for file in corpus])

    for label in labels:
        print(f'## {label}: median ms\n')
        table(['file'] + list(SERIES) + ['sj-string MB/s'], [
            [file] + [ms(val(label, file, s)) for s in SERIES] +
            ['-' if not results.get((label, file, 'sj-string')) else
             f"{statistics.median(r['mbs'] for r in results[(label, file, 'sj-string')].values()):.1f}"]
            for file in corpus])
        mapped = [f for f in corpus if val(label, f, 'neo-map') is not None]
        if mapped:
            table(['file'] + list(MAPPING) + ['neo-map/sj-map', 'neo-map/sj-map-symbols'], [
                [file] + [ms(val(label, file, s)) for s in MAPPING] +
                [times(ratio(val(label, file, 'neo-map'), val(label, file, 'sj-map'))),
                 times(ratio(val(label, file, 'neo-map'), val(label, file, 'sj-map-symbols')))]
                for file in mapped])
        others = [(f, s) for f in files for s in NDJSON + ('neo-small', 'sj-small')
                  if val(label, f, s) is not None]
        if others:
            table(['file', 'series', 'median ms', 'MB/s'], [
                [f, s, ms(val(label, f, s)),
                 f"{statistics.median(r['mbs'] for r in results[(label, f, s)].values()):.1f}"]
                for f, s in others])

    print('## Plugin time per sj-string run (parse / build ms)\n')

    def split(label, file):
        runs = csplits.get((label, file, 'sj-string'))
        if not runs:
            return '-'
        parse = statistics.median(float(r.get('parse_ms', 'nan')) for r in runs.values())
        build = statistics.median(float(r.get('build_ms', 'nan')) for r in runs.values())
        return f'{parse:.2f} / {build:.2f}'
    table(['file'] + labels, [[file] + [split(l, file) for l in labels] for file in corpus])

    print('## Spread of the medians between runs\n')
    rows = []
    for label in labels:
        spreads = []
        for (l, file, series), runs in results.items():
            values = [r['median'] for r in runs.values()]
            if l == label and len(values) > 1 and min(values) > 0:
                spreads.append(((max(values) - min(values)) / min(values) * 100, file, series))
        if spreads:
            worst = max(spreads)
            rows.append((label, len(spreads), f'{statistics.median(s[0] for s in spreads):.1f}%',
                         f'{worst[0]:.1f}% ({worst[1]} {worst[2]})'))
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
