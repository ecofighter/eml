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
runs=3

cargo build --release -p eml_cli --manifest-path "$root/Cargo.toml" >&2
eml="$root/target/release/eml"

# 3つの値の中央値
median() {
  printf '%s\n' "$@" | sort -n | sed -n '2p'
}

echo "- 機種: $(sysctl -n hw.model) ($(sysctl -n machdep.cpu.brand_string))"
echo "- OS: macOS $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
echo "- rustc: $(rustc -V)"
echo "- コミット: $(git -C "$root" rev-parse --short HEAD)"
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
    instructions+=("$(awk '/instructions retired/ { print $1 }' <<<"$report")")
    seconds+=("$(awk '/ real / { print $1 }' <<<"$report")")
  done
  echo "| \`$name\` | $(median "${instructions[@]}") | $(median "${seconds[@]}") |"
done
