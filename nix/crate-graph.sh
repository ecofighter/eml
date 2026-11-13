#!/usr/bin/env bash
# workspace の crate の依存の辺、feature、他の crate からの再 export を表に持ち、cargo の解決した結果と比べる
# (docs/implementation/architecture.md の「crate の依存関係」)。辺・feature・再 export を変える変更は、同じコミットで下の表を直す。
# 使い方: nix/crate-graph.sh [--check]
# 引数なしは、いまの結果を表の見出しつきで書き出す (表に貼る用)。--check は表との差と閉包の検査をすべて走らせ、
# 1つでも外れれば失敗する (CI の crate graph の step)。cargo tree の出力は Windows の Git Bash でも読めるよう CR を落とす
set -euo pipefail

cd "$(dirname "$0")/.."

# 直接の辺 (normal、optional、target 固有)。crate ごとに1行
EDGES='eml_cabi: eml_extern
eml_cli: eml_clink eml_core_ir eml_diagnostics eml_hir eml_lir eml_native eml_syntax eml_tls eml_types eml_vm
eml_clink: eml_cabi eml_extern
eml_core_ir: eml_diagnostics eml_extern eml_hir eml_lir eml_types
eml_diagnostics:
eml_extern:
eml_gui:
eml_hir: eml_diagnostics eml_extern eml_syntax
eml_io: eml_rt eml_tls
eml_lir: eml_extern
eml_native: eml_cabi eml_extern eml_lir eml_nrt eml_rt
eml_nrt: eml_extern eml_rows eml_rt eml_sched
eml_rows: eml_extern eml_rt eml_sched
eml_rt:
eml_sched: eml_io eml_rt
eml_syntax: eml_diagnostics
eml_test_support: eml_cli eml_clink eml_core_ir eml_diagnostics eml_hir eml_lir eml_native eml_nrt eml_syntax eml_types eml_vm
eml_tls:
eml_types: eml_diagnostics eml_extern eml_hir
eml_vm: eml_cabi eml_extern eml_gui eml_io eml_lir eml_rows eml_rt eml_sched eml_tls'

# [features] の節 (空行とコメントを除く)
FEATURES='[eml_cli]
default = ["run", "tls", "gui"]
types = ["dep:eml_types"]
core = ["types", "dep:eml_core_ir", "dep:eml_lir"]
run = ["core", "dep:eml_vm", "dep:eml_clink", "dep:eml_native", "dep:libc", "dep:rustyline", "dep:windows-sys"]
tls = ["run", "eml_vm/tls", "dep:eml_tls"]
gui = ["run", "eml_vm/gui"]
gui-native = ["gui", "eml_vm/gui-native"]
[eml_gui]
native = [
    "dep:objc2",
    "dep:objc2-foundation",
    "dep:objc2-app-kit",
    "dep:objc2-core-foundation",
    "dep:cxx",
    "dep:cxx-build",
    "dep:qt-build-utils",
    "dep:windows-collections",
    "dep:windows-core",
    "dep:windows-reference",
    "dep:windows-time",
]
[eml_io]
tls = ["dep:eml_tls"]
[eml_sched]
testing = []
[eml_test_support]
default = ["run"]
hir = ["dep:eml_hir", "dep:eml_cli"]
types = ["hir", "dep:eml_types", "eml_cli/types"]
core = ["types", "dep:eml_core_ir", "dep:eml_lir", "eml_cli/core"]
run = ["core", "dep:eml_vm", "dep:eml_clink", "dep:eml_native", "dep:eml_nrt", "eml_cli/run"]
[eml_vm]
tls = ["dep:eml_tls", "eml_io/tls"]
gui = ["dep:eml_gui"]
gui-native = ["gui", "eml_gui/native"]'

failed=0

fail() {
  echo "crate-graph: $*" >&2
  failed=1
}

crates() {
  local dir
  for dir in crates/*/; do
    basename "$dir"
  done
}

edges() {
  local c list
  for c in $(crates); do
    list=$(cargo tree -p "$c" -e normal --depth 1 --prefix none --all-features --target all |
      tr -d '\r' | { grep '^eml_' || true; } | sed 's/ .*//' | { grep -vx "$c" || true; } | sort -u | paste -sd' ' -)
    if [ -n "$list" ]; then
      echo "$c: $list"
    else
      echo "$c:"
    fi
  done
}

features() {
  local c
  for c in $(crates); do
    if tr -d '\r' <"crates/$c/Cargo.toml" | grep -q '^\[features\]'; then
      echo "[$c]"
      tr -d '\r' <"crates/$c/Cargo.toml" |
        awk '/^\[features\]/{f=1; next} /^\[/{f=0} f' |
        grep -vE '^[[:space:]]*(#|$)' || true
    fi
  done
}

if [ "${1:-}" != "--check" ]; then
  echo "EDGES"
  edges
  echo "FEATURES"
  features
  exit 0
fi

compare() {
  local name=$1 table=$2 actual=$3
  if ! diff -u --label "$name (table)" --label "$name (cargo)" <(printf '%s\n' "$table") <(printf '%s\n' "$actual"); then
    fail "$name differs from its table; edit the table of nix/crate-graph.sh in the same commit"
  fi
}

compare EDGES "$EDGES" "$(edges)"
compare FEATURES "$FEATURES" "$(features)"

# cargo tree の結果のうち、行頭が PATTERN に合う行の数が WANT に等しいことを調べる
expect_tree() {
  local want=$1 pattern=$2 label=$3 got
  shift 3
  got=$(cargo tree "$@" --target all --prefix none | tr -d '\r' | { grep -cE "$pattern" || true; })
  if [ "$got" -ne "$want" ]; then
    fail "$label: $got lines match $pattern, expected $want"
  fi
}

TOOL='^(cranelift[a-z-]*|libloading|eml_lir|eml_core_ir|eml_types|eml_hir|eml_syntax|eml_diagnostics|eml_vm|eml_native|eml_clink|clap|rustyline|ariadne|logos|rowan) v'

expect_tree 0 '^(cranelift[a-z-]*|eml_native|eml_nrt|eml_clink) v' "eml_vm reaches the code generator" -p eml_vm -e normal
expect_tree 0 '^eml_vm v' "eml_native reaches eml_vm" -p eml_native -e normal
expect_tree 0 '^eml_rt v' "eml_lir reaches eml_rt" -p eml_lir -e normal
expect_tree 0 '^(eml_extern|eml_rows|eml_cabi|eml_tls|eml_gui|cranelift[a-z-]*|eml_lir|eml_vm|eml_nrt|eml_native) v' \
  "eml_sched reaches a crate above it" -p eml_sched -e normal
expect_tree 0 '^(cc|cmake|cxx-build|bindgen|nasm|pkg-config) v' "the runtime closure needs a C toolchain" -p eml_nrt -e normal,build
expect_tree 0 "$TOOL" "the runtime closure holds a compiler, an engine or a tool" -p eml_nrt -e normal

# 実行のバイナリの依存の木に、eml_sched の testing はない
testing=$(cargo tree -p eml_cli -e normal,build -f '{p} {f}' | tr -d '\r' | { grep -c 'eml_sched.*testing' || true; })
if [ "$testing" -ne 0 ]; then
  fail "eml_sched's feature testing is in eml_cli's tree"
fi

if grep -rn unsafe crates/eml_extern/src; then
  fail "crates/eml_extern/src has unsafe"
fi

exit "$failed"
