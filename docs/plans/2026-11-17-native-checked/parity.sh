#!/usr/bin/env bash
# usage: parity.sh EML TOP(run|run-fail) OUTDIR     (run from the repository root)
# Runs every UI file of tests/ui/TOP with `EML run --native --stats` and, where native code accepts it, with
# `EML run --stats`, one worker, each run alone under watch.sh, and writes SAME / DIFF / REJECTED / SKIPMARK per file to
# OUTDIR/summary.txt. `norm.py OUTDIR` then lists the DIFF files that still differ without `peak_stack`.
EML=$(cd "$(dirname "$1")" && pwd)/$(basename "$1"); TOP=$2; OUT=$3
W=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/watch.sh
ROOT=$PWD/tests/ui/$TOP
mkdir -p "$OUT"; OUT=$(cd "$OUT" && pwd); : > "$OUT/summary.txt"
cd "$PWD/tests" || exit 1
for f in $(find "$ROOT" -mindepth 2 -maxdepth 2 -name '*.em' | sort) $(find "$ROOT" -mindepth 3 -maxdepth 3 -name main.em | sort); do
  name=${f#"$ROOT"/}; key=$(echo "$name" | tr '/' '_')
  if head -5 "$f" | grep -q '^-- native: skip'; then echo "SKIPMARK $name" >> "$OUT/summary.txt"; continue; fi
  EML_WORKERS=1 "$W" 2097152 "$EML" run --native --stats "$f" < /dev/null > "$OUT/$key.n.out" 2> "$OUT/$key.n.err"
  if grep -q 'E0004' "$OUT/$key.n.err"; then echo "REJECTED $name" >> "$OUT/summary.txt"; rm -f "$OUT/$key".n.*; continue; fi
  EML_WORKERS=1 "$W" 2097152 "$EML" run --stats "$f" < /dev/null > "$OUT/$key.v.out" 2> "$OUT/$key.v.err"
  if cmp -s "$OUT/$key.n.out" "$OUT/$key.v.out" && cmp -s "$OUT/$key.n.err" "$OUT/$key.v.err"; then
    echo "SAME $name" >> "$OUT/summary.txt"; rm -f "$OUT/$key".[nv].*
  else
    echo "DIFF $name" >> "$OUT/summary.txt"
  fi
done
cut -d' ' -f1 "$OUT/summary.txt" | sort | uniq -c
