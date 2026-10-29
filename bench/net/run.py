"""1 つの物差しを流し、測った値を JSON の1行 (run ごとの辞書のリスト) で出す。

python3 -I run.py [--peer 'MODE ARGS'] [--strace] [--runs R] [--pin CPUS] [--peer-pin CPUS] -- CMD ARGS...

CMD の引数の `{port}` は、相手 (peer.py) が出したポートに置き換える。
--strace がなければ、実時間、user と sys の CPU、最大の常駐メモリ (KiB)、自発と非自発の切り替えを測る。
--strace なら `strace -f` の下で流し、スレッドごと (main、eml-monitor など PR_SET_NAME の名前、ほかは thread) に
syscall の名前ごとの数と、失敗の数 (`名前!ERRNO`) を出す。eventfd と timerfd の `read` は `read(eventfd)` と
`read(timerfd)` に分ける。時間は歪むので記録しない。Linux でだけ使える。
CMD の標準エラーに `stats: name=value ...` の行 (`eml run --stats`) があれば、その欄をすべて出す。
--pin は CMD を、--peer-pin は相手を `taskset -c` で CPU に固定する (Linux)。
"""
import json
import os
import re
import subprocess
import sys
import tempfile
import time

here = os.path.dirname(os.path.abspath(__file__))
darwin = sys.platform == "darwin"


def parse():
    opts = {"peer": None, "strace": False, "runs": 1, "pin": None, "peer_pin": None}
    argv = sys.argv[1:]
    while argv and argv[0] != "--":
        a = argv.pop(0)
        if a == "--peer":
            opts["peer"] = argv.pop(0)
        elif a == "--strace":
            opts["strace"] = True
        elif a == "--runs":
            opts["runs"] = int(argv.pop(0))
        elif a == "--pin":
            opts["pin"] = argv.pop(0)
        elif a == "--peer-pin":
            opts["peer_pin"] = argv.pop(0)
        else:
            sys.exit(f"unknown option {a}")
    if opts["strace"] and darwin:
        sys.exit("--strace は Linux でだけ使える")
    if (opts["pin"] or opts["peer_pin"]) and darwin:
        sys.exit("--pin は Linux でだけ使える")
    return opts, argv[1:]


def pinned(cpus, cmd):
    return ["taskset", "-c", cpus] + cmd if cpus else cmd


def start_peer(spec, cpus):
    cmd = pinned(cpus, [sys.executable, "-I", os.path.join(here, "peer.py")] + spec.split())
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE, text=True)
    port = p.stdout.readline().strip()
    if not port:
        sys.exit(f"peer did not start: {spec}")
    return p, port


START = re.compile(r"^(\d+)\s+(\w+)\((.*)$")
RESUMED = re.compile(r"^(\d+)\s+<\.\.\. (\w+) resumed>(.*)$")
RESULT = re.compile(r"=\s+(-?\d+|\?)(?:\s+(E[A-Z0-9]+))?")


def strace_counts(path):
    main = None
    names = {}
    eventfds, timerfds = set(), set()
    counts = {}
    pending = {}

    def bump(tid, key):
        d = counts.setdefault(tid, {})
        d[key] = d.get(key, 0) + 1

    def result(tid, name, rest, args):
        m = RESULT.search(rest)
        if not m:
            return
        ret, err = m.group(1), m.group(2)
        if err:
            bump(tid, f"{name}!{err}")
        elif name == "eventfd2":
            eventfds.add(ret)
        elif name == "timerfd_create":
            timerfds.add(ret)

    for line in open(path):
        m = RESUMED.match(line)
        if m:
            tid, name, rest = m.groups()
            result(tid, pending.pop(tid, name), rest, "")
            continue
        m = START.match(line)
        if not m:
            continue
        tid, name, rest = m.groups()
        if main is None:
            main = tid
        if name == "prctl":
            n = re.search(r'PR_SET_NAME, "([^"]*)"', rest)
            if n:
                names[tid] = n.group(1)
        key = name
        if name == "read":
            fd = rest.split(",", 1)[0].strip()
            if fd in eventfds:
                key = "read(eventfd)"
            elif fd in timerfds:
                key = "read(timerfd)"
        bump(tid, key)
        if "<unfinished ...>" in rest:
            pending[tid] = key
        else:
            result(tid, key, rest, rest)
    # 名前は PR_SET_NAME の後で分かるので、スレッドの番号で数えてから名前にまとめる
    out = {}
    for tid, d in counts.items():
        who = "main" if tid == main else names.get(tid, "thread")
        o = out.setdefault(who, {})
        for k, v in d.items():
            o[k] = o.get(k, 0) + v
    return out


def once(opts, cmd):
    peer, port = (None, None)
    if opts["peer"]:
        peer, port = start_peer(opts["peer"], opts["peer_pin"])
    cmd = [c.replace("{port}", port or "") for c in cmd]
    trace = None
    if opts["strace"]:
        fd, trace = tempfile.mkstemp(suffix=".strace")
        os.close(fd)
        cmd = ["strace", "-f", "-qq", "-o", trace] + cmd
    cmd = pinned(opts["pin"], cmd)
    errf = tempfile.TemporaryFile(mode="w+")
    t0 = time.perf_counter()
    p = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=errf)
    _, status, ru = os.wait4(p.pid, 0)
    wall = time.perf_counter() - t0
    if peer:
        peer.wait(timeout=60)
    errf.seek(0)
    err = errf.read()
    code = os.waitstatus_to_exitcode(status)
    if code != 0:
        sys.exit(f"failed ({code}): {' '.join(cmd)}\n{err}")
    # macOS の ru_maxrss はバイト、Linux は KiB
    rss = ru.ru_maxrss // 1024 if darwin else ru.ru_maxrss
    out = {"wall": wall, "user": ru.ru_utime, "sys": ru.ru_stime, "maxrss_kib": rss,
           "nvcsw": ru.ru_nvcsw, "nivcsw": ru.ru_nivcsw, "minflt": ru.ru_minflt}
    m = re.search(r"^stats: (.*)$", err, re.M)
    if m:
        out["stats"] = {k: int(v) for k, v in re.findall(r"(\w+)=(\d+)", m.group(1))}
    if trace:
        out = {"syscalls": strace_counts(trace)}
        os.unlink(trace)
    return out


def main():
    opts, cmd = parse()
    print(json.dumps([once(opts, cmd) for _ in range(opts["runs"])]))


main()
