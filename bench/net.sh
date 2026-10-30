#!/usr/bin/env bash
# Std.Net の物差し (bench/net/cases.tsv の件) を release ビルドの eml で流し、bench/net/bench.py の JSONL を標準出力に出す
# (docs/implementation/benchmarks.md の「ネットワークの計測」)。引数は bench.py にそのまま渡し、比べる eml は LABEL=EML で渡す
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cargo build --release -p eml_cli --manifest-path "$root/Cargo.toml" >&2
exec python3 -I "$root/bench/net/bench.py" "$@"
