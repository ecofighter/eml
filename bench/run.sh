#!/usr/bin/env bash
# 基準のプログラムを release ビルドの CEK (`eml run`) と VM (`eml run --vm`) で3回ずつ走らせ、実行した命令の数と
# 実時間の中央値を Markdown の表で出し、終わりに2つのエンジンの実行の分を比べる表を出す
# (docs/implementation/benchmarks.md)。命令の数は macOS の `/usr/bin/time -l` の `instructions retired` で測る。
# valgrind が arm64 の macOS で動かないためである。
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "bench/run.sh: macOS の /usr/bin/time -l の instructions retired を使うので、macOS でだけ動く" >&2
  exit 1
fi

# 連想配列 (`declare -A`) と名前の参照 (`local -n`) は bash 4.3 から使える。macOS の /bin/bash は 3.2 なので、
# devShell の bash で走らせる
if (( BASH_VERSINFO[0] < 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] < 3) )); then
  echo "bench/run.sh: bash 4.3 以上が要る (devShell の bash で走らせる)。今の bash は $BASH_VERSION" >&2
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
# `$1` は `eml run` に足す引数 (空か `--vm`)。プログラムごとの命令の数の中央値を連想配列 `$2` に入れ、表を出す
measure() {
  local flag="$1"
  local -n result="$2"
  echo "| プログラム | instructions retired | 実時間 (s) |"
  echo "|---|---:|---:|"
  for file in "$root"/bench/*.em; do
    name="$(basename "$file" .em)"
    instructions=()
    seconds=()
    for _ in $(seq "$runs"); do
      # 実行時エラーで止まったら、捕まえた出力を見せて止める。表の値が欠けたまま記録しないためである
      if ! report="$( { /usr/bin/time -l "$eml" run $flag "$file" >/dev/null; } 2>&1 )"; then
        echo "bench/run.sh: $name ($flag) が失敗した" >&2
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
    result[$name]="$(median "${instructions[@]}")"
    echo "| \`$name\` | ${result[$name]} | $(median "${seconds[@]}") |"
  done
}

declare -A cek vm
echo "CEK"
echo
measure "" cek
echo
echo "VM (S10a の間は \`eml run --vm\`)"
echo
measure "--vm" vm
echo
# 実行の分は `empty` (フロントエンドと起動の費用) を引いた値で比べる (docs/implementation/benchmarks.md の「命令の数の測り方」)
echo "| プログラム | CEK の実行の分 | VM の実行の分 | VM / CEK |"
echo "|---|---:|---:|---:|"
for name in "${!cek[@]}"; do
  [[ "$name" == empty ]] && continue
  c=$(( ${cek[$name]} - ${cek[empty]} ))
  v=$(( ${vm[$name]} - ${vm[empty]} ))
  echo "| \`$name\` | $c | $v | $(awk "BEGIN { printf \"%.3f\", $v / $c }") |"
done | sort
