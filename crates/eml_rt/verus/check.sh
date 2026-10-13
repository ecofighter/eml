#!/bin/sh
# eml_rt の Verus の写しを検証する (docs/implementation/testing.md の「Verus」)。Verus は rustup の toolchain を使うので、
# Nix の devShell の外の環境で流す。
set -eu
VERUS="${VERUS:-$HOME/.verus/verus-arm64-macos/verus}"
VERSION=0.2026.10.04.426d8b0
cd "$(dirname "$0")"
run() {
    env -i HOME="$HOME" PATH="$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin" "$VERUS" "$@"
}
run --version | grep -q "$VERSION" || {
    echo "check.sh: Verus $VERSION is required (docs/implementation/testing.md)" >&2
    exit 1
}
run --crate-type=lib lib.rs "$@"
