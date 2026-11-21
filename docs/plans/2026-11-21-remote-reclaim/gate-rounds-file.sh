#!/usr/bin/env bash
# usage: gate-rounds-file.sh BASE_BIN HEAD_BIN ROUNDS LABEL FILE [ARG...]   (run from the repository root)
# The same as gate-rounds.sh for one program given by path with its arguments. Prints
# `LABEL <median delta % of the run part> <median instructions per run, base> <the same, head>`.
set -euo pipefail
base_bin=${1:?BASE_BIN} head_bin=${2:?HEAD_BIN} rounds=${3:?ROUNDS} label=${4:?LABEL} file=${5:?FILE}
shift 5
export EML_WORKERS=1
count() { local bin=$1; shift; { /usr/bin/time -l "$bin" run "$@" >/dev/null; } 2>&1 | awk '/instructions retired/ {print $1}'; }
deltas=() bases=() heads=()
for _ in $(seq "$rounds"); do
  be=$(count "$base_bin" bench/empty.em); br=$(count "$base_bin" "$file" "$@")
  he=$(count "$head_bin" bench/empty.em); hr=$(count "$head_bin" "$file" "$@")
  deltas+=("$(python3 -c "print(f'{(($hr - $he) / ($br - $be) - 1) * 100:.3f}')")")
  bases+=("$((br - be))"); heads+=("$((hr - he))")
done
mid=$(( (rounds + 1) / 2 ))
med() { printf '%s\n' "$@" | sort -g | sed -n "${mid}p"; }
echo "$label $(med "${deltas[@]}") $(med "${bases[@]}") $(med "${heads[@]}")"
