#!/bin/bash
# Try a change: the letter census of the popular fonts at 8 and 15 mm,
# sewn as ZigZag sews them (sewn-8, sewn-15) and with the settings given
# (try-8, try-15), each against the last try.
#
#   tools/census/try.sh [VAR=VALUE ...]   e.g. tools/census/try.sh ZIGZAG_CUT=1
#
# (ZIGZAG_AUTO=1: letters with cut=0, mitred where that sews cleanly;
# ZIGZAG_MITRED=1: every corner mitred; letters are cut into columns with
# their corners squared otherwise, the default; ZIGZAG_OVERLAP=1: strokes
# laid over each other.)
#
# One build, every run's chunks on one pool. sewn-* is kept from the last
# time the source was the same (by a hash of it), so trying settings alone
# runs only try-*. With no settings, only sewn-*. Results in
# target/census/try/ (the last try's beside them, as NAME.before).
set -euo pipefail
. "$(cd "$(dirname "$0")" && pwd)/lib.sh"
build
dir=$root/try
mkdir -p "$dir" "$root/base"
key=$(cd "$repo" && {
  git ls-files -co --exclude-standard src | sort | xargs cat
  cat Cargo.toml Cargo.lock tools/census/fonts.txt
} | shasum | cut -c1-12)
jobs=$(mktemp); trap 'rm -f "$jobs"' EXIT
names=()
for mm in 8 15; do
  size=$([ $mm = 8 ] && echo 11 || echo 21)
  # (The last try's, kept to compare with.)
  for name in sewn-$mm try-$mm; do
    if [ -e "$dir/$name" ] || [ -L "$dir/$name" ]; then
      rm -rf "$dir/$name.before"; mv "$dir/$name" "$dir/$name.before"
    fi
  done
  base=$root/base/$key-$mm
  if [ ! -s "$base/done" ]; then
    rm -rf "$base"; prepare "$base" popular ZIGZAG_FONT_SIZE=$size
  fi
  ln -s "$base" "$dir/sewn-$mm"
  names+=("$dir/sewn-$mm")
  if [ $# -gt 0 ]; then
    prepare "$dir/try-$mm" popular ZIGZAG_FONT_SIZE=$size "$@"
    names+=("$dir/try-$mm")
  fi
done
start=$(date +%s)
[ -s "$jobs" ] && run_jobs
# (A baseline only kept whole: one with a chunk that failed is run again.)
for mm in 8 15; do
  b=$root/base/$key-$mm
  if [ -e "$b/failed" ]; then
    echo "sewn-$mm: a chunk failed; not kept for next time" >&2
  else
    echo ok > "$b/done"
  fi
done
# (Old kept runs: only the last few.)
ls -dt "$root"/base/*-8 2>/dev/null | tail -n +7 | sed 's/-8$//' | while read -r k; do rm -rf "$k-8" "$k-15"; done
echo "try (${*:-as sewn}): $(( $(date +%s) - start )) s"
python3 "$here/summary.py" --table "${names[@]}"
