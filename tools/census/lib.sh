# Shared by census.sh and try.sh (sourced): building the census, setting
# up a run, and running the chunks of any number of runs on one pool of
# workers, one per core.
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../.." && pwd)
root=$repo/target/census

# The library's test binary, built with the `trial` profile (as fast as
# release, but an edit rebuilds in seconds), into $bin; and the installed
# families (ZigZag's own font list, target/census/families.txt, made the
# first time: delete it after installing fonts).
build() {
  local log
  log=$(cd "$repo" && cargo test --profile trial --lib --no-run 2>&1) || { echo "$log" >&2; exit 1; }
  bin=$(echo "$log" | sed -n 's/.*Executable unittests src\/lib.rs (\(.*\))$/\1/p')
  case "$bin" in /*) ;; *) bin=$repo/$bin ;; esac
  "$bin" --list >/dev/null 2>&1 || { echo "no runnable test binary: $bin" >&2; exit 1; }
  mkdir -p "$root"
  if [ ! -s "$root/families.txt" ]; then
    "$bin" --ignored installed_font_families --nocapture 2>/dev/null \
      | sed -n 's/^FAMILY\t//p' > "$root/families.txt"
  fi
}

# prepare OUT MODE [VAR=VALUE ...]: a fresh run in OUT (popular: the fonts
# in fonts.txt that are installed; quick: every 8th installed family;
# full: all of them), its chunks' jobs added to $jobs. The binary is
# copied, so rebuilding during a run is safe.
prepare() {
  local out=$1 mode=$2 per
  shift 2
  rm -rf "$out"; mkdir -p "$out/chunks"
  cp "$bin" "$out/bin"
  case "$mode" in
    popular)
      grep -v '^#' "$here/fonts.txt" | sed '/^$/d' | while IFS= read -r face; do
        if grep -qxF "${face%%:*}" "$root/families.txt"; then echo "$face"; else echo "not installed: $face" >&2; fi
      done > "$out/faces.txt"
      per=2 ;;
    quick) sed '/^$/d' "$root/families.txt" | awk 'NR % 8 == 1' > "$out/faces.txt"; per=5 ;;
    full) sed '/^$/d' "$root/families.txt" > "$out/faces.txt"; per=5 ;;
    *) echo "unknown mode: $mode" >&2; exit 2 ;;
  esac
  split -l "$per" "$out/faces.txt" "$out/chunks/c"
  # (No blank at a line's end: xargs -L reads that as the line going on.)
  for c in "$out"/chunks/c*; do echo "$out $c${*:+ $*}" >> "$jobs"; done
}

# Every job in $jobs, on one pool; then each run's binary removed.
run_jobs() {
  xargs -P "$(sysctl -n hw.ncpu 2>/dev/null || nproc)" -L 1 "$here/chunk.sh" < "$jobs"
  cut -d' ' -f1 "$jobs" | sort -u | while read -r out; do rm -f "$out/bin"; done
}
