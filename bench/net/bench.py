"""本物の I/O の物差しを流し、1行1件の JSON (JSONL) を標準出力に出す。

python3 -I bench.py [--runs 11] [--strace-runs 3] [--stats] [--only REGEX] [--no-time] [--no-strace] LABEL=EML[@DIR] ...

LABEL=EML@DIR のラベルは、件のプログラムを DIR から読む。前のコミットの eml が今のプログラムを型検査できないとき、
そのコミットの bench/net を取り出して渡すためである。件の行、引数、相手 (peer.py) はこのディレクトリのものを使う。
@DIR がなければ、このディレクトリのプログラムを使う。LABEL=EML を2つ以上渡すと、時間の run は件ごとに交互に (1回目は A B、2回目は B A、…) 流す。機械の揺れを片方に
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


def run(extra, cmd, env):
    r = subprocess.run([sys.executable, "-I", os.path.join(here, "run.py")] + extra + ["--"] + cmd,
                       capture_output=True, text=True, timeout=600, env={**os.environ, **env})
    if r.returncode != 0:
        sys.exit(r.stderr or r.stdout)
    return json.loads(r.stdout)


def command(case, eml, size, stats=False):
    eml, progs, _ = eml
    args = [] if case[size] == "-" else case[size].split()
    if case["name"].startswith("py:"):
        return [sys.executable, "-I", os.path.join(here, "peer.py")] + args
    return [eml, "run"] + (["--stats"] if stats else []) + [os.path.join(progs, case["prog"])] + args


def peer(case, size):
    p = case["peer_" + size]
    return [] if p == "-" else ["--peer", p]


def main():
    argv = sys.argv[1:]
    runs, strace_runs, stats, only, do_time, do_strace = 11, 3, False, None, True, True
    emls, envs = [], []
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
        elif a == "--env":
            envs.append((argv.pop(0), argv.pop(0)))
        else:
            label, eml = a.split("=", 1)
            eml, _, progs = eml.partition("@")
            emls.append((label, (os.path.abspath(eml), os.path.abspath(progs) if progs else here, {})))
    if not emls:
        sys.exit(__doc__)
    named = dict(emls)
    for label, kv in envs:
        key, eq, value = kv.partition("=")
        if label not in named or not eq:
            sys.exit(f"bench.py: --env {label} {kv}: ラベルがないか KEY=VALUE でない")
        named[label][2][key] = value
    # `--stats` を持たない古い eml では RunStats を取らずに続ける
    has_stats = {label: "--stats" in subprocess.run([eml, "run", "--help"], capture_output=True, text=True,
                                                    env={**os.environ, **env}).stdout
                 for label, (eml, _, env) in emls}
    for label, ok in has_stats.items():
        if stats and not ok:
            print(f"bench.py: {label} の eml に --stats がないので、RunStats を取らない", file=sys.stderr)

    def emit(case, label, kind, data, env):
        print(json.dumps({"case": case["name"], "label": label, "kind": kind, "unit": case["unit"],
                          "n_big": case["n_big"], "n_small": case["n_small"], "env": env, "data": data}), flush=True)

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
                    got[label] += run(peer(case, "big"), command(case, eml, "big"), eml[2])
            for label, eml in labels:
                emit(case, "python" if py else label, "time", got[label], {} if py else eml[2])
        if py:
            continue
        for label, eml in labels:
            if do_strace and sys.platform.startswith("linux"):
                emit(case, label, "strace", run(peer(case, "small") + ["--strace", "--runs", str(strace_runs)],
                                                command(case, eml, "small"), eml[2]), eml[2])
            if stats and has_stats[label]:
                emit(case, label, "stats", run(peer(case, "small"), command(case, eml, "small", stats=True), eml[2]), eml[2])


main()
