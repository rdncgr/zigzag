#!/usr/bin/env python3
"""Summarise census folders (made by census.sh): the share of letters clean
on every measure, the faults by kind, trims and letters sewn in one piece.

    tools/census/summary.py target/census/NAME [target/census/OTHER ...]
    tools/census/summary.py --table DIR ...   (a line each; against DIR.before
                                               where there is one)

A letter's row (out*.tsv): columns, fills, pointed ends, stitches astray,
trims, travel in sight, lean, skewed stitches, bare cloth, stitches off the
edge, seams, layers, outside, crowded (see `Lettered` in
src/design.rs). Families that came
out as another's fallback, or with few letters, are left out."""
import glob, os, re, sys
from collections import Counter, defaultdict


def faults(v):
    out = []
    if v[1] > 0: out.append('filled')
    if v[3] > 1: out.append('astray')
    if v[5] > 0: out.append('travel')
    if v[8] > 30: out.append('bare')
    if v[7] >= 6 or v[6] >= 25: out.append('skewed')
    if v[10] >= 3: out.append('seams')
    if v[11] > 30: out.append('layers')
    return out


def summarise(d):
    rows = {}
    for f in sorted(glob.glob(d + '/out*.tsv')):
        for line in open(f):
            p = line.rstrip('\n').split('\t')
            rows[p[0]] = [int(x) for x in p[1:15]] + [0] * (14 - len(p[1:15]))
    fam = defaultdict(dict)
    for name, v in rows.items():
        family, ch = name.rsplit(' ', 1)
        fam[family][ch] = v
    seen, kept = set(), {}
    for family in sorted(fam):
        sig = tuple(sorted((ch, tuple(v)) for ch, v in fam[family].items()))
        if sum(1 for v in fam[family].values() if v[0] + v[1] > 0) < 30 or sig in seen:
            continue
        seen.add(sig)
        kept[family] = fam[family]
    tot = clean = 0
    kinds = Counter()
    for letters in kept.values():
        for v in letters.values():
            if v[0] + v[1] == 0:
                continue
            tot += 1
            fs = faults(v)
            clean += not fs
            kinds.update(fs)
    letters = trims = broken = 0
    for f in glob.glob(d + '/log*.txt'):
        for line in open(f):
            m = re.match(r'(\d+) letters: .* (\d+) trims', line)
            if m:
                letters += int(m.group(1)); trims += int(m.group(2))
            m = re.match(r'(\d+) letters break the thread', line)
            if m:
                broken += int(m.group(1))
    return {
        'families': len(kept), 'letters': tot,
        'clean': 100.0 * clean / max(tot, 1),
        'kinds': {k: 100.0 * n / max(tot, 1) for k, n in kinds.items()},
        'counts': kinds, 'clean_n': clean,
        'trims': trims, 'one': 100.0 * (letters - broken) / max(letters, 1),
    }


def report(d):
    r = summarise(d)
    print('%s: %d families, %d letters' % (os.path.basename(d), r['families'], r['letters']))
    print('  clean on every measure: %d (%.1f%%)' % (r['clean_n'], r['clean']))
    for k, n in r['counts'].most_common():
        print('  %-8s %5d letters (%.1f%%)' % (k, n, r['kinds'][k]))
    print('  %d trims, %.1f%% of letters in one piece' % (r['trims'], r['one']))


def table(dirs):
    cols = [('clean', '%.1f%%', 1), ('seams', '%.1f%%', -1), ('skewed', '%.1f%%', -1),
            ('bare', '%.1f%%', -1), ('trims', '%d', -1), ('one', '%.1f%%', 1)]
    get = lambda r, k: r[k] if k in ('clean', 'trims', 'one') else r['kinds'].get(k, 0.0)
    print('%-8s' % '' + ''.join('%19s' % (k if k != 'one' else 'one piece') for k, _, _ in cols))
    for d in dirs:
        r = summarise(d)
        b = summarise(d + '.before') if os.path.isdir(d + '.before') else None
        cells = []
        for k, f, good in cols:
            v = get(r, k)
            cell = f % v
            diff = v - get(b, k) if b is not None else 0.0
            shown = f.replace('%%', '') % abs(diff)
            if float(shown) > 0:
                cell += ' (%s%s %s)' % ('+' if diff > 0 else '-', shown, 'ok' if diff * good > 0 else '!!')
            cells.append(cell)
        print('%-8s' % os.path.basename(d) + ''.join('%19s' % c for c in cells))


if __name__ == '__main__':
    args = sys.argv[1:]
    if args[:1] == ['--table']:
        table([d.rstrip('/') for d in args[1:]])
    else:
        for d in args or ['.']:
            report(d.rstrip('/'))
