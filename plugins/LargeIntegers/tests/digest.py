"""digest.py: compact digests of the output of a differential run.

The drivers (plugin_harness calls, st/diff.st, st/diff-kernels.st) print one line per
primitive call, '<n> <op> <operands...> => <answer>', and '# <section>' lines between
groups of calls.  This prints one line per (section, op): the SHA-256 of those lines,
their count, and the key, in the order of first appearance:

    <sha256> <count> <section> | <op>

Lines that are neither calls nor sections are digested under the op '(other)', so
that they show up as a difference.

usage: python3 -I digest.py < output > digests
       python3 -I digest.py --compare expected.sha256 < output
         prints the digests that differ from (or are missing in) the expected file;
         exit status 1 when any does.
"""
import hashlib
import sys


def digest(lines):
    order, hashes, counts = [], {}, {}
    section = '(start)'
    for line in lines:
        line = line.rstrip('\n')
        if not line:
            continue
        if line.startswith('# '):
            section = line[2:]
            key, text = (section, '(section)'), line
        else:
            fields = line.split(' ')
            op = fields[1] if len(fields) > 2 and fields[0].isdigit() else '(other)'
            key, text = (section, op), line
        if key not in hashes:
            order.append(key)
            hashes[key] = hashlib.sha256()
            counts[key] = 0
        hashes[key].update(text.encode('utf-8', 'surrogateescape') + b'\n')
        counts[key] += 1
    return ['%s %d %s | %s' % (hashes[k].hexdigest(), counts[k], k[0], k[1]) for k in order]


def main(argv):
    sys.stdin.reconfigure(errors='surrogateescape')
    got = digest(sys.stdin)
    if len(argv) == 3 and argv[1] == '--compare':
        with open(argv[2]) as f:
            want = [l.rstrip('\n') for l in f if l.strip() and not l.startswith('#')]
        if got == want:
            print('digests: %d sections, all as expected' % len(got))
            return 0
        wanted, have = set(want), set(got)
        for l in want:
            if l not in have:
                print('expected: ' + l)
        for l in got:
            if l not in wanted:
                print('got:      ' + l)
        if wanted == have:
            print('digests: same lines in another order')
        return 1
    if len(argv) != 1:
        sys.stderr.write(__doc__)
        return 2
    print('\n'.join(got))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
