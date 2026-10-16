# logstat の Python の版。tests/ui/run/scripts/logstat/main.em と同じ標準出力と終了コードを返す
# (docs/implementation/benchmarks.md の「スクリプトの計測」)。
import io
import re
import sys
from collections import Counter

INT = re.compile(r"[+-]?[0-9]+")


# `String.to_int` と同じく、省ける符号と ASCII の数字だけを受け、`Int` の範囲を超えれば `None` である
def to_int(s):
    if not INT.fullmatch(s):
        return None
    n = int(s)
    return n if -(2**63) <= n < 2**63 else None


def usage():
    print("usage: logstat [-n N] [--whole]", file=sys.stderr)
    sys.exit(2)


# `Stdin.read_line` と同じく `\n` か `\r\n` を除く
def stdin_lines(stdin):
    for raw in stdin:
        if raw.endswith("\n"):
            raw = raw[:-1]
            if raw.endswith("\r"):
                raw = raw[:-1]
        yield raw


# `String.lines` と同じく `\n` で分け、各行の末尾の `\r` を1つ除く
def whole_lines(text):
    parts = text.split("\n")
    if parts[-1] == "":
        parts.pop()
    return [p[:-1] if p.endswith("\r") else p for p in parts]


def main():
    args = sys.argv[1:]
    top, whole = 10, False
    while args:
        if args[0] == "--whole":
            whole, args = True, args[1:]
        elif args[0] == "-n" and len(args) >= 2 and to_int(args[1]) is not None and to_int(args[1]) >= 0:
            top, args = to_int(args[1]), args[2:]
        else:
            usage()
    stdin = io.TextIOWrapper(sys.stdin.buffer, encoding="utf-8", newline="")
    lines = whole_lines(stdin.read()) if whole else stdin_lines(stdin)
    total = malformed = 0
    statuses, paths, durations = Counter(), Counter(), []
    for number, line in enumerate(lines, 1):
        total += 1
        fields = line.split(" ")
        status = to_int(fields[3]) if len(fields) == 5 else None
        duration = to_int(fields[4]) if len(fields) == 5 else None
        if status is None or duration is None:
            if malformed == 0:
                print(f"logstat: line {number}: malformed", file=sys.stderr)
            malformed += 1
            continue
        statuses[status] += 1
        paths[fields[2]] += 1
        durations.append(duration)
    print(f"lines {total}")
    print(f"malformed {malformed}")
    for status in sorted(statuses):
        print(f"status {status} {statuses[status]}")
    for path, count in sorted(paths.items(), key=lambda item: (-item[1], item[0]))[:top]:
        print(f"path {path} {count}")
    if durations:
        # eml の `/` と同じく 0 の方向に切り捨てる
        quotient = abs(sum(durations)) // len(durations)
        average = quotient if sum(durations) >= 0 else -quotient
        print(f"duration avg {average} max {max(durations)}")
    else:
        print("duration avg 0 max 0")


main()
