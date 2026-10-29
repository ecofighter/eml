# csv2jsonl の Python の実装。tests/ui/run/scripts/csv2jsonl/main.em と同じ標準出力、書き出すファイル、終了コードを返す
# (docs/implementation/benchmarks.md の「スクリプトの計測」)。比べるのは正しい入力の結果だけで、誤りの文言と行番号は
# eml の実装に合わせていない。
import csv
import json
import sys


def fail(message, code):
    print(f"csv2jsonl: {message}", file=sys.stderr)
    sys.exit(code)


def main():
    args = sys.argv[1:]
    if len(args) != 4 or args[0] != "--columns":
        fail("usage: csv2jsonl --columns NAME,NAME... INPUT OUTPUT", 2)
    columns, source, target = args[1].split(","), args[2], args[3]
    try:
        with open(source, encoding="utf-8", newline="") as f:
            # 空の行は飛ばす。`csv.reader` は空の行を空のリストにする
            rows = [row for row in csv.reader(f, strict=True) if row]
    except (OSError, UnicodeDecodeError, csv.Error) as e:
        fail(str(e), 1)
    if not rows:
        fail("line 1: missing header", 1)
    header, records = rows[0], rows[1:]
    for column in columns:
        if column not in header:
            fail(f"unknown column `{column}`", 2)
    picked = [header.index(column) for column in columns]
    out = []
    for record in records:
        if len(record) != len(header):
            fail(f"expected {len(header)} fields, found {len(record)}", 1)
        # 同じ列名を2回選んでも2回書くように、辞書を作らずに並べる
        fields = (
            json.dumps(columns[n], ensure_ascii=False) + ":" + json.dumps(record[i], ensure_ascii=False)
            for n, i in enumerate(picked)
        )
        out.append("{" + ",".join(fields) + "}\n")
    with open(target, "w", encoding="utf-8", newline="") as f:
        f.write("".join(out))


main()
