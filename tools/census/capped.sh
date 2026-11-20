#!/bin/bash
# capped.sh MB COMMAND...: run COMMAND, killing it if its memory goes over
# MB megabytes (macOS ignores `ulimit -v`, so a runaway census process can
# otherwise take the whole machine). Exits 137 if it had to kill it.
cap_kb=$(( $1 * 1024 )); shift
"$@" &
pid=$!
while kill -0 "$pid" 2>/dev/null; do
  rss=$(ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ')
  if [ -n "$rss" ] && [ "$rss" -gt "$cap_kb" ]; then
    kill -9 "$pid" 2>/dev/null
    echo "capped.sh: killed $1 at $(( rss / 1024 )) MB (cap $(( cap_kb / 1024 )) MB)" >&2
    wait "$pid" 2>/dev/null
    exit 137
  fi
  sleep 0.2
done
wait "$pid"
