#!/bin/bash
# eml バイナリに入る他者の crate のライセンスの全文を、devShell の cargo-about で THIRD-PARTY-LICENSES.md に書き出す
# (docs/implementation/architecture.md の「外部 crate」)。
# 使い方: nix develop -c nix/third-party-licenses.sh [--check]
# --check は書き出さずに、生成した結果とコミットしたファイルを比べ、違えば失敗する (CI の licenses ジョブ)
set -euo pipefail

cd "$(dirname "$0")/.."

# --frozen は ClearlyDefined を引かず、手元の crate のファイルだけを読むので、出力がネットワークに左右されない。
# その代わり、ほかの OS でだけ使う crate も含めて、Cargo.lock の crate を先に揃えておく
cargo fetch --locked

out=$(mktemp)
log=$(mktemp)
trap 'rm -f "$out" "$log"' EXIT
cargo-about generate --frozen --fail -m crates/eml_cli/Cargo.toml -o "$out" about.hbs 2>"$log"
# about.toml で指定したファイルの checksum が crate の更新で合わなくなると、cargo-about は警告だけを出して指定を捨て、
# 自動の判定が拾う別のファイルや SPDX の雛形の文面に戻る。そのまま書き出すと、その文面に替わったことに気づけないので、
# 警告を誤りとして扱う
if [ -s "$log" ]; then
  cat "$log" >&2
  echo "third-party-licenses: cargo-about が警告を出した。about.toml の範囲と checksum を crate の今のファイルに合わせる" >&2
  exit 1
fi

case "${1:-}" in
  "")
    cat "$out" >THIRD-PARTY-LICENSES.md
    ;;
  --check)
    if ! diff -u THIRD-PARTY-LICENSES.md "$out"; then
      echo "third-party-licenses: THIRD-PARTY-LICENSES.md が生成した結果と違う。nix develop -c nix/third-party-licenses.sh で書き直す" >&2
      exit 1
    fi
    ;;
  *)
    echo "使い方: nix/third-party-licenses.sh [--check]" >&2
    exit 2
    ;;
esac
