#!/usr/bin/env python3
# git-hunks.py - pick some hunks of the working-tree diff of one file, to commit a
# file in several steps without an interactive git add -p:
#   git-hunks.py list <file>              the hunks, numbered from 0, with their first lines
#   git-hunks.py pick <file> <i,j,...>    a patch with only those hunks, for
#                                         git apply --cached
# e.g.  git-hunks.py pick src/x.c 0,2 | git apply --cached && git commit
import subprocess, sys, re
mode, path = sys.argv[1], sys.argv[2]
d = subprocess.run(['git', 'diff', '-U3', '--', path], capture_output=True, text=True, check=True).stdout
lines = d.splitlines(keepends=True)
head, hunks, cur = [], [], None
for l in lines:
    if l.startswith('@@'):
        cur = [l]; hunks.append(cur)
    elif cur is None: head.append(l)
    else: cur.append(l)
if mode == 'list':
    for i, h in enumerate(hunks):
        ctx = h[0].strip()
        plus = [x[1:].strip() for x in h[1:] if x.startswith(('+', '-'))][:3]
        print(i, ctx[:110], '|', ' // '.join(plus)[:160])
else:
    pick = {int(x) for x in sys.argv[3].split(',')}
    sys.stdout.write(''.join(head))
    for i, h in enumerate(hunks):
        if i in pick: sys.stdout.write(''.join(h))
