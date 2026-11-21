#!/usr/bin/env bash
# usage: run.sh LOG CMD...  runs CMD under watch.sh (2 GB cap, timeout 20) and samples the eml process RSS into LOG
S=$(cd "$(dirname "$0")" && pwd)
log=$1; shift
: > "$log"
"$S/watch.sh" 2097152 "$@" & W=$!
p=
while [ -z "$p" ] && kill -0 $W 2>/dev/null; do p=$(pgrep -x eml); done
while kill -0 $W 2>/dev/null; do
  r=$(ps -o rss= -p $p 2>/dev/null | tr -d ' '); [ -n "$r" ] && echo "$r" >> "$log"
  sleep 0.05
done
wait $W
sort -n "$log" | tail -1 | awk '{print "peak_rss_kb=" $1}'
awk '{a[NR]=$1} END {n=NR; printf "samples=%d series_kb:", n; step=int(n/12); if(step<1)step=1; for(i=1;i<=n;i+=step) printf " %d", a[i]; printf " (last %d)\n", a[n]}' "$log"
