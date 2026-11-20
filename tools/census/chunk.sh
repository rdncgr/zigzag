#!/bin/bash
# One chunk of census.sh: OUT CHUNK [VAR=VALUE ...]. Killed past 2 GB.
here=$(cd "$(dirname "$0")" && pwd)
out=$1; c=$2; shift 2
k=$(basename "$c")
"$here/capped.sh" 2048 env "$@" ZIGZAG_FACES="$(paste -sd ';' "$c")" ZIGZAG_LETTERS="$out/out$k.tsv" \
  "$out/bin" --ignored letters_in_faces_of_your_own --nocapture > "$out/log$k.txt" 2>&1 || {
  echo "chunk $k failed or was killed: $(paste -sd ';' "$c")" >&2
  # (So the run isn't taken for a whole one: see try.sh.)
  touch "$out/failed"
}
