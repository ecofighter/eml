#!/usr/bin/env bash
# eml_rt の速いモードの計測用プログラム (crates/eml_rt/examples/rt_bench.rs と、extern の rt_extern_bench.rs) を
# release ビルドで3回ずつ走らせ、実行した命令の数と実時間の中央値を、プログラムごとに Markdown の表で出す
# (docs/implementation/benchmarks.md)。測り方は bench/run.sh と同じで、
# macOS の `/usr/bin/time -l` の `instructions retired` を使う。
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "bench/rt.sh: macOS の /usr/bin/time -l の instructions retired を使うので、macOS でだけ動く" >&2
  exit 1
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# 中央値を1つの値に決めるため、奇数にする
runs=3

cargo build --release -p eml_rt --example rt_bench --example rt_extern_bench \
  --manifest-path "$root/Cargo.toml" >&2
examples="$root/target/release/examples"

# `runs` 個の値の中央値
median() {
  printf '%s\n' "$@" | sort -n | sed -n "$(( (runs + 1) / 2 ))p"
}

# measure が出した行の中央値 ("計測 命令の数 実時間")。比の表が引く
results=""

# measure <プログラム> <計測>…: 計測ごとに1行の表を出す
measure() {
  local bin="$1"
  shift
  echo "| 計測 | instructions retired | 実時間 (s) |"
  echo "|---|---:|---:|"
  local case instructions seconds report count elapsed ins_median sec_median
  for case in "$@"; do
    instructions=()
    seconds=()
    for _ in $(seq "$runs"); do
      if ! report="$( { /usr/bin/time -l "$bin" "$case" >/dev/null; } 2>&1 )"; then
        echo "bench/rt.sh: $case が失敗した" >&2
        echo "$report" >&2
        exit 1
      fi
      count="$(awk '/instructions retired/ { print $1 }' <<<"$report")"
      elapsed="$(awk '/ real / { print $1 }' <<<"$report")"
      # 数を読めないと表に空の欄が混ざるので止める
      if [[ ! "$count" =~ ^[0-9]+$ || ! "$elapsed" =~ ^[0-9]+\.[0-9]+$ ]]; then
        echo "bench/rt.sh: $case の /usr/bin/time -l の出力から命令の数か実時間を読めなかった" >&2
        echo "$report" >&2
        exit 1
      fi
      instructions+=("$count")
      seconds+=("$elapsed")
    done
    ins_median="$(median "${instructions[@]}")"
    sec_median="$(median "${seconds[@]}")"
    results+="$case $ins_median $sec_median"$'\n'
    echo "| \`$case\` | $ins_median | $sec_median |"
  done
}

# ratios <eml の計測> <参照の計測>…: 組ごとに eml / 参照の比を表の1行に出す
ratios() {
  echo "| eml の計測 | 参照の計測 | 命令の数の比 | 実時間の比 |"
  echo "|---|---|---:|---:|"
  local eml ref
  while [[ $# -gt 0 ]]; do
    eml="$1"
    ref="$2"
    shift 2
    awk -v eml="$eml" -v ref="$ref" '
      $1 == eml { ei = $2; es = $3 }
      $1 == ref { ri = $2; rs = $3 }
      END {
        if (ei == "" || ri == "") { print "bench/rt.sh: " eml " か " ref " の計測がない" > "/dev/stderr"; exit 1 }
        # 実時間は 0.01 秒きざみなので、参照が 0 秒なら比を出さない
        time = (rs + 0 > 0) ? sprintf("%.2f", es / rs) : "-"
        printf "| `%s` | `%s` | %.2f | %s |\n", eml, ref, ei / ri, time
      }' <<<"$results"
  done
}

echo "- 機種: $(sysctl -n hw.model) ($(sysctl -n machdep.cpu.brand_string))"
echo "- OS: macOS $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
echo "- rustc: $(rustc -V)"
echo "- コミット: $(git -C "$root" describe --always --dirty --abbrev=7)"
echo
measure "$examples/rt_bench" \
  empty list box-list dup free map map-reuse append perform perform-deep handle yield mask poll
echo
echo "extern の計測 (rt_extern_bench)"
echo
measure "$examples/rt_extern_bench" \
  string-eq string-eq-early compare concat-copy show-int show-int-long show-string show-string-dense \
  println-block println-line read-all read-all-unsized \
  find-byte-1mib find-byte-1mib-position find-bytes-1mib find-bytes-1mib-windows \
  find-byte find-byte-swar find-sub find-sub-contains slice-short slice-short-box \
  read-all-bytes read-all-bytes-std read-line-bytes-64mib read-line-bytes-64mib-std \
  stdout-write-line-10m
echo
echo "eml / 参照の比 (1 MiB の2組は目標が2倍、続く4組は予算の行、最後の組は記録だけの行。docs/implementation/benchmarks.md)"
echo
ratios \
  find-byte-1mib find-byte-1mib-position \
  find-bytes-1mib find-bytes-1mib-windows \
  find-byte find-byte-swar \
  find-sub find-sub-contains \
  slice-short slice-short-box \
  read-all-bytes read-all-bytes-std \
  read-line-bytes-64mib read-line-bytes-64mib-std
