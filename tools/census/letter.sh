#!/bin/bash
# One letter, set as a satin and planned as ZigZag sews it and with the
# settings given, each with its trims and why (ZIGZAG_TRIMS: see
# `trim_report` in src/design.rs). For looking into a letter the census
# counts against.
#
#   tools/census/letter.sh FAMILY CHAR [8|15] [VAR=VALUE ...]
#   e.g. tools/census/letter.sh "Times New Roman" g 8 ZIGZAG_CUT=1
#
# (FAMILY:bold for a bold face, as in fonts.txt.) The stitches go to
# target/census/letters/, as SVG to open in ZigZag or a browser.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
[ $# -ge 2 ] || { sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
family=$1; char=$2; shift 2
mm=15
case "${1:-}" in 8|15) mm=$1; shift ;; esac
size=$([ "$mm" = 8 ] && echo 11 || echo 21)
weight=normal
case "$family" in *:bold) family=${family%:bold}; weight=bold ;; esac
log=$(cd "$repo" && cargo build --profile trial --bin zigzag 2>&1) || {
  echo "$log" | grep -E '^error' -A8 >&2
  exit 1
}
out=$repo/target/census/letters
mkdir -p "$out"
tag=$(printf '%s-%s-%s-%s' "$family" "$weight" "$char" "$mm" | tr -c 'A-Za-z0-9-' '_')
# The letter as a satin, with the settings the census's ZIGZAG_CUT,
# ZIGZAG_MITRED and ZIGZAG_OVERLAP name in its id (as `letters_in_faces_of_your_own` does).
letter() {
  local id=satin
  case " $* " in *" ZIGZAG_CUT=1 "*) id=${id}_cut ;; esac
  case " $* " in *" ZIGZAG_MITRED=1 "*) id=${id}_mitred ;; esac
  case " $* " in *" ZIGZAG_AUTO=1 "*) id=${id}_cut=0 ;; esac
  case " $* " in *" ZIGZAG_OVERLAP=1 "*) id=${id}_overlap ;; esac
  printf '<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="40mm" viewBox="0 0 40 40"><text id="%s" x="6" y="28" font-family="%s" font-weight="%s" font-size="%s" fill="#000000">%s</text></svg>' \
    "$id" "$family" "$weight" "$size" "$char"
}
show() {
  local name=$1; shift
  echo "== $name"
  letter "$@" > "$out/$tag-$name-in.svg"
  env "$@" ZIGZAG_TRIMS=1 "$repo/target/trial/zigzag" convert "$out/$tag-$name-in.svg" --out "$out/$tag-$name.svg" 2>&1 \
    | grep -v -E '^(wrote|loaded|  color)'
}
show sewn
[ $# -gt 0 ] && show tried "$@"
echo "(stitches: $out/$tag-*.svg)"
