# スクリプトの計測の入力を作る (docs/implementation/benchmarks.md の「スクリプトの計測」)。種を固定するので、同じ
# 大きさを指定すれば毎回同じファイルになる。
#
#   python3 -I bench/scripts/gen.py OUTDIR [--mb 30]
#
# OUTDIR/access.log はアクセスログ (`時刻 メソッド パス ステータス 応答時間`)、OUTDIR/data.csv はヘッダ付きの CSV で、
# それぞれ約 `--mb` MB である。
import argparse
import os
import random

METHODS = ["GET", "GET", "GET", "POST", "PUT", "DELETE"]
STATUSES = [200] * 12 + [201, 204, 301, 304, 400, 404, 404, 500]
PATHS = [f"/api/v1/items/{k}" for k in range(190)] + [f"/ドキュメント/{k}" for k in range(10)]
NAMES = ["Alice", "Bob", "Carol", "Dave", "山田 太郎", "佐藤 花子", "O'Brien", "Zoë"]
NOTES = [
    "plain note",
    "note, with a comma",
    'says "hello"',
    "two\nlines",
    "日本語のメモ",
    "tab\tinside",
]


def write_log(path, size, rng):
    with open(path, "w", encoding="utf-8", newline="") as f:
        written = number = 0
        while written < size:
            number += 1
            hour, rest = divmod(number % 86400, 3600)
            stamp = f"2026-10-17T{hour:02d}:{rest // 60:02d}:{rest % 60:02d}Z"
            fields = [stamp, rng.choice(METHODS), rng.choice(PATHS), str(rng.choice(STATUSES)), str(rng.randint(1, 2000))]
            # 約1万行に1行、欄の欠けた形の崩れた行を混ぜる
            if rng.randrange(10000) == 0:
                fields.pop()
            line = " ".join(fields) + "\n"
            f.write(line)
            written += len(line.encode("utf-8"))


def quote(field):
    if any(c in field for c in ',"\n'):
        return '"' + field.replace('"', '""') + '"'
    return field


def write_csv(path, size, rng):
    with open(path, "w", encoding="utf-8", newline="") as f:
        f.write("id,name,email,note,score\n")
        written = number = 0
        while written < size:
            number += 1
            name = rng.choice(NAMES)
            fields = [str(number), name, f"user{number}@example.com", rng.choice(NOTES), str(rng.randint(0, 100))]
            line = ",".join(quote(field) for field in fields) + "\n"
            f.write(line)
            written += len(line.encode("utf-8"))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("outdir")
    parser.add_argument("--mb", type=int, default=30)
    args = parser.parse_args()
    os.makedirs(args.outdir, exist_ok=True)
    rng = random.Random(12)
    size = args.mb * 1024 * 1024
    write_log(os.path.join(args.outdir, "access.log"), size, rng)
    write_csv(os.path.join(args.outdir, "data.csv"), size, rng)


main()
