"""bench.py の JSONL を Markdown の表にする。ラベルが2つあれば、1つ目を前、2つ目を後として比べる表も出す。

python3 -I report.py results.jsonl

比べる規則 (docs/implementation/benchmarks.md の「ネットワークの計測」): 主指標は eml のプロセスの CPU (user+sys) を床を引いて単位で割った値の中央値。後÷前の比が
0.90 以下か 1.10 以上で、かつ生の CPU の四分位の範囲 (25%-75%) が重ならないときだけ「差」とする。
"""
import json
import statistics as st
import sys

rows = [json.loads(l) for l in open(sys.argv[1])]
labels = []
for r in rows:
    if r["label"] not in labels and r["label"] != "python":
        labels.append(r["label"])
get = {(r["case"], r["label"], r["kind"]): r for r in rows}
cases = []
for r in rows:
    if r["case"] not in cases:
        cases.append(r["case"])

SYS = ["epoll_ctl", "epoll_pwait", "epoll_wait", "timerfd_settime", "read(eventfd)", "recvfrom", "recvfrom!EAGAIN",
       "sendto", "accept4", "accept4!EAGAIN", "connect", "brk", "mmap", "munmap"]
STATS = ["parks", "completions", "tasks", "allocations", "string_bytes_copied", "rc_increments",
         "rc_decrements", "preemptions"]


def cpu(r):
    return r["user"] + r["sys"]


def q(xs):
    xs = sorted(xs)
    if len(xs) < 4:
        return xs[0], xs[-1]
    a = st.quantiles(xs, n=4)
    return a[0], a[2]


def base_med(label, f):
    b = get.get(("base", label, "time"))
    return st.median(f(x) for x in b["data"]) if b else 0


def fmt(x):
    return f"{x:.2f}" if abs(x) >= 0.005 else "0"


for label in labels + ["python"]:
    print(f"\n## 時間: {label} (中央値。括弧は最小-最大。単位あたりは床を引く。µs)\n")
    print("| 件 | 単位 | 実時間 s | CPU s (四分位) | 単位あたり 実時間 | 単位あたり CPU | 単位あたり sys | 最大 RSS MiB | minflt/単位 | 自発/非自発 |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    for c in cases:
        r = get.get((c, label, "time"))
        if not r:
            continue
        d, n = r["data"], r["n_big"]
        floor = c != "base" and label != "python"
        bw = base_med(label, lambda x: x["wall"]) if floor else 0
        bc = base_med(label, cpu) if floor else 0
        bs = base_med(label, lambda x: x["sys"]) if floor else 0
        bf = base_med(label, lambda x: x["minflt"]) if floor else 0
        w = [x["wall"] for x in d]
        cs = [cpu(x) for x in d]
        lo, hi = q(cs)
        print(f"| {c} | {n} {r['unit']} | {st.median(w):.3f} ({min(w):.3f}-{max(w):.3f}) | {st.median(cs):.3f} ({lo:.3f}-{hi:.3f}) | "
              f"{(st.median(w) - bw) / n * 1e6:.2f} | {(st.median(cs) - bc) / n * 1e6:.2f} | "
              f"{(st.median(x['sys'] for x in d) - bs) / n * 1e6:.2f} | {st.median(x['maxrss_kib'] for x in d) / 1024:.1f} | "
              f"{(st.median(x['minflt'] for x in d) - bf) / n:.2f} | "
              f"{int(st.median(x['nvcsw'] for x in d))}/{int(st.median(x['nivcsw'] for x in d))} |")

for label in labels:
    b = get.get(("base", label, "strace"))
    if not b:
        continue
    print(f"\n## syscall: {label} (strace -f、main のスレッド、床を引いて単位あたり。複数回なら最小-最大)\n")
    print("| 件 | 単位 | " + " | ".join(SYS) + " | main の計 | ほかのスレッドの計 |")
    print("|---|---|" + "---|" * (len(SYS) + 2))
    bm = b["data"][0]["syscalls"].get("main", {})
    bo = sum(sum(v.values()) for k, v in b["data"][0]["syscalls"].items() if k != "main")
    for c in cases:
        r = get.get((c, label, "strace"))
        if not r or c == "base":
            continue
        n = r["n_small"]
        cells = []
        for k in SYS + ["#main", "#other"]:
            vals = []
            for x in r["data"]:
                m = x["syscalls"].get("main", {})
                if k == "#main":
                    v = sum(m.values()) - sum(bm.values())
                elif k == "#other":
                    v = sum(sum(t.values()) for name, t in x["syscalls"].items() if name != "main") - bo
                else:
                    v = m.get(k, 0) - bm.get(k, 0)
                vals.append(v / n)
            lo, hi = min(vals), max(vals)
            cells.append(fmt(lo) if abs(hi - lo) < 0.005 else f"{fmt(lo)}-{fmt(hi)}")
        print(f"| {c} | {n} {r['unit']} | " + " | ".join(cells) + " |")

for label in labels:
    b = get.get(("base", label, "stats"))
    if not b:
        continue
    bs = b["data"][0]["stats"]
    present = [k for k in STATS if k in bs]
    print(f"\n## RunStats: {label} (--stats、床を引いて単位あたり。床: " + ", ".join(f"{k}={bs[k]}" for k in present) + ")\n")
    print("| 件 | 単位 | " + " | ".join(present) + " |")
    print("|---|---|" + "---|" * len(present))
    for c in cases:
        r = get.get((c, label, "stats"))
        if not r or c == "base":
            continue
        s = r["data"][0]["stats"]
        print(f"| {c} | {r['n_small']} {r['unit']} | " + " | ".join(fmt((s[k] - bs[k]) / r["n_small"]) for k in present) + " |")

if len(labels) >= 2:
    a, b = labels[0], labels[1]
    print(f"\n## 比べ: {a} → {b} (CPU の中央値、床を引いて単位あたり µs)\n")
    print(f"| 件 | {a} | {b} | 比 | 四分位の重なり | 判定 | 実時間の比 |")
    print("|---|---|---|---|---|---|---|")
    for c in cases:
        ra, rb = get.get((c, a, "time")), get.get((c, b, "time"))
        if not ra or not rb or c == "base":
            continue
        n = ra["n_big"]
        pa = (st.median(cpu(x) for x in ra["data"]) - base_med(a, cpu)) / n * 1e6
        pb = (st.median(cpu(x) for x in rb["data"]) - base_med(b, cpu)) / n * 1e6
        qa, qb = q([cpu(x) for x in ra["data"]]), q([cpu(x) for x in rb["data"]])
        overlap = not (qa[1] < qb[0] or qb[1] < qa[0])
        ratio = pb / pa if pa else float("nan")
        verdict = "差" if (ratio <= 0.90 or ratio >= 1.10) and not overlap else "差なし"
        wa = st.median(x["wall"] for x in ra["data"]) - base_med(a, lambda x: x["wall"])
        wb = st.median(x["wall"] for x in rb["data"]) - base_med(b, lambda x: x["wall"])
        print(f"| {c} | {pa:.2f} | {pb:.2f} | {ratio:.2f} | {'重なる' if overlap else '重ならない'} | {verdict} | {wb / wa:.2f} |")
