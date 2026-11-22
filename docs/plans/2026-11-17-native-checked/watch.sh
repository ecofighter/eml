#!/usr/bin/env bash
# usage: watch.sh LIMIT_KB CMD...   runs CMD under timeout 20 and kills it (and its children) above LIMIT_KB of RSS
limit=$1; shift
timeout 20 "$@" & P=$!
killed=0
while kill -0 $P 2>/dev/null; do
  for c in $P $(pgrep -P $P); do
    r=$(ps -o rss= -p $c 2>/dev/null | tr -d ' ')
    if [ -n "$r" ] && [ "$r" -gt "$limit" ]; then kill -9 $c; killed=1; echo "WATCHDOG: killed pid $c at ${r}KB"; fi
  done
  sleep 0.1
done
wait $P; rc=$?
echo "exit=$rc killed=$killed"
