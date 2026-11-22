#!/usr/bin/env python3
"""usage: norm.py OUTDIR   prints the files whose VM and native results differ after peak_stack is removed, with the fields that differ"""
import re, sys, os
d = sys.argv[1]
same = diff = 0
for line in open(os.path.join(d, 'summary.txt')):
    kind, name = line.split(' ', 1)
    name = name.strip()
    if kind != 'DIFF':
        continue
    k = os.path.join(d, name.replace('/', '_'))
    try:
        n, v = open(k + '.n.err', errors='replace').read(), open(k + '.v.err', errors='replace').read()
        no, vo = open(k + '.n.out', 'rb').read(), open(k + '.v.out', 'rb').read()
    except FileNotFoundError:
        continue
    strip = lambda s: re.sub(r'peak_stack=\d+ ', '', s)
    if strip(n) == strip(v) and no == vo:
        same += 1
        continue
    diff += 1
    fn = dict(re.findall(r'(\w+)=(\d+)', n)); fv = dict(re.findall(r'(\w+)=(\d+)', v))
    fields = [f'{f}: native {fn.get(f)} vm {fv.get(f)}' for f in fv if f != 'peak_stack' and fn.get(f) != fv.get(f)]
    other = '' if fields or no != vo else ' (stderr text differs)'
    print(name, '; '.join(fields), 'STDOUT DIFFERS' if no != vo else '', other)
print('same', same, 'diff', diff)
