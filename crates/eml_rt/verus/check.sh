#!/bin/sh
# eml_rt の Verus の写しを検証する (docs/implementation/testing.md の「Verus」)。Verus は Nix の devShell が flake から入れる。
set -eu
cd "$(dirname "$0")"
exec verus --crate-type=lib lib.rs "$@"
