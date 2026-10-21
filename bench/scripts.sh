#!/usr/bin/env bash
# スクリプトと数値計算のプログラムを、release ビルドの `eml run` (VM の速いモード) と Python の版の両方で3回ずつ
# 走らせ、実時間と最大の常駐メモリの中央値を Markdown の表で出す (docs/implementation/benchmarks.md の「スクリプトの
# 計測」)。行ごとの大きさは、eml の実時間が約 300 ms になるように決めてある。測る前に、両方の出力が一致することを
# bench/scripts/compare.sh で確かめる。macOS の `/usr/bin/time -l` を使うので、macOS でだけ動く。
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

# 種類 `$1` (`log` か `csv`) で約 `$2` KiB の入力のパスを出す。生成に時間がかかるので、あれば作り直さない。名前に大きさを
# 入れるので、ほかの大きさで作ったファイルを測ることはない
input() {
  local kind="$1" kib="$2" path
  if [[ "$kind" == log ]]; then
    path="$data/access-${kib}k.log"
  else
    path="$data/data-${kib}k.csv"
  fi
  if [[ ! -f "$path" ]]; then
    # コマンド置換の中では `set -e` が効かないので、失敗を返り値で伝える
    python3 -I "$root/bench/scripts/gen.py" "$kind" "$kib" "$path" >&2 || return 1
  fi
  echo "$path"
}

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

# 計測 `$1` の大きさの欄 `$2` の行を出す。eml の版 `$3` と Python の版 `$4` を、標準入力 `$5` と引数 (`$6` 以降) で
# 比べてから測る。引数の `@OUT` は書き出すファイルのパスである
row() {
  local label="$1" size="$2" eml_file="$3" py_file="$4" stdin="$5"
  shift 5
  if ! EML="$eml" "$root/bench/scripts/compare.sh" --paths "$eml_file" "$py_file" "$@" < "$stdin" > "$data/compare.txt"; then
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
  result="$(measure "$stdin" "$eml" run "$eml_file" ${args[@]+"${args[@]}"})"
  read -r eml_seconds eml_mb <<<"$result"
  result="$(measure "$stdin" python3 -I "$py_file" ${args[@]+"${args[@]}"})"
  read -r python_seconds python_mb <<<"$result"
  local ratio
  ratio="$(awk -v a="$eml_seconds" -v b="$python_seconds" 'BEGIN { if (b > 0) printf "%.1f", a / b; else print "-" }')"
  echo "| $label | $size | $eml_seconds | $python_seconds | $ratio | $eml_mb | $python_mb |"
}

# スクリプト `$4` の行。入力は種類 `$2` の約 `$3` KiB で、引数 (`$5` 以降) に `@IN` があればそのパスとして渡し、
# なければ標準入力にする
script_row() {
  local label="$1" kind="$2" kib="$3" name="$4"
  shift 4
  local path stdin args=() arg
  path="$(input "$kind" "$kib")"
  stdin="$path"
  for arg in "$@"; do
    if [[ "$arg" == @IN ]]; then
      args+=("$path")
      stdin=/dev/null
    else
      args+=("$arg")
    fi
  done
  row "$label" "$kib KiB" "$scripts/$name/main.em" "$root/bench/scripts/$name.py" "$stdin" ${args[@]+"${args[@]}"}
}

# 数値計算のプログラム `bench/$2.em` と `bench/numeric/$3.py` の行。大きさ `$4` を引数で渡す
numeric_row() {
  local label="$1" program="$2" py="$3" size="$4"
  row "$label" "$size" "$root/bench/$program.em" "$root/bench/numeric/$py.py" /dev/null "$size"
}

header() {
  echo "| 計測 | 大きさ | eml の実時間 (s) | Python の実時間 (s) | 比 (eml / Python) | eml の最大 RSS (MB) | Python の最大 RSS (MB) |"
  echo "|---|---:|---:|---:|---:|---:|---:|"
}

echo "- 機種: $(sysctl -n hw.model) ($(sysctl -n machdep.cpu.brand_string))"
echo "- OS: macOS $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
echo "- rustc: $(rustc -V)"
echo "- Python: $(python3 -V)"
echo "- コミット: $(git -C "$root" describe --always --dirty --abbrev=7)"
echo "- 入力: スクリプトの入力は bench/scripts/gen.py が作る。大きさは表の「大きさ」の欄にある"
echo
echo "スクリプト"
echo
header
script_row '`wc`' log 9000 wc
# 約 0.5% の行が一致する。末尾の空白で `/api/v1/items/70` などを除く
script_row '`grep -n`' log 70000 grep -n "/api/v1/items/7 "
script_row '`logstat`' log 6700 logstat
script_row '`logstat --whole`' log 6700 logstat --whole
script_row '`csv2jsonl`' csv 2900 csv2jsonl --columns id,name,score @IN @OUT
echo
echo "数値計算"
echo
header
numeric_row nbody nbody nbody 140000
numeric_row spectral-norm spectral_norm spectral_norm 510
numeric_row mandelbrot mandelbrot mandelbrot 480
# n が1つ増えると実時間が約9倍になり、300 ms に近い n がない。300 ms との比が最も小さい 9 にする
numeric_row fannkuch-redux fannkuch_var fannkuch 9
numeric_row sieve sieve_var sieve 3500000
