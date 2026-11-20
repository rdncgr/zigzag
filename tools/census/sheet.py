#!/usr/bin/env python3
"""Letters set as satins, each sewn in a few styles side by side, on one
sheet to look at: for judging by eye what the census's measures should
see (judged.txt here).

  tools/census/sheet.py OUT.png [8|15] FAMILY=CHARS ... [--styles satin,satin_cut]
  e.g. tools/census/sheet.py n.png 15 Helvetica=NVW "Arial:bold=NVW"

FAMILY:bold for a bold face, as in fonts.txt; 15 mm tall unless 8. A style is the satin's id,
its settings: satin (as ZigZag sews it), satin_cut (the `cut` setting),
and so on; STYLE@VAR=VALUE+VAR=VALUE sets those in the planner's
environment too. Each letter is drawn as its stitches, each a thread's width in the order
sewn (so underlay and laps under satin are hidden, as on the cloth;
SHEET_THIN=1 draws every stitch as a thin line instead, to see what's
underneath), under its outline in red (a running stitch round it:
stitches beyond it are outside the letter).
Needs rsvg-convert (Homebrew librsvg).
"""
import os, re, subprocess, sys

here = os.path.dirname(os.path.abspath(__file__))
repo = os.path.abspath(os.path.join(here, '..', '..'))
THREAD = 0.36  # mm: a thread's width, as it's drawn
LINE = 0.08  # mm: each stitch a line, with SHEET_THIN=1
THIN = bool(os.environ.get('SHEET_THIN'))
PER_ROW = int(os.environ.get('SHEET_ACROSS', 6))  # letters across a sheet


def build():
    log = subprocess.run(['cargo', 'build', '--profile', 'trial', '--bin', 'zigzag'],
                         cwd=repo, capture_output=True, text=True)
    if log.returncode:
        sys.exit(log.stderr)
    return os.path.join(repo, 'target', 'trial', 'zigzag')


def sew(zz, family, weight, ch, size, style, work):
    tag = re.sub(r'[^A-Za-z0-9]', '_', f'{family}-{weight}-{ord(ch)}-{size}-{style}')
    # (STYLE@VAR=VALUE,...: settings for the planner, for trying a change.)
    style, _, env = style.partition('@')
    env = dict(os.environ, **dict(kv.split('=', 1) for kv in env.split('+') if kv))
    src, out = os.path.join(work, tag + '-in.svg'), os.path.join(work, tag + '.svg')
    esc = {'<': '&lt;', '>': '&gt;', '&': '&amp;'}.get(ch, ch)
    text = lambda id_, fill: (f'<text id="{id_}" x="6" y="28" font-family="{family}" font-weight="{weight}" '
                              f'font-size="{size}" fill="{fill}">{esc}</text>')
    open(src, 'w').write('<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="40mm" viewBox="0 0 40 40">'
                         + text(style, '#000000') + text('run', '#ff0000') + '</svg>')
    r = subprocess.run([zz, 'convert', src, '--out', out], capture_output=True, text=True, env=env)
    if r.returncode:
        return None
    lines = []
    for m in re.finditer(r'<polyline[^>]*stroke="(#[0-9a-f]{6})"[^>]*points="([^"]*)"', open(out).read()):
        pts = [tuple(map(float, p.split(','))) for p in m.group(2).split()]
        lines.append((m.group(1), pts))
    return lines


def main():
    args = sys.argv[1:]
    styles = ['satin', 'satin_cut']
    if '--styles' in args:
        k = args.index('--styles')
        styles = args[k + 1].split(',')
        del args[k:k + 2]
    mm = '15'
    if len(args) > 1 and args[1] in ('8', '15'):
        mm = args.pop(1)
    if len(args) < 2 or any('=' not in a for a in args[1:]):
        sys.exit(__doc__)
    out = args[0]
    size = 11 if mm == '8' else 21
    global CELL
    CELL = 12 if mm == '8' else 22  # mm each letter is drawn in
    letters = []
    for item in args[1:]:
        family, chars = item.split('=', 1)
        weight = 'normal'
        if family.endswith(':bold'):
            family, weight = family[:-5], 'bold'
        letters += [(family, weight, ch) for ch in chars]
    zz = build()
    work = os.path.join(repo, 'target', 'census', 'sheets')
    os.makedirs(work, exist_ok=True)
    body = []
    # Each letter a block, its styles one under another; blocks in rows.
    across = min(PER_ROW, len(letters))
    block = CELL * len(styles) + 3
    for n, (family, weight, ch) in enumerate(letters):
        bx, by = (n % across) * CELL, (n // across) * block
        body.append(f'<text x="{bx + 1}" y="{by + 2.2}" font-size="{CELL / 15:.2f}" font-family="Helvetica">'
                    f'{family}{" bold" if weight == "bold" else ""} {ch} ({mm} mm)</text>')
        for k, style in enumerate(styles):
            x0, y0 = bx, by + 3 + k * CELL
            body.append(f'<text x="{x0 + 1}" y="{y0 + 1.5}" font-size="{CELL / 18:.2f}" fill="#666" '
                        f'font-family="Helvetica">{style}</text>')
            lines = sew(zz, family, weight, ch, size, style, work)
            if not lines:
                body.append(f'<text x="{x0 + 2}" y="{y0 + 10}" font-size="3">failed</text>')
                continue
            xs = [p[0] for _, pts in lines for p in pts]
            ys = [p[1] for _, pts in lines for p in pts]
            cx, cy = (min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2
            dx, dy = x0 + CELL / 2 - cx, y0 + CELL / 2 + 0.5 - cy
            # (The outline over the stitches, to see past it.)
            for colour, pts in sorted(lines, key=lambda l: l[0] == '#ff0000'):
                pts = [(x + dx, y + dy) for x, y in pts]
                if colour == '#ff0000' or THIN:
                    width = (0.15 if colour == '#ff0000' else LINE) * CELL / 22
                    body.append('<polyline fill="none" points="%s" stroke="%s" stroke-linejoin="round" '
                                'stroke-width="%.3f"/>' % (' '.join('%.2f,%.2f' % p for p in pts), colour, width))
                    continue
                # Thread: each stitch in the order sewn, a thread's width,
                # edged, so what's sewn later lies over what's under it.
                for (px, py), (qx, qy) in zip(pts, pts[1:]):
                    for stroke, width in (('#333', THREAD), ('#d8d2c4', THREAD - 0.1)):
                        body.append(f'<line x1="{px:.2f}" y1="{py:.2f}" x2="{qx:.2f}" y2="{qy:.2f}" '
                                    f'stroke="{stroke}" stroke-width="{width}" stroke-linecap="round"/>')
    w, h = CELL * across, block * ((len(letters) + across - 1) // across)
    svg = os.path.splitext(out)[0] + '.svg'
    px = 440 / CELL  # (each letter as big at either size)
    open(svg, 'w').write(f'<svg xmlns="http://www.w3.org/2000/svg" width="{w * px:.0f}" height="{h * px:.0f}" '
                         f'viewBox="0 0 {w} {h}"><rect width="{w}" height="{h}" fill="#f4f1ea"/>'
                         + ''.join(body) + '</svg>')
    subprocess.run(['rsvg-convert', svg, '-o', out], check=True)
    print(out)

main()
