# wc の Python の版。tests/ui/run/scripts/wc/main.em と同じ標準出力と終了コードを返す
# (docs/implementation/benchmarks.md の「スクリプトの計測」)。
import sys

# 語は ASCII の空白 (空白、`\t`、`\n`、`\x0B`、`\x0C`、`\r`) で区切る。`bytes.split()` の区切りと同じである
def counts(data):
    return [data.count(b"\n"), len(data.split()), len(data)]


def main():
    args = sys.argv[1:]
    chosen = []
    while args and args[0] in ("-l", "-w", "-c"):
        chosen.append("lwc".index(args[0][1]))
        args = args[1:]
    if args and args[0].startswith("-") and args[0] != "-":
        print("usage: wc [-l] [-w] [-c] [FILE...]", file=sys.stderr)
        sys.exit(2)
    shown = sorted(set(chosen)) or [0, 1, 2]
    names = args or [None]
    total = [0, 0, 0]
    failed = False
    for name in names:
        if name is None or name == "-":
            data = sys.stdin.buffer.read()
        else:
            try:
                with open(name, "rb") as f:
                    data = f.read()
            except OSError as e:
                print(f"wc: {name}: {e.strerror}", file=sys.stderr)
                failed = True
                continue
        found = counts(data)
        total = [a + b for a, b in zip(total, found)]
        print(" ".join([str(found[i]) for i in shown] + ([name] if name else [])))
    if len(names) >= 2:
        print(" ".join([str(total[i]) for i in shown] + ["total"]))
    sys.exit(1 if failed else 0)


main()
