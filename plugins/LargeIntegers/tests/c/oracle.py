"""Independent oracle: Python big integers check the vectors `kernels_test dump N` prints.
M a b resLen res : res == (a * b) mod 2^(32 resLen)       (liDigitMultiply)
D a b q r        : q == a // b and r == a % b              (liDigitDiv, remainder unshifted)
usage: ./kernels_fast dump 2000 | python3 -I oracle.py"""
import sys
n = bad = 0
for line in sys.stdin:
    f = line.split()
    if f[0] == 'M':
        a, b, rl, r = int(f[1], 16), int(f[2], 16), int(f[3]), int(f[4], 16)
        ok = r == (a * b) % (1 << (32 * rl))
    elif f[0] == 'D':
        a, b, q, r = (int(x, 16) for x in f[1:5])
        ok = (q, r) == divmod(a, b)
    else:
        continue
    n += 1
    if not ok:
        bad += 1
        if bad < 5: print('MISMATCH', line[:200])
print('oracle: %d vectors, %d mismatches' % (n, bad))
sys.exit(1 if bad or n == 0 else 0)
