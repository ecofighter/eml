#!/usr/bin/env bash
# コンパイラの段階のハッシュ表のハッシュ関数を、FxHash (`rustc-hash` 2) と aHash、foldhash、rapidhash で比べる。
# 使い方: docs/plans/2026-10-24-hash-choice/hashers.sh [回数] [プログラム...]
#   回数の既定は 11。プログラムは bench/ のファイル名から `.em` を除いたもので、既定は下の `programs` である。
# 作業ツリーを一時のディレクトリに置き、release で4つの `eml` を組み立て、`eml check` と `eml run` を交互に走らせて、
# 命令の数 (macOS の `/usr/bin/time -l` の instructions retired) と CPU 時間 (user + sys) の中央値を Markdown の表で出す。
# Linux では命令の数を出さず、CPU 時間だけを出す。リポジトリの作業ツリーは変えない。
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(git -C "$here" rev-parse --show-toplevel)"
runs="${1:-11}"
shift || true
if (( $# > 0 )); then
  programs=("$@")
else
  programs=(empty fib nbody std_map spectral_norm fannkuch_var orders iter_pipeline rbtree upsert)
fi
variants=(fx ahash fold rapid)

work="$(mktemp -d)"
cleanup() {
  git -C "$root" worktree remove --force "$work/tree" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

git -C "$root" worktree add --detach "$work/tree" HEAD >/dev/null
tree="$work/tree"

# fx は手を加えない木で組む。残りは代わりの crate を `rustc-hash` に差し込み、cfg で関数を選ぶ
echo "hashers.sh: fx を組み立てる" >&2
CARGO_TARGET_DIR="$work/target-fx" cargo build -q --release -p eml_cli --manifest-path "$tree/Cargo.toml"
cp "$work/target-fx/release/eml" "$work/eml-fx"
printf '\n[patch.crates-io]\nrustc-hash = { path = "%s" }\n' "$here/shim" >>"$tree/Cargo.toml"
for v in ahash fold rapid; do
  echo "hashers.sh: $v を組み立てる" >&2
  RUSTFLAGS="--cfg eml_hash_$v" CARGO_TARGET_DIR="$work/target-$v" \
    cargo build -q --release -p eml_cli --manifest-path "$tree/Cargo.toml"
  cp "$work/target-$v/release/eml" "$work/eml-$v"
done

# ハッシュ関数を替えても出力が変わらないことを先に確かめる。変わるなら、どこかの出力が表の順に依存している
for p in "${programs[@]}"; do
  expected="$("$work/eml-fx" run "$root/bench/$p.em" | cksum)"
  for v in ahash fold rapid; do
    if [[ "$("$work/eml-$v" run "$root/bench/$p.em" | cksum)" != "$expected" ]]; then
      echo "hashers.sh: $p の出力が fx と $v で違う" >&2
      exit 1
    fi
  done
done

darwin=false
[[ "$(uname -s)" == Darwin ]] && darwin=true

# 1回走らせ、「命令の数 CPU 時間」を出す。命令の数が取れない Linux では 0 にする
measure_once() {
  local binary="$1" mode="$2" file="$3" report
  if $darwin; then
    report="$( { /usr/bin/time -l "$binary" "$mode" "$file" >/dev/null; } 2>&1 )"
    awk '/instructions retired/ { n = $1 } / real / { cpu = $3 + $5 } END { print n, cpu }' <<<"$report"
  else
    report="$( { TIMEFORMAT='%U %S'; time "$binary" "$mode" "$file" >/dev/null; } 2>&1 )"
    awk '{ print 0, $1 + $2 }' <<<"$report"
  fi
}

# ファイルの各行の値の中央値
median() {
  sort -g "$1" | awk '{ v[NR] = $1 } END { print (NR % 2) ? v[(NR + 1) / 2] : (v[NR / 2] + v[NR / 2 + 1]) / 2 }'
}

echo "- 機種: $(if $darwin; then echo "$(sysctl -n hw.model) ($(sysctl -n machdep.cpu.brand_string))"; else uname -m; fi)"
echo "- OS: $(if $darwin; then echo "macOS $(sw_vers -productVersion)"; else uname -sr; fi)"
echo "- rustc: $(rustc -V)"
echo "- コミット: $(git -C "$root" describe --always --dirty --abbrev=7)"
echo "- 回数: $runs (変種の順を回ごとに逆にして交互に走らせ、中央値をとる)"
echo
header="| モード | プログラム |"
rule="|---|---|"
for v in "${variants[@]}"; do
  header="$header $v 命令 | $v CPU (ms) |"
  rule="$rule---:|---:|"
done
echo "$header"
echo "$rule"
for mode in check run; do
  for p in "${programs[@]}"; do
    rm -f "$work"/n-* "$work"/t-*
    for (( i = 0; i < runs; i++ )); do
      order=("${variants[@]}")
      if (( i % 2 == 1 )); then
        order=(rapid fold ahash fx)
      fi
      for v in "${order[@]}"; do
        read -r n cpu <<<"$(measure_once "$work/eml-$v" "$mode" "$root/bench/$p.em")"
        echo "$n" >>"$work/n-$v"
        echo "$cpu" >>"$work/t-$v"
      done
    done
    base_n="$(median "$work/n-fx")"
    base_t="$(median "$work/t-fx")"
    row="| $mode | \`$p\` |"
    for v in "${variants[@]}"; do
      n="$(median "$work/n-$v")"
      t="$(median "$work/t-$v")"
      if [[ "$v" == fx ]]; then
        row="$row $($darwin && echo "$n" || echo -) | $(awk -v t="$t" 'BEGIN { printf "%.1f", t * 1000 }') |"
      else
        rel_n="$($darwin && awk -v a="$n" -v b="$base_n" 'BEGIN { printf "%+.2f%%", 100 * (a / b - 1) }' || echo -)"
        rel_t="$(awk -v a="$t" -v b="$base_t" 'BEGIN { printf "%.1f (%+.1f%%)", a * 1000, 100 * (a / b - 1) }')"
        row="$row $rel_n | $rel_t |"
      fi
    done
    echo "$row"
  done
done
