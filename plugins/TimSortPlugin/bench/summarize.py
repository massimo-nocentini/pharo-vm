# summarize.py - the tables of docs/TimSort.md from the output of TSBench
#
# usage: python3 -I plugins/TimSortPlugin/bench/summarize.py TSV...
#
# Reads the LABEL.runN.tsv files of run-bench.sh (the INFO line of each gives
# its VM and run) and prints, in Markdown (for the case block, mode d, the
# series plugin and ffi are the resumable C sorts, TimSort sort:with:backend:,
# and smalltalk is timSort:, whose default is the port on every VM): the gates, the speedups over
# Pharo's sort of every case at the largest size of each VM on the random and
# the sorted data, every series of each VM, and the spread of the medians
# between runs.  The value of a series is the median of its run values (each
# the median sample of a run); a speedup is the time of Pharo's sort over the
# series' time.
# Gates (exit status 1 when one fails):
#   - every CHECK is equal (the result of each series is Pharo's sort's, the
#     same object at every index), and no ERROR line;
#   - natively, the plugin at least 1.5x Pharo's sort on the random data of
#     every case of modes a and b at 100000 elements and more, and 1.2x for
#     mode c (key), whose time includes the n evaluations of the key block
#     and the permutation applied in Smalltalk.
import statistics
import sys

VMS = ('native-cog', 'native-stack', 'wasm-node', 'wasm-web-node')
NATIVE = ('native-cog', 'native-stack')
CASES = ('int', 'float', 'string', 'f64', 'i32', 'u8', 'key', 'block')
SHAPES = ('random', 'sorted', 'reversed', 'fewUnique', 'nearlySorted')
SERIES = ('sort', 'asSortedCollection', 'plugin', 'ffi', 'smalltalk')
GATE_PLUGIN_NATIVE = 1.5
GATE_PLUGIN_NATIVE_KEY = 1.2


def fields(rest):
    """name=value fields as a dict"""
    return dict(f.split('=', 1) for f in rest if '=' in f)


def read(paths):
    infos, checks, results, errors, skips = [], [], {}, [], []
    for path in paths:
        run = None
        for line in open(path, encoding='utf-8', errors='replace'):
            f = line.rstrip('\r\n').split('\t')
            kind = f[0]
            if kind == 'INFO' and len(f) > 2:
                run = fields(f[2:]).get('run', '1')
                infos.append((f[1], run, f[2:], path))
            elif kind == 'CHECK' and len(f) >= 7:
                checks.append({'label': f[1], 'run': run, 'case': f[2], 'shape': f[3], 'n': int(f[4]),
                               'series': f[5], 'equal': f[6] == 'equal'})
            elif kind == 'RESULT' and len(f) >= 9:
                results.setdefault((f[1], f[2], f[3], int(f[4]), f[5]), {})[run] = {
                    'best': float(f[6]), 'median': float(f[7]), 'gcs': int(f[8]), **fields(f[9:])}
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


def ms_text(x):
    if x is None:
        return '-'
    if x >= 100:
        return f'{x:.0f}'
    if x >= 10:
        return f'{x:.1f}'
    if x >= 1:
        return f'{x:.2f}'
    return f'{x:.3f}'


def ratio_text(x):
    if x is None:
        return '-'
    if x >= 100:
        return f'{x:.0f}x'
    if x >= 10:
        return f'{x:.1f}x'
    return f'{x:.2f}x'


def n_text(n):
    for power in range(9, 0, -1):
        if n == 10 ** power:
            return f'1e{power}'
    return str(n)


def main(paths):
    infos, checks, results, errors, skips = read(paths)
    value = {key: statistics.median(r['median'] for r in runs.values()) for key, runs in results.items()}
    labels = [vm for vm in VMS if any(k[0] == vm for k in value)]
    labels += sorted({k[0] for k in value} - set(labels))
    failures = []

    def get(label, case, shape, n, series):
        return value.get((label, case, shape, n, series))

    def speedup(label, case, shape, n, series):
        base, t = get(label, case, shape, n, 'sort'), get(label, case, shape, n, series)
        return base / t if base and t else None

    # gates
    different = [c for c in checks if not c['equal']]
    for c in different:
        failures.append(f"{c['label']} run {c['run']}: {c['case']} {c['shape']} {c['n']} {c['series']} differs from Pharo's sort")
    for path, line in errors:
        failures.append(f'{path}: ERROR {line}')
    for label in labels:
        if label not in NATIVE:
            continue
        for case in ('int', 'float', 'string', 'f64', 'i32', 'u8', 'key'):
            for n in sorted({k[3] for k in value if k[0] == label and k[1] == case}):
                if n < 100000:
                    continue
                s = speedup(label, case, 'random', n, 'plugin')
                gate = GATE_PLUGIN_NATIVE_KEY if case == 'key' else GATE_PLUGIN_NATIVE
                if s is not None and s < gate:
                    failures.append(f'{label}: the plugin is {s:.2f}x Pharo\'s sort on {case} random {n} (gate {gate}x)')

    print('## Gates\n')
    print(f'{len(checks)} checks ({len(checks) - len(different)} equal to Pharo\'s sort), {len(errors)} errors, {len(skips)} skipped series.\n')
    if failures:
        for f in failures:
            print(f'- FAILED: {f}')
    else:
        print('- every gate passed')
    print()

    print('## Runs\n')
    for label, run, info, path in infos:
        print(f'- {label} run {run}: ' + ' '.join(info))
    print()

    # the largest size of each VM, random and sorted, every series
    print('## Speedups over Pharo\'s sort at the largest size\n')
    for label in labels:
        sizes = sorted({k[3] for k in value if k[0] == label})
        if not sizes:
            continue
        n = sizes[-1]
        print(f'### {label}, n = {n_text(n)}\n')
        series = [s for s in SERIES if s != 'sort' and any(k[0] == label and k[4] == s for k in value)]
        rows = []
        for case in CASES:
            for shape in ('random', 'sorted'):
                base = get(label, case, shape, n, 'sort')
                if base is None:
                    continue
                rows.append([case, shape, ms_text(base)] +
                            [f'{ms_text(get(label, case, shape, n, s))} ({ratio_text(speedup(label, case, shape, n, s))})'
                             for s in series])
        table(['case', 'shape', 'sort ms'] + [f'{s} ms (x)' for s in series], rows)
    print('For block (mode d), plugin and ffi are the resumable C sorts (`TimSort sort:with:backend:`), '
          'smalltalk is `timSort:`, the default (the port).\n')

    # every series
    print('## Every series\n')
    for label in labels:
        print(f'### {label}\n')
        series = [s for s in SERIES if s != 'sort' and any(k[0] == label and k[4] == s for k in value)]
        rows = []
        for case in CASES:
            for n in sorted({k[3] for k in value if k[0] == label and k[1] == case}):
                for shape in SHAPES:
                    base = get(label, case, shape, n, 'sort')
                    if base is None:
                        continue
                    rows.append([case, n_text(n), shape, ms_text(base)] +
                                [f'{ms_text(get(label, case, shape, n, s))} ({ratio_text(speedup(label, case, shape, n, s))})'
                                 for s in series])
        table(['case', 'n', 'shape', 'sort ms'] + [f'{s} ms (x)' for s in series], rows)

    # spread between runs
    spreads = []
    for key, runs in results.items():
        values = [r['median'] for r in runs.values()]
        if len(values) > 1 and min(values) > 0:
            spreads.append((max(values) / min(values) - 1, key))
    if spreads:
        spreads.sort(reverse=True)
        print('## Spread between runs\n')
        print(f'Median spread {statistics.median(s for s, _ in spreads) * 100:.1f}%, largest:\n')
        for s, key in spreads[:5]:
            print(f'- {" ".join(str(k) for k in key)}: {s * 100:.1f}%')
        print()
    return 1 if failures else 0


if __name__ == '__main__':
    if len(sys.argv) < 2:
        sys.stderr.write('usage: summarize.py TSV...\n')
        sys.exit(2)
    sys.exit(main(sys.argv[1:]))
