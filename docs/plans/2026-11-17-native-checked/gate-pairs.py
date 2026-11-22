# usage: gate-pairs.py BASE-1.md HEAD-1.md BASE-2.md HEAD-2.md ...   (outputs of bench/run.sh, alternating)
# Prints, per row present in both tables, the median of the deltas (%) of the run part, and marks rows over +0.5 %.
import re, statistics, sys
def parse(p):
    return {m.group(1): int(m.group(3)) for line in open(p)
            if (m := re.match(r"\| `([^`]+)` \| (\d+) \| ?(\d*) ?\|", line)) and m.group(3)}
f = sys.argv[1:]
pairs = [(parse(f[i]), parse(f[i + 1])) for i in range(0, len(f), 2)]
for n in sorted(set(pairs[0][0]) & set(pairs[0][1])):
    d = [(h[n] / b[n] - 1) * 100 for b, h in pairs]
    m = statistics.median(d)
    print(f"{n:24s} {m:+7.2f} %  [{' '.join(f'{x:+.2f}' for x in d)}]{' OVER' if m > 0.5 else ''}")
