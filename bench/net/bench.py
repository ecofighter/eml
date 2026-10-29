"""本物の I/O の物差しを流し、1行1件の JSON (JSONL) を標準出力に出す。

python3 -I bench.py [--runs 11] [--strace-runs 3] [--stats] [--only REGEX] [--no-time] [--no-strace] LABEL=EML ...

LABEL=EML を2つ以上渡すと、時間の run は件ごとに交互に (1回目は A B、2回目は B A、…) 流す。機械の揺れを片方に
寄せないためである。--stats は `EML run --stats` (検査モード) を1回流して RunStats を取る。件は cases.tsv の行である。
"""
import json
import os
import re
import subprocess
import sys

here = os.path.dirname(os.path.abspath(__file__))


def cases():
    for line in open(os.path.join(here, "cases.tsv")):
        if line.startswith("#") or not line.strip():
            continue
        f = line.rstrip("\n").split("\t")
        yield dict(name=f[0], prog=f[1], big=f[2], small=f[3], peer_big=f[4], peer_small=f[5],
                   n_big=int(f[6]), n_small=int(f[7]), unit=f[8])


def run(extra, cmd):
    r = subprocess.run([sys.executable, "-I", os.path.join(here, "run.py")] + extra + ["--"] + cmd,
                       capture_output=True, text=True, timeout=600)
    if r.returncode != 0:
        sys.exit(r.stderr or r.stdout)
    return json.loads(r.stdout)


def command(case, eml, size, stats=False):
    args = [] if case[size] == "-" else case[size].split()
    if case["name"].startswith("py:"):
        return [sys.executable, "-I", os.path.join(here, "peer.py")] + args
    return [eml, "run"] + (["--stats"] if stats else []) + [os.path.join(here, case["prog"])] + args


def peer(case, size):
    p = case["peer_" + size]
    return [] if p == "-" else ["--peer", p]


def main():
    argv = sys.argv[1:]
    runs, strace_runs, stats, only, do_time, do_strace = 11, 3, False, None, True, True
    emls = []
    while argv:
        a = argv.pop(0)
        if a == "--runs":
            runs = int(argv.pop(0))
        elif a == "--strace-runs":
            strace_runs = int(argv.pop(0))
        elif a == "--stats":
            stats = True
        elif a == "--only":
            only = re.compile(argv.pop(0))
        elif a == "--no-time":
            do_time = False
        elif a == "--no-strace":
            do_strace = False
        else:
            label, eml = a.split("=", 1)
            emls.append((label, os.path.abspath(eml)))
    if not emls:
        sys.exit(__doc__)
    # `--stats` を持たない古い eml では RunStats を取らずに続ける
    has_stats = {label: "--stats" in subprocess.run([eml, "run", "--help"], capture_output=True, text=True).stdout
                 for label, eml in emls}
    for label, ok in has_stats.items():
        if stats and not ok:
            print(f"bench.py: {label} の eml に --stats がないので、RunStats を取らない", file=sys.stderr)

    def emit(case, label, kind, data):
        print(json.dumps({"case": case["name"], "label": label, "kind": kind, "unit": case["unit"],
                          "n_big": case["n_big"], "n_small": case["n_small"], "data": data}), flush=True)

    for case in cases():
        if only and not only.search(case["name"]):
            continue
        py = case["name"].startswith("py:")
        labels = emls[:1] if py else emls
        if do_time:
            got = {label: [] for label, _ in labels}
            for i in range(runs):
                order = labels if i % 2 == 0 else labels[::-1]
                for label, eml in order:
                    got[label] += run(peer(case, "big"), command(case, eml, "big"))
            for label, _ in labels:
                emit(case, "python" if py else label, "time", got[label])
        if py:
            continue
        for label, eml in labels:
            if do_strace and sys.platform.startswith("linux"):
                emit(case, label, "strace", run(peer(case, "small") + ["--strace", "--runs", str(strace_runs)],
                                                command(case, eml, "small")))
            if stats and has_stats[label]:
                emit(case, label, "stats", run(peer(case, "small"), command(case, eml, "small", stats=True)))


main()
