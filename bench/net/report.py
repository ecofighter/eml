"""bench.py の JSONL を Markdown の表にする。ラベルが2つあれば、1つ目を前、2つ目を後として比べる表も出す。

python3 -I report.py results.jsonl

strace の run があるラベルには、syscall の目標の表も出す (docs/implementation/benchmarks.md の「ネットワークの計測」)。

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

SYS = ["epoll_ctl", "epoll_pwait", "epoll_wait", "timerfd_settime", "read(eventfd)", "recvfrom", "recvfrom!EAGAIN", "recvfrom(peek)", "recvfrom(peek)!EAGAIN", "sendto!EAGAIN",
       "sendto", "accept4", "accept4!EAGAIN", "connect", "epoll_pwait2", "getsockopt", "getpeername", "brk", "mmap", "munmap", "io_uring_enter"]
# 目標の表の式。床を引いた回数の辞書 v と、その件の単位の数 n から値を出す。
# mem = brk + mmap + munmap、E = 空振りの読み、R = データを読んだ読み、P = 当たった覗き。
# 覗きは別に数えるので、(R + P) ÷ R が空きなしの読みの「覗きと読みの2回まで」を表す。


def derive(v):
    v["mem"] = v["brk"] + v["mmap"] + v["munmap"]
    v["E"] = v["recvfrom!EAGAIN"] + v["recvfrom(peek)!EAGAIN"]
    v["R"] = v["recvfrom"] - v["recvfrom!EAGAIN"]
    v["P"] = v["recvfrom(peek)"] - v["recvfrom(peek)!EAGAIN"]
    v["waits"] = v["epoll_wait"] + v["epoll_pwait"] + v["epoll_pwait2"]
    v["per_wait"] = v["timerfd_settime"] + v["read(eventfd)"] + v["epoll_ctl"]
    return v


def div(a, b):
    return a / b if b else float("inf")


# (項目, 件, 値の名前, 式, 合格の判定, 合格の表示)
TARGETS = [
    ("書き 1", "bulk-send-py", "(main - mem) / 書き",
     lambda v, n: div(v["#main"] - v["mem"], v["sendto"] - v["sendto!EAGAIN"]), lambda x: x <= 1.05, "≤ 1.05"),
    ("データのある読み (空きあり) 1", "echo-py-reuse", "(main - sendto - mem - 2E) / 往復",
     lambda v, n: (v["#main"] - v["sendto"] - v["mem"] - 2 * v["E"]) / n, lambda x: x <= 1.05, "≤ 1.05"),
    ("1往復", "echo-py-reuse", "main / 往復",
     lambda v, n: v["#main"] / n, lambda x: x <= 4, "≤ 4"),
    ("空きなしの読みは覗きと読みの 2 まで", "bulk-recv-py", "(R + P) / R",
     lambda v, n: div(v["R"] + v["P"], v["R"]), lambda x: x <= 2.02, "≤ 2.02"),
    ("空きなしの読みは覗きと読みの 2 まで", "echo-py-fresh", "(R + P) / R",
     lambda v, n: div(v["R"] + v["P"], v["R"]), lambda x: x <= 2.02, "≤ 2.02"),
    ("待つ読み 3", "echo-self-fresh", "(main - sendto - mem) / E",
     lambda v, n: div(v["#main"] - v["sendto"] - v["mem"], v["E"]), lambda x: x <= 3.05, "≤ 3.05"),
    ("待つ読み 3 の分母", "echo-self-fresh", "E / 往復",
     lambda v, n: v["E"] / n, lambda x: x >= 1.5, "≥ 1.5"),
    ("操作ごとの epoll_ctl 0", "echo-self-fresh", "epoll_ctl / 往復",
     lambda v, n: v["epoll_ctl"] / n, lambda x: x <= 0.005, "≤ 0.005"),
    ("操作ごとの epoll_ctl 0", "echo-py-reuse", "epoll_ctl / 往復",
     lambda v, n: v["epoll_ctl"] / n, lambda x: x <= 0.005, "≤ 0.005"),
    ("epoll_ctl の総数", "echo-py-reuse", "main の epoll_ctl (床を引かない)",
     lambda v, n: v["raw epoll_ctl"], lambda x: x <= 2, "≤ 2"),
    ("受けと登録", "conn-self-32", "accept4 / 接続",
     lambda v, n: v["accept4"] / n, lambda x: x <= 1.05, "≤ 1.05"),
    ("受けと登録", "conn-self-32", "epoll_ctl / 接続",
     lambda v, n: v["epoll_ctl"] / n, lambda x: x <= 2.005, "≤ 2.005"),
    ("待ち1回の費用", "echo-self-fresh", "(timerfd_settime + read(eventfd) + epoll_ctl) / 待ち",
     lambda v, n: div(v["per_wait"], v["waits"]), lambda x: x <= 0.05, "≤ 0.05"),
]
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


def strace_runs(c, label):
    """件 c の strace の各 run を、床を引いた回数の辞書にする。floor は base の1回目。"""
    b, r = get.get(("base", label, "strace")), get.get((c, label, "strace"))
    if not b or not r:
        return None, None
    bm = b["data"][0]["syscalls"].get("main", {})
    keys = set(SYS) | {"epoll_wait", "epoll_pwait", "epoll_pwait2", "timerfd_settime", "read(eventfd)", "epoll_ctl"}
    out = []
    for x in r["data"]:
        m = x["syscalls"].get("main", {})
        v = {k: m.get(k, 0) - bm.get(k, 0) for k in keys}
        # run.py は失敗した呼び出しを `名前` と `名前!ERRNO` の両方で数えるので、目標の式は `!` の鍵を除いた呼び出しの数を使う
        v["#main"] = calls(m) - calls(bm)
        v["raw epoll_ctl"] = m.get("epoll_ctl", 0)
        out.append(derive(v))
    return out, r["n_small"]


def calls(m):
    return sum(n for k, n in m.items() if "!" not in k)


def span(xs):
    lo, hi = min(xs), max(xs)
    return f"{lo:.3f}" if hi - lo < 0.0005 else f"{lo:.3f}-{hi:.3f}"


for label in labels:
    if not get.get(("base", label, "strace")):
        continue
    print(f"\n## 目標: {label} (strace の各 run の最小-最大。合格は全 run で満たすこと)\n")
    print("| 項目 | 件 | 式 | 値 | 合格 | 判定 |")
    print("|---|---|---|---|---|---|")
    for item, c, name, f, ok, shown in TARGETS:
        runs, n = strace_runs(c, label)
        if not runs:
            print(f"| {item} | {c} | {name} | - | {shown} | 件なし |")
            continue
        xs = [f(v, n) for v in runs]
        print(f"| {item} | {c} | {name} | {span(xs)} | {shown} | {'合格' if all(ok(x) for x in xs) else '不合格'} |")
    # 本数によらない費用は時間の run から出す。床を引いた1接続あたりの CPU の中央値の比
    per = {}
    for c in ("many-self-250", "many-self-2000"):
        t = get.get((c, label, "time"))
        # 床の base の時間の run がなければ判定しない
        if t and ("base", label, "time") in get:
            per[c] = (st.median(cpu(x) for x in t["data"]) - base_med(label, cpu)) / t["n_big"]
    if len(per) == 2:
        ratio = div(per["many-self-2000"], per["many-self-250"])
        print(f"| 本数によらない費用 | many-self-250、-2000 | 1接続あたり CPU の 2000 / 250 | {ratio:.3f} | ≤ 1.5 | {'合格' if ratio <= 1.5 else '不合格'} |")
    else:
        print("| 本数によらない費用 | many-self-250、-2000 | 1接続あたり CPU の 2000 / 250 | - | ≤ 1.5 | 件なし |")
    # 情報として、空きなしの読みに対する覗きの割合を並べる
    info = []
    for c in cases:
        runs, n = strace_runs(c, label)
        if runs and c != "base":
            xs = [div(v["P"], v["R"]) for v in runs if v["R"]]
            if xs:
                info.append(f"{c} {span(xs)}")
    if info:
        print("\nP / R (情報): " + "、".join(info))

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

# 1つ目のラベルを前とし、ほかのラベルをそれぞれ後として比べる
for b in labels[1:]:
    a = labels[0]
    print(f"\n## 比べ: {a} → {b} (CPU の中央値、床を引いて単位あたり µs)\n")
    print(f"| 件 | {a} | {b} | 比 | 生の CPU の比 | 四分位の重なり | 判定 | minflt/単位の差 | 確保の揺れ | 実時間の比 |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    for c in cases:
        ra, rb = get.get((c, a, "time")), get.get((c, b, "time"))
        if not ra or not rb:
            continue
        n = ra["n_big"]
        floor = c != "base"
        ca, cb = st.median(cpu(x) for x in ra["data"]), st.median(cpu(x) for x in rb["data"])
        pa = (ca - (base_med(a, cpu) if floor else 0)) / n * 1e6
        pb = (cb - (base_med(b, cpu) if floor else 0)) / n * 1e6
        qa, qb = q([cpu(x) for x in ra["data"]]), q([cpu(x) for x in rb["data"]])
        overlap = not (qa[1] < qb[0] or qb[1] < qa[0])
        ratio = pb / pa if pa else float("nan")
        verdict = "差" if (ratio <= 0.90 or ratio >= 1.10) and not overlap else "差なし"
        mf = lambda r, l: (st.median(x["minflt"] for x in r["data"]) - (base_med(l, lambda x: x["minflt"]) if floor else 0)) / n
        dm = mf(rb, b) - mf(ra, a)
        wa = st.median(x["wall"] for x in ra["data"]) - (base_med(a, lambda x: x["wall"]) if floor else 0)
        wb = st.median(x["wall"] for x in rb["data"]) - (base_med(b, lambda x: x["wall"]) if floor else 0)
        # 印は「比べる規則」の「確保の揺れ」で、判定を読む人が時間の判定から外す
        print(f"| {c} | {pa:.2f} | {pb:.2f} | {ratio:.3f} | {cb / ca:.3f} | {'重なる' if overlap else '重ならない'} | {verdict} | "
              f"{dm:.2f} | {'印' if floor and abs(dm) >= 1 else ''} | {wb / wa:.2f} |")
