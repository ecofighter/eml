#!/usr/bin/env bash
# usage: gate-workers.sh BASE_BIN HEAD_BIN ROUNDS   (run from the repository root; a record, not a gate)
# Wall time (s) and peak RSS (MB) at 2 and 4 workers, median of ROUNDS, for programs on the changed paths.
set -euo pipefail
base_bin=${1:?BASE_BIN} head_bin=${2:?HEAD_BIN} rounds=${3:?ROUNDS}
one() { # BIN WORKERS FILE [ARG...]  ->  "real rss_bytes"
  local bin=$1 n=$2; shift 2
  { /usr/bin/time -l "$bin" run --workers "$n" "$@" >/dev/null; } 2>&1 |
    awk '/ real / {r=$1} /maximum resident set size/ {m=$1} END {print r, m}'
}
mid=$(( (rounds + 1) / 2 ))
med() { printf '%s\n' "$@" | sort -g | sed -n "${mid}p"; }
row() { # LABEL FILE [ARG...]
  local label=$1; shift
  for n in 2 4; do
    for which in base head; do
      local bin=$base_bin; [ "$which" = head ] && bin=$head_bin
      local reals=() rss=()
      for _ in $(seq "$rounds"); do
        read -r r m < <(one "$bin" "$n" "$@"); reals+=("$r"); rss+=("$m")
      done
      echo "$label N=$n $which real=$(med "${reals[@]}") rss_mb=$(( $(med "${rss[@]}") / 1048576 ))"
    done
  done
}
row pfib30 bench/pfib.em big
row par_tree bench/par_tree.em
row spread_8_30 bench/async/spread.em 8 30
row spread_100000_1 bench/async/spread.em 100000 1
row chan_pingpong bench/async/chan_pingpong.em
