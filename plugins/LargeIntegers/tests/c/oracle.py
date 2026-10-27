"""Independent oracle: Python big integers check the vectors `kernels_test dump N` prints.
M a b resLen res : res == (a * b) mod 2^(32 resLen)       (liDigitMultiply)
D a b q r        : q == a // b and r == a % b              (liDigitDiv, remainder unshifted)
G a b m n res    : with R = 2^(32 n), U = -a b / m mod R and T = (a b + U m) / R,
                   res == T - m if T >= m else T, mod R       (liDigitMontgomery, m odd)
usage: ./kernels_fast dump 2000 | python3 -I oracle.py"""
import sys
n = bad = 0
kinds = {}
for line in sys.stdin:
    f = line.split()
    kinds[f[0]] = kinds.get(f[0], 0) + 1
    if f[0] == 'M':
        a, b, rl, r = int(f[1], 16), int(f[2], 16), int(f[3]), int(f[4], 16)
        ok = r == (a * b) % (1 << (32 * rl))
    elif f[0] == 'D':
        a, b, q, r = (int(x, 16) for x in f[1:5])
        ok = (q, r) == divmod(a, b)
    elif f[0] == 'G':
        a, b, m, digits, r = int(f[1], 16), int(f[2], 16), int(f[3], 16), int(f[4]), int(f[5], 16)
        R = 1 << (32 * digits)
        u = (-a * b * pow(m, -1, R)) % R
        t, low = divmod(a * b + u * m, R)
        ok = low == 0 and r == ((t - m) % R if t >= m else t)
    else:
        continue
    n += 1
    if not ok:
        bad += 1
        if bad < 5: print('MISMATCH', line[:200])
print('oracle: %d vectors (%s), %d mismatches' % (n, ', '.join('%s %d' % kv for kv in sorted(kinds.items())), bad))
sys.exit(1 if bad or n == 0 else 0)
