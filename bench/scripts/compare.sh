#!/usr/bin/env bash
# スクリプトの eml の版と Python の版を、今のディレクトリで同じ引数と同じ標準入力で動かし、標準出力と終了コードを
# 比べる (docs/implementation/benchmarks.md の「スクリプトの計測」)。標準エラーの文言は比べない。
#
#   compare.sh NAME [ARG...]     標準入力をそのまま両方に渡す
#   compare.sh --fixture NAME    tests/ui/run/scripts/NAME/ の写しの中で、main.args を引数に、main.stdin を標準入力に
#                                する。各側は自分の写しの中で動き、書いたファイルも比べる (リポジトリにファイルを残さない)
#
# 引数の `@OUT` は各側の一時ファイルのパスに置き換え、実行の後でその中身を標準出力の後ろに足して比べる。書き出す
# ファイルも比べるためである。eml は `$EML` (既定は debug ビルドの `target/debug/eml`) を使う。
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
eml="${EML:-$root/target/debug/eml}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fixture=""
if [[ "${1:-}" == "--fixture" ]]; then
  name="$2"
  fixture="$root/tests/ui/run/scripts/$name"
  cd "$fixture"
  args=()
  if [[ -f main.args ]]; then
    while IFS= read -r line || [[ -n "$line" ]]; do
      args+=("$line")
    done < main.args
  fi
  if [[ -f main.stdin ]]; then
    cp main.stdin "$work/stdin"
  else
    : > "$work/stdin"
  fi
else
  name="$1"
  shift
  args=("$@")
  cat > "$work/stdin"
fi

# 側 `$1` のコマンド (`$2` 以降) を動かし、標準出力、`@OUT` の中身、終了コードを `$work/$1` に書く
run_side() {
  local side="$1"
  shift
  local out="$work/$side.out"
  local command=()
  local arg
  for arg in "$@"; do
    command+=("${arg//@OUT/$out}")
  done
  local code=0
  if [[ -n "$fixture" ]]; then
    cp -R "$fixture" "$work/$side.dir"
    (cd "$work/$side.dir" && "${command[@]}" < "$work/stdin" > "$work/$side" 2> "$work/$side.err") || code=$?
    # 付属のファイルにないか、中身の変わったファイルは、スクリプトが書いたものである
    local file
    while IFS= read -r file; do
      if ! cmp -s "$work/$side.dir/$file" "$fixture/$file"; then
        printf -- '--- written: %s ---\n' "$file" >> "$work/$side"
        cat "$work/$side.dir/$file" >> "$work/$side"
      fi
    done < <(cd "$work/$side.dir" && find . -type f | sort)
  else
    "${command[@]}" < "$work/stdin" > "$work/$side" 2> "$work/$side.err" || code=$?
  fi
  if [[ -f "$out" ]]; then
    printf -- '--- @OUT ---\n' >> "$work/$side"
    cat "$out" >> "$work/$side"
  fi
  printf -- '--- exit: %s ---\n' "$code" >> "$work/$side"
}

run_side eml "$eml" run "$root/tests/ui/run/scripts/$name/main.em" ${args[@]+"${args[@]}"}
run_side python python3 -I "$root/bench/scripts/$name.py" ${args[@]+"${args[@]}"}

if cmp -s "$work/eml" "$work/python"; then
  echo same
else
  diff -u --label eml --label python "$work/eml" "$work/python" || true
  echo "--- eml stderr ---"
  cat "$work/eml.err"
  exit 1
fi
