#!/usr/bin/env python3
"""Sort a census's trims by where the plan and the stitches part ways.

    ZIGZAG_TRIMS=1 must be among the census's settings, e.g.
    tools/census/census.sh trims-8 ZIGZAG_FONT_SIZE=11 ZIGZAG_TRIMS=1
    tools/census/trims.py target/census/trims-8 [-v]

Each letter's report (see `trim_report` in src/design.rs) says how many cuts
`through` expected after cutting its columns at joins, how many the order
search counted, how many its order took tried out as it would be sewn, and
the trims sewn. So a trim is:

  expected         the order taken, tried out, has it;
  order mismatch   sewn, though the order taken tried out with fewer (or,
                   with no order tried out, the search expected fewer);
  no order search  in a letter whose pieces weren't searched (a ring among
                   them, or too many);
and, of the cuts the order tried out with,
  search missed    more than the search counted (found by trying it out);
  split mismatch   more than `through` expected (it judged the joins from
                   where the rails end, not the stitches).
-v lists the letters of each kind."""
import glob, re, sys
from collections import defaultdict

d = sys.argv[1].rstrip('/')
verbose = '-v' in sys.argv
letters = {}
cur = None
for f in sorted(glob.glob(d + '/log*.txt')):
    for line in open(f, errors='replace'):
        if line.startswith('LETTER '):
            cur = line[7:].strip()
            letters[cur] = {'through': None, 'order': None, 'tried': None, 'sewn': 0, 'ring': False, 'many': False}
            continue
        if cur is None:
            continue
        r = letters[cur]
        m = re.match(r'through: \d+ columns, expected (\d+) cuts whole; (?:cut at \d+ joins, expecting (\d+)|no join)', line)
        if m:
            r['through'] = (r['through'] or 0) + int(m.group(2) if m.group(2) is not None else m.group(1))
        elif line.startswith('through:') and 'a ring' in line:
            r['ring'] = True
        elif line.startswith('through:') and 'too many' in line:
            r['many'] = True
        m = re.match(r'column order: \d+ columns, expected (\d+) cuts(?:; tried out, (\d+))?', line)
        if m:
            r['order'] = (r['order'] or 0) + int(m.group(1))
            if m.group(2) is not None:
                r['tried'] = (r['tried'] or 0) + int(m.group(2))
                r['found'] = r.get('found', 0) + max(0, int(m.group(2)) - int(m.group(1)))
        if line.startswith('trim before'):
            r['sewn'] += 1

kinds = defaultdict(list)
total = 0
for name, r in letters.items():
    sewn = r['sewn']
    total += sewn
    if r['order'] is None:
        if sewn:
            kinds['no order search'].append((name, sewn))
        continue
    order = r['tried'] if r['tried'] is not None else r['order']
    expected = min(sewn, order)
    if expected:
        kinds['expected'].append((name, expected))
    if sewn > order:
        kinds['order mismatch'].append((name, sewn - order))
    if r.get('found'):
        kinds['search missed'].append((name, r['found']))
    if r['through'] is not None and order > r['through']:
        kinds['split mismatch'].append((name, order - r['through']))
    if sewn and r['ring']:
        kinds['  (a ring: not cut at joins)'].append((name, sewn))
    if sewn and r['many']:
        kinds['  (too many columns to cut)'].append((name, sewn))

print('%s: %d letters, %d trims' % (d.split('/')[-1], len(letters), total))
for k in ['expected', 'order mismatch', 'no order search', 'search missed', 'split mismatch',
          '  (a ring: not cut at joins)', '  (too many columns to cut)']:
    v = kinds.get(k, [])
    print('  %-30s %4d trims in %d letters' % (k, sum(n for _, n in v), len(v)))
    if verbose and v:
        print('      ' + ', '.join('%s%s' % (n, '' if c == 1 else ' ×%d' % c) for n, c in sorted(v)))
