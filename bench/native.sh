#!/usr/bin/env bash
# 基準のプログラムを release ビルドの `eml run` (VM) と `eml run --native` で3回ずつ走らせ、実行した命令の数と実時間の
# 中央値、native / VM の比 (実行の分どうし) を Markdown の表で出す (docs/implementation/benchmarks.md の「ネイティブの数値」)。
# 数は記録で、門ではない。ネイティブが断るプログラムは `rejected`、VM と標準出力が違うプログラムは `differs` と出す。
# 命令の数は macOS の `/usr/bin/time -l` の `instructions retired` で測る (bench/run.sh と同じ)。
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "bench/native.sh: macOS の /usr/bin/time -l の instructions retired を使うので、macOS でだけ動く" >&2
  exit 1
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runs=3
# 1回の実行の上限 (秒)。止まらないプログラムで表が出ないままになるのを防ぐ
limit=60

cargo build --release -p eml_cli --manifest-path "$root/Cargo.toml" >&2
eml="$root/target/release/eml"
# `instructions retired` はプロセスのすべてのスレッドを数える。ネイティブは worker が1つなので、VM も1つに揃える
export EML_WORKERS=1
# `--native` は EML_GUI が設定されていると断るので、両方の実行から外す
unset EML_GUI

limited() {
  if command -v timeout >/dev/null 2>&1; then
    timeout "$limit" "$@"
  else
    "$@"
  fi
}

median() {
  printf '%s\n' "$@" | sort -n | sed -n "$(( (runs + 1) / 2 ))p"
}

# `$1` が `eml run` の引数 (`--native` か空)、`$2` がファイルのとき、命令の数と実時間の中央値を空白で区切って出す
measure() {
  local mode="$1" file="$2"
  local name report count elapsed
  local instructions=() seconds=()
  name="$(basename "$file" .em)"
  for _ in $(seq "$runs"); do
    # shellcheck disable=SC2086
    if ! report="$( { limited /usr/bin/time -l "$eml" run $mode "$file" >/dev/null; } 2>&1 )"; then
      echo "bench/native.sh: $name ($mode) が失敗した" >&2
      echo "$report" >&2
      exit 1
    fi
    count="$(awk '/instructions retired/ { print $1 }' <<<"$report")"
    elapsed="$(awk '/ real / { print $1 }' <<<"$report")"
    if [[ ! "$count" =~ ^[0-9]+$ || ! "$elapsed" =~ ^[0-9]+\.[0-9]+$ ]]; then
      echo "bench/native.sh: $name ($mode) の /usr/bin/time -l の出力から命令の数か実時間を読めなかった" >&2
      echo "$report" >&2
      exit 1
    fi
    instructions+=("$count")
    seconds+=("$elapsed")
  done
  echo "$(median "${instructions[@]}") $(median "${seconds[@]}")"
}

# VM の `--stats` の仕事の数のうち、名前 `$2` の値 (`$1` は stats の行)
stat() {
  tr ' ' '\n' <<<"$1" | sed -n "s/^$2=//p"
}

echo "- 機種: $(sysctl -n hw.model) ($(sysctl -n machdep.cpu.brand_string))"
echo "- OS: macOS $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
echo "- rustc: $(rustc -V)"
echo "- コミット: $(git -C "$root" describe --always --dirty --abbrev=7)"
echo
echo "| プログラム | VM 命令 | native 命令 | native / VM (実行の分) | VM 実時間 (s) | native 実時間 (s) | dup (VM) | decref (VM) | alloc (VM) | reuse (VM) |"
echo "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|"

vm_base=0
native_base=0
# `empty` を先に測る (比の基準になる)
files=("$root/bench/empty.em")
for file in "$root"/bench/*.em; do
  [[ "$file" == "$root/bench/empty.em" ]] || files+=("$file")
done
for file in "${files[@]}"; do
  name="$(basename "$file" .em)"
  # 標準出力の比べは、断られたかどうかの判定も兼ねる。断りは終了コード1と標準エラーの E0004 で、実行時の失敗と区別する
  vm_err="$(mktemp)"
  vm_out="$(limited "$eml" run "$file" 2>"$vm_err")" || {
    echo "bench/native.sh: $name が VM で失敗した (終了コード $?)" >&2
    cat "$vm_err" >&2
    rm -f "$vm_err"
    exit 1
  }
  rm -f "$vm_err"
  native_err="$(mktemp)"
  native_status=0
  native_out="$(limited "$eml" run --native "$file" 2>"$native_err")" || native_status=$?
  rejected=0
  if (( native_status == 1 )) && grep -q 'E0004' "$native_err"; then
    rejected=1
  elif (( native_status != 0 )); then
    echo "bench/native.sh: $name の --native が終了コード $native_status で失敗した" >&2
    cat "$native_err" >&2
    rm -f "$native_err"
    exit 1
  fi
  rm -f "$native_err"

  if (( rejected )); then
    echo "| \`$name\` | | rejected | rejected | | | | | | |"
    continue
  fi
  if [[ "$vm_out" != "$native_out" ]]; then
    echo "| \`$name\` | differs | differs | differs | | | | | | |"
    continue
  fi

  read -r vm_count vm_seconds <<<"$(measure "" "$file")"
  read -r native_count native_seconds <<<"$(measure --native "$file")"
  stats="$(limited "$eml" run --stats "$file" 2>&1 >/dev/null | grep '^stats:' || true)"
  if [[ "$name" == empty ]]; then
    vm_base="$vm_count"
    native_base="$native_count"
    ratio=""
  else
    vm_part=$(( vm_count - vm_base ))
    native_part=$(( native_count - native_base ))
    # 実行の分が 0 以下なら、比は意味を持たない
    if (( vm_part > 0 && native_part > 0 )); then
      ratio="$(awk -v n="$native_part" -v v="$vm_part" 'BEGIN { printf "%.2f", n / v }')"
    else
      ratio="-"
    fi
  fi
  echo "| \`$name\` | $vm_count | $native_count | $ratio | $vm_seconds | $native_seconds | $(stat "$stats" rc_increments) | $(stat "$stats" rc_decrements) | $(stat "$stats" allocations) | $(stat "$stats" reuses) |"
done
