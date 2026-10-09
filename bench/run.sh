#!/usr/bin/env bash
# 基準のプログラムを release ビルドで3回ずつ走らせ、実行した命令の数と実時間の中央値を Markdown の表で出す
# (docs/implementation/benchmarks.md)。命令の数は macOS の `/usr/bin/time -l` の `instructions retired` で測る。
# valgrind が arm64 の macOS で動かないためである。
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "bench/run.sh: macOS の /usr/bin/time -l の instructions retired を使うので、macOS でだけ動く" >&2
  exit 1
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# 中央値を1つの値に決めるため、奇数にする
runs=3

cargo build --release -p eml_cli --manifest-path "$root/Cargo.toml" >&2
eml="$root/target/release/eml"

# `runs` 個の値の中央値
median() {
  printf '%s\n' "$@" | sort -n | sed -n "$(( (runs + 1) / 2 ))p"
}

echo "- 機種: $(sysctl -n hw.model) ($(sysctl -n machdep.cpu.brand_string))"
echo "- OS: macOS $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
echo "- rustc: $(rustc -V)"
echo "- コミット: $(git -C "$root" describe --always --dirty --abbrev=7)"
echo
echo "| プログラム | instructions retired | 実時間 (s) |"
echo "|---|---:|---:|"
for file in "$root"/bench/*.em; do
  name="$(basename "$file" .em)"
  instructions=()
  seconds=()
  for _ in $(seq "$runs"); do
    # 実行時エラーで止まったら、捕まえた出力を見せて止める。表の値が欠けたまま記録しないためである
    if ! report="$( { /usr/bin/time -l "$eml" run "$file" >/dev/null; } 2>&1 )"; then
      echo "bench/run.sh: $name が失敗した" >&2
      echo "$report" >&2
      exit 1
    fi
    count="$(awk '/instructions retired/ { print $1 }' <<<"$report")"
    elapsed="$(awk '/ real / { print $1 }' <<<"$report")"
    # macOS の版や実行の環境によって `time -l` が数を出さないと、表に空の欄が混ざるので止める
    if [[ ! "$count" =~ ^[0-9]+$ || ! "$elapsed" =~ ^[0-9]+\.[0-9]+$ ]]; then
      echo "bench/run.sh: $name の /usr/bin/time -l の出力から命令の数か実時間を読めなかった" >&2
      echo "$report" >&2
      exit 1
    fi
    instructions+=("$count")
    seconds+=("$elapsed")
  done
  echo "| \`$name\` | $(median "${instructions[@]}") | $(median "${seconds[@]}") |"
done
