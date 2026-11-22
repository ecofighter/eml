#!/usr/bin/env bash
# usage: gate-rounds.sh BASE_BIN HEAD_BIN ROUNDS ROW...        (run from the repository root; ROW is a name under bench/)
# Per round runs `empty` and the row with both binaries at one worker and prints, per row, the median of the
# deltas (%) of the run part (instructions of the row minus instructions of `empty`).
set -euo pipefail
base_bin=${1:?BASE_BIN} head_bin=${2:?HEAD_BIN} rounds=${3:?ROUNDS}
shift 3
export EML_WORKERS=1
count() { { /usr/bin/time -l "$1" run "$2" >/dev/null; } 2>&1 | awk '/instructions retired/ {print $1}'; }
for row in "$@"; do
  deltas=()
  for _ in $(seq "$rounds"); do
    be=$(count "$base_bin" bench/empty.em); br=$(count "$base_bin" "bench/$row.em")
    he=$(count "$head_bin" bench/empty.em); hr=$(count "$head_bin" "bench/$row.em")
    deltas+=("$(python3 -c "print(f'{(($hr - $he) / ($br - $be) - 1) * 100:.3f}')")")
  done
  med=$(printf '%s\n' "${deltas[@]}" | sort -g | sed -n "$(( (rounds + 1) / 2 ))p")
  echo "$row $med"
done
