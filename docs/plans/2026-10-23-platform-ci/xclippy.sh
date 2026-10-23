#!/usr/bin/env bash
# CI の Windows (msvc) と Linux の target で clippy を流す (link はしない)。使い方: xclippy.sh <repo の絶対パス> [clippy の引数...]
set -euo pipefail
repo=$1; shift
if [ $# -eq 0 ]; then set -- --workspace --all-targets -- -D warnings; fi
here=$(cd "$(dirname "$0")" && pwd)
tc=$(nix build --impure --no-link --print-out-paths --expr "import $here/xtarget.nix { repo = \"path:$repo\"; }")
for t in x86_64-pc-windows-msvc x86_64-unknown-linux-gnu; do
  echo "== $t"
  (cd "$repo" && PATH="$tc/bin:$PATH" CARGO_TARGET_DIR="$repo/target/xclippy" cargo clippy --target "$t" "$@")
done
