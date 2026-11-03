#!/usr/bin/env bash
# worker の数 N = 1、2、4 で、Async のタスク、同じ仕事の par、別プロセスの床、channel の往復の実時間を測り、
# Markdown の表を標準出力に出す (docs/implementation/benchmarks.md の「ネットワークの計測」の `bench/async/scale.sh`)。
# EML (既定は release ビルドの target/release/eml。省くとビルドする) と ROUNDS (既定 11) を環境変数で受ける
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
if [ -z "${EML:-}" ]; then
  cargo build --release -p eml_cli --manifest-path "$root/Cargo.toml" >&2
  EML="$root/target/release/eml"
fi
export EML ROUNDS="${ROUNDS:-11}"
exec python3 -I - "$root" <<'PY'
import os
import platform
import shutil
import statistics as st
import subprocess
import sys
import time

root = sys.argv[1]
eml = os.environ["EML"]
rounds = int(os.environ["ROUNDS"])
here = os.path.join(root, "bench", "async")
NS = [1, 2, 4]
FIB30 = 832040


def spread(prog, n):
    return [[eml, "run", "--workers", str(n), os.path.join(here, prog), "8", "30"]], f"spread done {8 * FIB30}"


def floor(n):
    # 1 worker のプロセスを N 個同時に起こし、8 個の fib 30 を分け合う。機械そのものの伸びの対照である
    k = 8 // n
    cmd = [eml, "run", "--workers", "1", os.path.join(here, "spread.em"), str(k), "30"]
    return [cmd] * n, f"spread done {k * FIB30}"


def pingpong(n):
    return [[eml, "run", "--workers", str(n), os.path.join(here, "chan_pingpong.em")]], "800040000"


MEASURES = [
    ("spread", lambda n: spread("spread.em", n)),
    ("spread_par", lambda n: spread("spread_par.em", n)),
    ("床 (別プロセス)", floor),
    ("chan_pingpong", pingpong),
]


def run(cmds, expected):
    """cmds を同時に起こし、最後のプロセスが終わるまでの実時間 (ms) を返す。出力が違えば止める"""
    start = time.perf_counter()
    procs = [subprocess.Popen(c, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
             for c in cmds]
    outs = [p.communicate() for p in procs]
    elapsed = (time.perf_counter() - start) * 1000
    for c, p, (out, err) in zip(cmds, procs, outs):
        if p.returncode != 0 or out.strip() != expected:
            sys.exit(f"scale.sh: {' '.join(c)}: exit {p.returncode}, stdout {out.strip()!r} (want {expected!r})\n{err}")
    return elapsed


times = {(name, n): [] for name, _ in MEASURES for n in NS}
for r in range(rounds):
    # 回ごとに物差しの順を回し、N の順も入れ替える。機械の揺れを1つの物差しや N に寄せないためである
    order = MEASURES[r % len(MEASURES):] + MEASURES[:r % len(MEASURES)]
    ns = NS if r % 2 == 0 else NS[::-1]
    for name, make in order:
        for n in ns:
            times[(name, n)].append(run(*make(n)))

print("| 物差し | t(1) ms | t(2) | t(4) | S(2) | S(4) |")
print("|---|---|---|---|---|---|")
for name, _ in MEASURES:
    t = {n: st.median(times[(name, n)]) for n in NS}
    print(f"| {name} | {t[1]:.0f} | {t[2]:.0f} | {t[4]:.0f} | {t[1] / t[2]:.2f} | {t[1] / t[4]:.2f} |")


def machine():
    if shutil.which("lscpu"):
        info = {}
        for line in subprocess.run(["lscpu"], capture_output=True, text=True).stdout.splitlines():
            k, _, v = line.partition(":")
            info.setdefault(k.strip(), v.strip())
        keys = ["Model name", "CPU(s)", "Thread(s) per core", "Core(s) per socket", "Socket(s)"]
        return "lscpu: " + "、".join(f"{k} {info[k]}" for k in keys if k in info)
    if platform.system() == "Darwin":
        out = subprocess.run(["sysctl", "-n", "machdep.cpu.brand_string", "hw.ncpu"], capture_output=True, text=True).stdout
        return "sysctl: " + "、".join(out.split("\n")[:2])
    return "機械: " + platform.platform()


print()
print(f"{machine()}。中央値は {rounds} 回の生の実時間、S(N) = t(1) / t(N)")
PY
