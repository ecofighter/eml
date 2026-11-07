#!/bin/bash
# WinUIのbackendのbinding (crates/eml_gui/src/winui/bindings.rs) を、windows-bindgenでWindows App SDKのメタデータから作り直す
# (docs/implementation/architecture.mdの「`eml_gui`の内部」)。
# 使い方: nix develop -c nix/winui-bindings.sh [--check]
# --checkは書き出さずに、生成した結果とコミットしたファイルを比べ、違えば失敗する。メタデータとcrateをネットワークから取るので、
# CIでは走らせない
set -euo pipefail

cd "$(dirname "$0")/.."

# メタデータは、microsoft/windows-rsが`release`の版のWindows App SDKのNuGetのパッケージから写し、
# crates/tools/reactor-metadata/winmdに置いた`.winmd`である。コミットとファイルごとのSHA-256で固定し、emlは生成したbindingだけを
# コミットする。版を上げるときは、crates/eml_gui/src/winui/bootstrap.rsの`MIN_VERSION`も上げる
release=2.6.4
commit=2b5f63797a464bbe571b444b1f19dbd26d98c228
base="https://raw.githubusercontent.com/microsoft/windows-rs/$commit/crates/tools/reactor-metadata/winmd"
winmds=(
  "Microsoft.UI.winmd f70326853a3244cbdc647e690e538d1769752136df8548dacb2e0f8c804327b1"
  "Microsoft.UI.Xaml.winmd a1ad041e172cfd82492964200d1361dd0652b17ba0a81209cab6f9953f958145"
)
target=crates/eml_gui/src/winui/bindings.rs

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/winmd"

sha256() {
  if command -v sha256sum >/dev/null; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

for entry in "${winmds[@]}"; do
  read -r name sum <<<"$entry"
  curl -sSfL -o "$tmp/winmd/$name" "$base/$name"
  actual=$(sha256 "$tmp/winmd/$name")
  if [ "$actual" != "$sum" ]; then
    echo "winui-bindings: ${name}のSHA-256が${actual}で、固定した${sum}と違う" >&2
    exit 1
  fi
done

# windows-bindgenのメンバーごとのfilterは、--minimal --flatの形で働く。windows-reactorも同じ形を使う。全部を出す形では、
# 生成しないインターフェースを使うfactoryの補助を出してしまう
args=(
  --minimal --flat
  --implement Microsoft.UI.Xaml.IApplicationOverrides Microsoft.UI.Xaml.Markup.IXamlMetadataProvider
  --compose Microsoft.UI.Xaml.Application
  --filter-file nix/winui-bindings/filter.txt
)
cargo run --quiet --locked --manifest-path nix/winui-bindings/Cargo.toml --target-dir target/winui-bindings -- \
  --in "$tmp/winmd" default --out "$tmp/generated.rs" "${args[@]}"

bindgen=$(grep -A1 '^name = "windows-bindgen"$' nix/winui-bindings/Cargo.lock | sed -n 's/^version = "\(.*\)"$/\1/p')
{
  echo "// nix/winui-bindings.shが生成した。手では直さない。"
  echo "// 生成: windows-bindgen ${bindgen}"
  echo "// メタデータ: Windows App SDK ${release}のMicrosoft.UI.winmdとMicrosoft.UI.Xaml.winmd (NuGetのパッケージ"
  echo "// Microsoft.WindowsAppSDK.InteractiveExperiencesとMicrosoft.WindowsAppSDK.WinUIのもの)。microsoft/windows-rs"
  echo "// (MIT OR Apache-2.0) がコミット${commit}のcrates/tools/reactor-metadata/winmdに置いたものを使う。"
  echo "// windows-reactorも同じメタデータからbindingを生成する。"
  echo "// 作り直し: nix develop -c nix/winui-bindings.sh (--checkは比べるだけ)"
  cat "$tmp/generated.rs"
} >"$tmp/bindings.rs"

case "${1:-}" in
  "")
    cp "$tmp/bindings.rs" "$target"
    ;;
  --check)
    if ! diff -u "$target" "$tmp/bindings.rs"; then
      echo "winui-bindings: ${target}が生成した結果と違う。nix develop -c nix/winui-bindings.shで書き直す" >&2
      exit 1
    fi
    ;;
  *)
    echo "使い方: nix/winui-bindings.sh [--check]" >&2
    exit 2
    ;;
esac
