#!/usr/bin/env bash
# S12d のスクリプトを、生成した約 30 MB の入力で、release ビルドの `eml run` (VM の速いモード) と Python の版の両方で
# 3回ずつ走らせ、実時間と最大の常駐メモリの中央値を Markdown の表で出す (docs/implementation/benchmarks.md の
# 「スクリプトの計測」)。測る前に、両方の出力が一致することを bench/scripts/compare.sh で確かめる。macOS の
# `/usr/bin/time -l` を使うので、macOS でだけ動く。
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "bench/scripts.sh: macOS の /usr/bin/time -l を使うので、macOS でだけ動く" >&2
  exit 1
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# 中央値を1つの値に決めるため、奇数にする
runs=3
data="$root/target/bench-scripts"
scripts="$root/tests/ui/run/scripts"

cargo build --release -p eml_cli --manifest-path "$root/Cargo.toml" >&2
eml="$root/target/release/eml"
# 入力は生成に時間がかかるので、あれば作り直さない
if [[ ! -f "$data/access.log" || ! -f "$data/data.csv" ]]; then
  python3 -I "$root/bench/scripts/gen.py" "$data" >&2
fi

median() {
  printf '%s\n' "$@" | sort -n | sed -n "$(( (runs + 1) / 2 ))p"
}

# 標準入力 `$1` で、コマンド (`$2` 以降) を `runs` 回走らせ、実時間 (秒) と最大の常駐メモリ (MB) の中央値を空白で
# 区切って出す
measure() {
  local input="$1"
  shift
  local report elapsed rss
  local seconds=()
  local sizes=()
  for _ in $(seq "$runs"); do
    # 実行時エラーで止まったら、捕まえた出力を見せて止める。表の値が欠けたまま記録しないためである。プログラムの
    # 標準エラーも `report` に入るが、読むのは `time -l` の行だけである
    if ! report="$( { /usr/bin/time -l "$@" < "$input" > /dev/null; } 2>&1 )"; then
      echo "bench/scripts.sh: $* が失敗した" >&2
      echo "$report" >&2
      exit 1
    fi
    elapsed="$(awk '/ real / { print $1 }' <<<"$report")"
    rss="$(awk '/maximum resident set size/ { print $1 }' <<<"$report")"
    if [[ ! "$elapsed" =~ ^[0-9]+\.[0-9]+$ || ! "$rss" =~ ^[0-9]+$ ]]; then
      echo "bench/scripts.sh: /usr/bin/time -l の出力から実時間か常駐メモリを読めなかった" >&2
      echo "$report" >&2
      exit 1
    fi
    seconds+=("$elapsed")
    sizes+=("$rss")
  done
  echo "$(median "${seconds[@]}") $(( $(median "${sizes[@]}") / 1024 / 1024 ))"
}

# 計測 `$1` を、標準入力 `$2` とスクリプト `$3` と引数 (`$4` 以降) で比べてから測り、表の1行を出す。引数の `@OUT` は
# 書き出すファイルのパスである
row() {
  local label="$1" input="$2" name="$3"
  shift 3
  if ! EML="$eml" "$root/bench/scripts/compare.sh" "$name" "$@" < "$input" > "$data/compare.txt"; then
    echo "bench/scripts.sh: $label の出力が eml と Python で違う" >&2
    cat "$data/compare.txt" >&2
    exit 1
  fi
  local args=()
  local arg
  for arg in "$@"; do
    args+=("${arg//@OUT/$data/out.txt}")
  done
  local result eml_seconds eml_mb python_seconds python_mb
  result="$(measure "$input" "$eml" run "$scripts/$name/main.em" ${args[@]+"${args[@]}"})"
  read -r eml_seconds eml_mb <<<"$result"
  result="$(measure "$input" python3 -I "$root/bench/scripts/$name.py" ${args[@]+"${args[@]}"})"
  read -r python_seconds python_mb <<<"$result"
  local ratio
  ratio="$(awk -v a="$eml_seconds" -v b="$python_seconds" 'BEGIN { if (b > 0) printf "%.1f", a / b; else print "-" }')"
  echo "| $label | $eml_seconds | $python_seconds | $ratio | $eml_mb | $python_mb |"
}

echo "- 機種: $(sysctl -n hw.model) ($(sysctl -n machdep.cpu.brand_string))"
echo "- OS: macOS $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
echo "- rustc: $(rustc -V)"
echo "- Python: $(python3 -V)"
echo "- コミット: $(git -C "$root" describe --always --dirty --abbrev=7)"
echo "- 入力: access.log $(( $(wc -c < "$data/access.log") / 1024 / 1024 )) MB ($(wc -l < "$data/access.log" | tr -d ' ') 行)、data.csv $(( $(wc -c < "$data/data.csv") / 1024 / 1024 )) MB"
echo
echo "| 計測 | eml の実時間 (s) | Python の実時間 (s) | 比 (eml / Python) | eml の最大 RSS (MB) | Python の最大 RSS (MB) |"
echo "|---|---:|---:|---:|---:|---:|"
row '`wc`' "$data/access.log" wc
# 約 0.5% の行が一致する。末尾の空白で `/api/v1/items/70` などを除く
row '`grep -n`' "$data/access.log" grep -n "/api/v1/items/7 "
row '`logstat`' "$data/access.log" logstat
row '`logstat --whole`' "$data/access.log" logstat --whole
row '`csv2jsonl`' /dev/null csv2jsonl --columns id,name,score "$data/data.csv" @OUT
