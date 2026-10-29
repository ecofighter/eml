# grep の Python の実装。tests/ui/run/scripts/grep/main.em と同じ標準出力と終了コードを返す
# (docs/implementation/benchmarks.md の「スクリプトの計測」)。
import io
import sys


# `String.lines` と同じく `\n` で分け、各行の末尾の `\r` を1つ除く。最後の行の終わりは省ける
def file_lines(text):
    parts = text.split("\n")
    if parts[-1] == "":
        parts.pop()
    return [p[:-1] if p.endswith("\r") else p for p in parts]


# `Stdin.read_line` と同じく、1行ずつ読んで `\n` か `\r\n` を除く
def stdin_lines():
    for raw in io.TextIOWrapper(sys.stdin.buffer, encoding="utf-8", newline=""):
        if raw.endswith("\n"):
            raw = raw[:-1]
            if raw.endswith("\r"):
                raw = raw[:-1]
        yield raw


def main():
    args = sys.argv[1:]
    flags = set()
    while args and args[0] in ("-v", "-n", "-c"):
        flags.add(args[0])
        args = args[1:]
    if not args or (args[0].startswith("-") and args[0] != "-"):
        print("usage: grep [-v] [-n] [-c] PATTERN [FILE...]", file=sys.stderr)
        sys.exit(2)
    pattern, names = args[0], args[1:]
    prefixed = len(names) >= 2
    failed = matched = False
    out = []
    for name in names or ["-"]:
        if name == "-":
            lines = stdin_lines()
        else:
            try:
                with open(name, encoding="utf-8", newline="") as f:
                    lines = file_lines(f.read())
            except (OSError, UnicodeDecodeError) as e:
                print(f"grep: {name}: {e}", file=sys.stderr)
                failed = True
                continue
        prefix = f"{name}:" if prefixed else ""
        count = 0
        for number, line in enumerate(lines, 1):
            if (pattern in line) != ("-v" in flags):
                count += 1
                if "-c" not in flags:
                    out.append(f"{prefix}{number}:{line}" if "-n" in flags else f"{prefix}{line}")
        if "-c" in flags:
            out.append(f"{prefix}{count}")
        matched = matched or count > 0
    sys.stdout.write("".join(line + "\n" for line in out))
    sys.exit(2 if failed else 0 if matched else 1)


main()
