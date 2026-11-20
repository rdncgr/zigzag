#!/bin/bash
# The letter census: every letter and digit of a set of fonts set as a
# satin and planned, each counted clean or faulty (see
# `letters_in_faces_of_your_own` in src/design.rs and summary.py here).
#
#   tools/census/census.sh NAME [popular|quick|full] [VAR=VALUE ...]
#
# popular (the default) takes the fonts in fonts.txt here, popular faces
# of every kind of type, in about half a minute. quick takes every 8th
# font family installed, full all of them (much longer; for confirming).
# VAR=VALUE settings go to the test: ZIGZAG_FONT_SIZE=11 sets letters
# about 8 mm tall (21, about 15 mm, otherwise), ZIGZAG_OVERLAP=1 sews them
# with overlapping strokes. Results go to target/census/NAME/; compare runs
# with `tools/census/summary.py target/census/A target/census/B`.
#
# For trying a change, try.sh runs the usual comparison in one go.
#
# Installed families are ZigZag's own font list (target/census/
# families.txt, made the first time; delete it after installing fonts).
# The work runs in small chunks, one process per core, each killed if it
# goes over 2 GB (capped.sh). Run one census at a time.
set -euo pipefail
. "$(cd "$(dirname "$0")" && pwd)/lib.sh"
[ $# -ge 1 ] || { sed -n '2,21p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
name=$1; shift
mode=popular
case "${1:-}" in popular|quick|full) mode=$1; shift ;; esac
build
out=$root/$name
jobs=$(mktemp); trap 'rm -f "$jobs"' EXIT
prepare "$out" "$mode" "$@"
start=$(date +%s)
run_jobs
[ -e "$out/failed" ] && echo "$name: some chunks failed (see above); the counts are short" >&2
echo "$name ($mode${*:+, $*}): $(( ($(date +%s) - start) / 60 )) min $(( ($(date +%s) - start) % 60 )) s"
python3 "$here/summary.py" "$out"
