#!/usr/bin/env python3
"""The letters judged by eye (judged.txt here) beside what the census
measured on them: whether its faults agree with the eye.

  tools/census/judge.py STYLE=DIR ...   e.g.
  tools/census/judge.py satin_mitred=target/census/try/try-15 satin_cut=DIR

Each DIR a census of the popular fonts at the judged size, sewn in that
style (try.sh's try-15 with ZIGZAG_MITRED=1 for satin_mitred, or with
ZIGZAG_CUT=1 for satin_cut).
"""
import glob, os, sys

here = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, here)
from summary import faults  # noqa: E402

NAMES = ('columns fills pointed astray trims travel lean skewed bare off_edge '
         'seams layers outside crowded').split()


def rows(d):
    out = {}
    for f in glob.glob(os.path.join(d, 'out*.tsv')):
        for line in open(f):
            p = line.rstrip('\n').split('\t')
            out[p[0]] = [int(x) for x in p[1:15]] + [0] * (14 - len(p[1:15]))
    return out


def main():
    dirs = dict(a.split('=', 1) for a in sys.argv[1:])
    if not dirs:
        sys.exit(__doc__)
    measured = {style: rows(d) for style, d in dirs.items()}
    agree = total = 0
    for line in open(os.path.join(here, 'judged.txt')):
        if line.startswith('#') or not line.strip():
            continue
        family, ch, _mm, style, verdict, *where = line.rstrip('\n').split('\t')
        v = measured.get(style, {}).get(f'{family} {ch}')
        if v is None:
            print(f'{family} {ch} {style}: not in the census')
            continue
        found = faults(v)
        ok = (verdict == 'clean') == (not found)
        total += 1
        agree += ok
        shown = ' '.join(f'{n}={v[k]}' for k, n in enumerate(NAMES) if n in
                         ('skewed', 'bare', 'astray', 'seams', 'outside', 'crowded'))
        print(f'{"ok " if ok else "NO "} {family} {ch} {style}: eye {verdict}, census '
              f'{",".join(found) or "clean"}  ({shown})')
    print(f'{agree} of {total} agree')


main()
