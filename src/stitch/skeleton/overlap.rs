//! Overlapping strokes: satin columns for a letter read from its
//! skeleton as whole strokes laid over each other, as a hand digitiser
//! sews one, instead of the pieces the shape is cut into where strokes
//! meet ([`super::columns`]). A stem runs from end to end, and a bowl, an
//! arm or a serif that butts into it runs on under it to the middle of
//! the join, sewn first.
//!
//! Asked for with a satin's `overlap` setting. Measured over the installed
//! fonts' letters (2026-11-18), it leaves more of them clean than the cut
//! columns do (81% against 74%), at about a sixth more trims. Where
//! it can't read a letter's strokes, or its columns would leave cloth bare
//! or stitches off the shape, the letter is cut into columns as usual.

use super::*;

/// The shape's strokes as whole columns that overlap where
/// they meet, as a hand digitiser sews a letter: a stem runs from end to
/// end, and a bowl, an arm or a serif that butts into it runs on under it
/// to the middle of the join, sewn first. Each column is built along its
/// stroke's centreline, its stitches square to it: across each point of
/// the centreline, out to the outline either side, as far as the stroke is
/// wide there (not on across a join into the stroke beyond).
///
/// `None` if the shape's strokes can't be read, or a column's stitches
/// would leave it.
pub(in crate::stitch) fn overlapping(comp: &[Polyline]) -> Option<Vec<Rails>> {
    let (outline, graph) = skeleton_of(comp)?;
    let how = Reading {
        square: true,
        strokes: true,
        ..Default::default()
    };
    let (fate, cuts, _, _) = resolve(&graph, &outline, how)?;
    let strokes = strokes(&graph, &fate);
    if strokes.is_empty() {
        return None;
    }
    // (A stroke that's all join, its own sides nowhere, is left to the
    // strokes it joins; whether they cover it is seen to below.)
    let columns: Vec<Rails> = strokes
        .iter()
        .flat_map(|s| at_bends(&outline, s))
        .filter_map(|s| stroke_rails(&outline, &cuts, &s))
        .collect();
    if columns.is_empty() {
        return None;
    }
    // (Not if they leave some of the shape bare, or are sewn well outside
    // it: a stroke the skeleton doesn't show as one, the stem of a round
    // a that its bowl runs right through.)
    // Held to what's asked of any letter's columns: no more than a
    // stitch or so astray, and no bare patch of cloth to speak of.
    let astray: usize = columns
        .iter()
        .map(|r| crate::stitch::columns::astray_in(&outline.rings, FillRule::EvenOdd, r))
        .sum();
    let bare = bare_inside(&outline.rings, &columns);
    (bare <= STROKES_BARE && astray <= STROKES_ASTRAY).then_some(columns)
}

/// The most of a shape (square units, a tenth of a millimetre a side)
/// its strokes may leave bare, well inside it.
const STROKES_BARE: usize = 30;
/// The most of their stitches that may leave it.
const STROKES_ASTRAY: usize = 1;
/// How far round a column (units) its stitches cover the cloth.
const BARE_REACH: f64 = 3.5;

/// How many unit squares well inside the shape (a stitch's width and
/// more from its edge) aren't within a stitch's width of any column.
fn bare_inside(rings: &[Polyline], columns: &[Rails]) -> usize {
    let all = rings
        .iter()
        .flatten()
        .chain(columns.iter().flat_map(|r| r.a.iter().chain(&r.b)));
    let (lo, hi) = all.fold(
        (
            P::new(f64::INFINITY, f64::INFINITY),
            P::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        ),
        |(lo, hi), p| {
            (
                P::new(lo.x.min(p.x), lo.y.min(p.y)),
                P::new(hi.x.max(p.x), hi.y.max(p.y)),
            )
        },
    );
    if !lo.x.is_finite() {
        return 0;
    }
    let (w, h) = (
        (hi.x - lo.x).ceil() as usize + 1,
        (hi.y - lo.y).ceil() as usize + 1,
    );
    let shape = raster(rings, lo, w, h);
    let mut sewn = vec![false; w * h];
    for r in columns {
        let band: Vec<Polyline> = if r.closed {
            vec![r.a.clone(), r.b.clone()]
        } else {
            let mut ring = r.a.clone();
            ring.extend(r.b.iter().rev());
            ring.push(r.a[0]);
            vec![ring]
        };
        for (c, on) in raster(&band, lo, w, h).into_iter().enumerate() {
            sewn[c] |= on;
        }
    }
    let at = |g: &[bool], x: i64, y: i64| {
        x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && g[y as usize * w + x as usize]
    };
    // (A stitch covers about a stitch's width either side of its line,
    // and is sewn a little wider than its column, for the pull.)
    let reach: Vec<(i64, i64)> = (-4..=4i64)
        .flat_map(|dy| (-4..=4i64).map(move |dx| (dx, dy)))
        .filter(|&(dx, dy)| ((dx * dx + dy * dy) as f64).sqrt() <= BARE_REACH)
        .collect();
    let mut bare = 0;
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let deep = [(0, 0), (3, 0), (-3, 0), (0, 3), (0, -3)]
                .iter()
                .all(|&(dx, dy)| at(&shape, x + dx, y + dy));
            let near = reach.iter().any(|&(dx, dy)| at(&sewn, x + dx, y + dy));
            bare += usize::from(deep && !near);
        }
    }
    bare
}

/// Which unit squares (from `lo`, `w` by `h`) have their centres inside
/// `rings`, even-odd.
fn raster(rings: &[Polyline], lo: P, w: usize, h: usize) -> Vec<bool> {
    let mut grid = vec![false; w * h];
    let mut xs: Vec<f64> = Vec::new();
    for row in 0..h {
        let y = lo.y + row as f64 + 0.5;
        xs.clear();
        for ring in rings {
            for e in ring.windows(2) {
                let (a, b) = (e[0], e[1]);
                if (a.y <= y) != (b.y <= y) {
                    xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
                }
            }
            // (Closing the ring, if it isn't.)
            if let (Some(&a), Some(&b)) = (ring.last(), ring.first())
                && a != b
                && (a.y <= y) != (b.y <= y)
            {
                xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
            }
        }
        xs.sort_by(f64::total_cmp);
        for pair in xs.chunks(2) {
            let [x0, x1] = pair else { continue };
            let from = (x0 - lo.x - 0.5).ceil().max(0.0) as usize;
            let to = ((x1 - lo.x - 0.5).floor() as i64).min(w as i64 - 1);
            for col in from as i64..=to {
                grid[row * w + col as usize] = true;
            }
        }
    }
    grid
}

/// A stroke turns a corner where its centreline turns more than this
/// (radians) over its width either side.
const BEND_SHARP: f64 = 0.8;

/// A corner turns at least this much (radians) within [`BEND_NEAR`]
/// times its half-width either side. A curve can't: its centreline can't
/// bend tighter than its half-width, or its inside would fold over, and
/// so turns at most `BEND_NEAR` radians that far.
const BEND_AT_ONCE: f64 = 0.65;
const BEND_NEAR: f64 = 0.7;

/// A stroke cut at its corners into strokes that turn none: where it
/// turns sharply (two strokes meeting at an angle that the skeleton reads
/// as one, the arm and the leg of a k), each runs on straight to the
/// outline at the corner, over the other, as a digitiser sews them. (Not
/// a ring: a bowl turns smoothly all round.)
fn at_bends(outline: &Outline, stroke: &Stroke) -> Vec<Stroke> {
    let line = &stroke.line;
    let total = polyline_length(line);
    let closed = line.len() > 3 && line[0].dist(*line.last().unwrap()) < 1.0;
    if closed || total < 8.0 {
        return vec![stroke.clone()];
    }
    let r = outline.distance(point_along(line, total * 0.5)).max(2.0);
    let step = 1.0;
    let n = (total / step) as usize;
    let at = |s: f64| point_along(line, s.clamp(0.0, total));
    // The turn at each point, over the stroke's width either side; if
    // most of it is over a fraction of that: a corner turns all at once,
    // a bowl's curve evenly all the way.
    let over = |s: f64, h: f64| {
        let (u, v) = ((at(s) - at(s - h)).norm(), (at(s + h) - at(s)).norm());
        cross(u, v).atan2(u.dot(v)).abs()
    };
    let turn: Vec<f64> = (0..=n)
        .map(|k| {
            let s = k as f64 * step;
            if s < 2.0 * r || total - s < 2.0 * r {
                return 0.0;
            }
            // (Its width there: a stroke can thin into a corner.)
            let r = outline.distance(at(s)).clamp(1.0, r);
            let (whole, near) = (over(s, 2.0 * r), over(s, BEND_NEAR * r));
            if near < BEND_AT_ONCE {
                return 0.0;
            }
            whole
        })
        .collect();
    // The sharpest point of each run that turns more than enough.
    let mut cuts: Vec<f64> = Vec::new();
    let mut k = 0;
    while k <= n {
        if turn[k] <= BEND_SHARP {
            k += 1;
            continue;
        }
        let start = k;
        while k <= n && turn[k] > BEND_SHARP {
            k += 1;
        }
        let best = (start..k)
            .max_by(|&a, &b| turn[a].total_cmp(&turn[b]))
            .unwrap();
        cuts.push(best as f64 * step);
    }
    if cuts.is_empty() {
        return vec![stroke.clone()];
    }
    let mut out = Vec::new();
    let mut from = 0.0;
    let mut first_end = stroke.ends[0];
    for (i, &c) in cuts.iter().chain(std::iter::once(&total)).enumerate() {
        let last = i == cuts.len();
        let piece: Vec<P> = {
            let mut v = vec![at(from)];
            let mut gone = 0.0;
            for w in line.windows(2) {
                let d = w[0].dist(w[1]);
                if gone + d > from && gone < c {
                    v.push(w[1]);
                }
                gone += d;
            }
            v.retain(|p| {
                let s = nearest_arc(line, *p);
                s > from && s < c
            });
            v.insert(0, at(from));
            v.push(at(c));
            v
        };
        // (Each end at a corner carried on straight, the way the piece ran
        // into it, to the outline at the corner: past the skeleton's
        // corner, which sits inside the bend by about the stroke's width.)
        let mut piece = piece;
        let carry = |piece: &mut Vec<P>| {
            let len = polyline_length(piece);
            let end = *piece.last().unwrap();
            let before = point_along(piece, (len - 2.0 * r).max(0.0));
            let dir = (end - before).norm();
            if dir.len() > 0.5
                && let Some(hit) = outline.hit(end, dir, 8.0 * r)
            {
                piece.push(end.lerp(hit.p, (1.0 - 0.5 / end.dist(hit.p).max(0.5)).max(0.0)));
            }
        };
        if !last {
            carry(&mut piece);
        }
        if i > 0 {
            piece.reverse();
            carry(&mut piece);
            piece.reverse();
        }
        out.push(Stroke {
            line: piece,
            ends: [first_end, if last { stroke.ends[1] } else { Fate::Tip }],
            forks: stroke.forks.clone(),
        });
        from = c;
        first_end = Fate::Tip;
    }
    out
}

/// How far along `line` the point of it nearest `p` is.
fn nearest_arc(line: &[P], p: P) -> f64 {
    let (mut best, mut at, mut gone) = (f64::INFINITY, 0.0, 0.0);
    for w in line.windows(2) {
        let (t, q) = nearest_on(w[0], w[1], p);
        let d = q.dist(p);
        if d < best {
            best = d;
            at = gone + t * w[0].dist(w[1]);
        }
        gone += w[0].dist(w[1]);
    }
    at
}

/// How far apart along a stroke's centreline [`stroke_rails`] sets its
/// stitches' directions (units).
const STROKE_STEP: f64 = 2.0;

/// How far either side of a join (times its half-width) the skeleton is
/// bent toward what meets it there.
const JOIN_REACH: f64 = 1.3;
/// `v` turned by `a` radians.
fn rotate(v: P, a: f64) -> P {
    let (s, c) = a.sin_cos();
    P::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// How far either side of a join (times the stroke's half-width) its
/// centreline isn't trusted to give the stroke's width.
const JOIN_WIDTH: f64 = 1.0;

/// The most a stroke is taken to be wider at an end than it mostly is.
const STROKE_WIDER: f64 = 1.8;

/// How far a stroke that ends in a join runs on under the stroke it
/// meets there (units).
const JOIN_LAP: f64 = 4.0;

/// A stroke's centreline resampled every unit and averaged over its
/// half-width either side (less toward an open end, whose point stays
/// where it is).
fn smoothed(line: &[P], closed: bool, outline: &Outline) -> Polyline {
    let total = polyline_length(line);
    let n = total.ceil().max(2.0) as usize;
    let at = |s: f64| point_along(line, s.clamp(0.0, total));
    let pts: Vec<P> = (0..=n).map(|k| at(k as f64 * total / n as f64)).collect();
    let mut half: Vec<f64> = pts
        .iter()
        .step_by(4)
        .map(|&p| outline.distance(p))
        .collect();
    half.sort_by(f64::total_cmp);
    let r = half.get(half.len() / 2).copied().unwrap_or(2.0);
    let w = (r.round() as usize).clamp(2, 40);
    let m = pts.len();
    (0..m)
        .map(|k| {
            let reach = if closed { w } else { w.min(k).min(m - 1 - k) };
            if reach == 0 {
                return pts[k];
            }
            let mut sum = P::new(0.0, 0.0);
            for d in 0..=2 * reach {
                let i = if closed {
                    (k + m + d - reach) % m
                } else {
                    k + d - reach
                };
                sum = sum + pts[i];
            }
            sum * (1.0 / (2 * reach + 1) as f64)
        })
        .collect()
}

/// How far off centre a stroke's centreline may be (the difference of its
/// two sides, over their sum) and still be the stroke's own.
const STROKE_CENTRED: f64 = 0.2;

/// How far from `p` along `dir` to the outline (infinitely, if not within
/// the widest column there is).
fn side_of(outline: &Outline, p: P, dir: P) -> f64 {
    outline
        .hit(p, dir, AUTO_MAX_WIDTH)
        .map_or(f64::INFINITY, |h| h.p.dist(p))
}

/// One stroke as a column along its centreline (see [`overlapping`]).
fn stroke_rails(outline: &Outline, cuts: &[Cut], stroke: &Stroke) -> Option<Rails> {
    let mut line = stroke.line.clone();
    if line.len() < 2 {
        return None;
    }
    let closed = line.len() > 3 && line[0].dist(*line.last().unwrap()) < 1.0;
    // Straight through every join it goes through: the skeleton bends
    // toward what meets it there, and the stroke doesn't.
    for &(fork, r) in &stroke.forks {
        let reach = JOIN_REACH * r.max(1.0);
        let near: Vec<usize> = (0..line.len())
            .filter(|&k| line[k].dist(fork) < reach)
            .collect();
        if let (Some(&first), Some(&last)) = (near.first(), near.last())
            && first > 0
            && last + 1 < line.len()
            && near.len() == last - first + 1
        {
            line.drain(first..=last);
        }
    }
    // And straight into a join it ends in, as far as the middle of it,
    // the way it came: under the stroke there. An end that runs on to the
    // outline (a tip, or past a corner) is carried on to it, if that's
    // near; else (the outline is across another stroke) it ends in the
    // join likewise.
    let mut laps: [Option<(P, P)>; 2] = [None, None];
    if !closed {
        for start in [true, false] {
            if start {
                line.reverse();
            }
            let fate = stroke.ends[if start { 0 } else { 1 }];
            let end = *line.last().unwrap();
            let r = outline.distance(end).max(1.0);
            let total = polyline_length(&line);
            let back = (JOIN_REACH * r).min(total * 0.4);
            let base = point_along(&line, total - back);
            let before = point_along(&line, (total - back - 2.0 * r).max(0.0));
            let dir = (base - before).norm();
            let outward = match fate {
                Fate::Reach { dir, .. } => Some(dir),
                Fate::Tip => Some((end - point_along(&line, (total - 4.0).max(0.0))).norm()),
                _ => None,
            };
            let hit = outward
                .filter(|d| d.len() > 0.5)
                .and_then(|d| outline.hit(end, d, 2.0 * JOIN_REACH * r));
            match hit {
                Some(hit) => {
                    // (Just short of it: a stitch's ends sit on the outline.)
                    let p = end.lerp(hit.p, (1.0 - 0.5 / end.dist(hit.p).max(0.5)).max(0.0));
                    line.push(p);
                }
                None if dir.len() > 0.5 && !matches!(fate, Fate::Tip) => {
                    // (Cut back to where it last ran straight, and on from
                    // there to beside the middle of the join.)
                    while line.len() > 2 && polyline_length(&line) > total - back {
                        line.pop();
                    }
                    line.push(base);
                    // (As far as the near side of the stroke it meets,
                    // half that stroke's width short of the middle of the
                    // join, and [`JOIN_LAP`] on under it.)
                    let on = (base.dist(end) - r + JOIN_LAP).max(JOIN_LAP);
                    let lap = base + dir * on;
                    line.push(lap);
                    // (And on to the outline, for a side of it that runs on
                    // along the shape's edge past the join: the outside of
                    // a corner two strokes meet at, an A's or a V's.)
                    // (However far: past the lap a side goes on only along the
                    // shape's edge, and a thin stroke can run a long way
                    // into the point of a V.)
                    // (Not a half of a stroke that crosses the one it
                    // butts into: past the lap is the other half.)
                    let crossing =
                        matches!(fate, Fate::Butt(c) if cuts.get(c).is_some_and(|c| c.crossing));
                    if !crossing && let Some(hit) = outline.hit(lap, dir, 8.0 * r + 40.0) {
                        line.push(lap.lerp(hit.p, (1.0 - 0.5 / lap.dist(hit.p).max(0.5)).max(0.0)));
                    }
                    laps[usize::from(!start)] = Some((lap, dir));
                }
                None => {}
            }
            if start {
                line.reverse();
            }
        }
    }
    // The centreline smoothed over about the stroke's width: the skeleton
    // wavers by a pixel, and bends at its ends toward the corners there,
    // and stitches square to it would fan.
    let line = smoothed(&line, closed, outline);
    let total = polyline_length(&line);
    if total < 2.0 * STROKE_STEP {
        return None;
    }
    let n = (total / STROKE_STEP).ceil() as usize;
    let at = |s: f64| -> P {
        if closed {
            point_along(&line, s.rem_euclid(total))
        } else {
            point_along(&line, s.clamp(0.0, total))
        }
    };
    // Across each point: how far to the outline either side.
    let mut samples: Vec<(P, P, f64, f64)> = Vec::with_capacity(n + 1);
    let last = if closed { n } else { n + 1 };
    for k in 0..last {
        let s = k as f64 * total / n as f64;
        let p = at(s);
        let h = STROKE_STEP * 2.0;
        let t = (at(s + h) - at(s - h)).norm();
        if t.len() < 0.5 {
            return None;
        }
        let nrm = t.perp();
        samples.push((p, nrm, side_of(outline, p, nrm), side_of(outline, p, -nrm)));
    }
    // Where the centreline runs centred between the stroke's two sides,
    // it's the stroke's own; elsewhere (through a join, or into one) it's
    // the join's, off to one side, and a side runs on into the stroke
    // beyond. There the stroke goes on straight, as wide as it was: from
    // the stroke either side of a join it goes through, or on from the
    // stroke before one it ends in.
    let m = samples.len();
    let clean: Vec<bool> = samples
        .iter()
        .map(|&(_, _, da, db)| {
            da.is_finite() && db.is_finite() && (da - db).abs() <= STROKE_CENTRED * (da + db) + 1.0
        })
        .collect();
    if !clean.iter().any(|&c| c) {
        return None;
    }
    // (Nor at an end where it's far wider than it mostly is: the stroke
    // run into a serif or a bar across its end, whose ends are as far
    // either side. Not along the way: a bowl swells.)
    let clean: Vec<bool> = {
        let width: Vec<f64> = samples.iter().map(|s| s.2 + s.3).collect();
        let mut widths: Vec<f64> = width
            .iter()
            .zip(&clean)
            .filter(|(_, c)| **c)
            .map(|(w, _)| *w)
            .collect();
        widths.sort_by(f64::total_cmp);
        let usual = widths[widths.len() / 2];
        let mut clean = clean;
        if !closed {
            let wide = |k: usize, clean: &[bool]| !clean[k] || width[k] > STROKE_WIDER * usual;
            for order in [(0..m).collect::<Vec<_>>(), (0..m).rev().collect()] {
                for k in order {
                    if !wide(k, &clean) {
                        break;
                    }
                    clean[k] = false;
                }
            }
        }
        // (Nor that much wider in a join it goes through, centred or not:
        // a crossbar can stick out as far either side, and isn't the
        // stroke's width. Its width there is the stroke's either side, as
        // its course is: see above.)
        for (k, sample) in samples.iter().enumerate() {
            let in_join = stroke
                .forks
                .iter()
                .any(|&(fork, r)| sample.0.dist(fork) < JOIN_REACH * r.max(1.0));
            if in_join && width[k] > STROKE_WIDER * usual {
                clean[k] = false;
            }
        }
        clean
    };
    if !clean.iter().any(|&c| c) {
        return None;
    }
    // (And about a stroke's width either side of where it isn't, its
    // width there: by a join the centreline can be centred between the
    // stroke's side and the notch where the other stroke leaves it, the
    // stroke not yet as wide as it is.)
    let clean: Vec<bool> = {
        let step = total / n as f64;
        (0..m)
            .map(|k| {
                if !clean[k] {
                    return false;
                }
                let reach =
                    ((samples[k].2 + samples[k].3) * 0.5 * JOIN_WIDTH / step).round() as usize;
                let (lo, hi) = (k.saturating_sub(reach), (k + reach + 1).min(m));
                (lo..hi).all(|i| clean[i])
            })
            .collect()
    };
    if !clean.iter().any(|&c| c) {
        return None;
    }
    let mut spine: Vec<(P, P, f64, f64)> = samples.clone();
    // (Which samples are carried on to an end of the stroke that's the
    // shape's own, a tip or past a corner: those are cut off where the
    // outline is.)
    let mut free = vec![false; m];
    let free_end = |at_end: bool| -> bool {
        !closed
            && matches!(
                stroke.ends[usize::from(at_end)],
                Fate::Tip | Fate::Reach { .. }
            )
    };
    let mut k = 0;
    while k < m {
        if clean[k] {
            k += 1;
            continue;
        }
        let first = k;
        while k < m && !clean[k] {
            k += 1;
        }
        let end = k; // first clean after, or m
        match (first.checked_sub(1), (end < m).then_some(end)) {
            // Between two clean stretches: across on the course the
            // stroke runs either side, straight if it's straight, curving
            // if it curves, from the one's width to the other's.
            (Some(i), Some(j)) => {
                let (pi, _, ai, bi) = samples[i];
                let (pj, _, aj, bj) = samples[j];
                let chord = pi.dist(pj);
                let course = |from: usize, to: usize| (samples[to].0 - samples[from].0).norm();
                let span = |k: usize| {
                    ((samples[k].2 + samples[k].3) / STROKE_STEP)
                        .round()
                        .max(2.0) as usize
                };
                let ti = course(i.saturating_sub(span(i)), i);
                let tj = course(j, (j + span(j)).min(m - 1));
                let (ti, tj) = if ti.len() < 0.5 || tj.len() < 0.5 {
                    ((pj - pi).norm(), (pj - pi).norm())
                } else {
                    (ti, tj)
                };
                for q in first..end {
                    let t = (q - i) as f64 / (j - i) as f64;
                    let (t2, t3) = (t * t, t * t * t);
                    let p = pi * (2.0 * t3 - 3.0 * t2 + 1.0)
                        + ti * (chord * (t3 - 2.0 * t2 + t))
                        + pj * (-2.0 * t3 + 3.0 * t2)
                        + tj * (chord * (t3 - t2));
                    let d = pi * (6.0 * t2 - 6.0 * t)
                        + ti * (chord * (3.0 * t2 - 4.0 * t + 1.0))
                        + pj * (-6.0 * t2 + 6.0 * t)
                        + tj * (chord * (3.0 * t2 - 2.0 * t));
                    let nrm = d.norm().perp();
                    let nrm = if nrm.dot(samples[q].1) < 0.0 {
                        -nrm
                    } else {
                        nrm
                    };
                    spine[q] = (p, nrm, ai + (aj - ai) * t, bi + (bj - bi) * t);
                }
            }
            // At an end: on from the clean stretch before it, as it runs
            // there, straight or curving, as wide as it is there.
            (before, after) => {
                let toward_end = after.is_none();
                let anchor = if toward_end { before? } else { after? };
                // (Its course over the stroke's width or so, and how fast
                // that turns over the width before.)
                let span = ((samples[anchor].2 + samples[anchor].3) / STROKE_STEP)
                    .round()
                    .max(2.0) as usize;
                let back = |k: usize, by: usize| -> Option<usize> {
                    if toward_end {
                        k.checked_sub(by)
                    } else {
                        (k + by < m).then_some(k + by)
                    }
                };
                let other = back(anchor, span).unwrap_or(if toward_end { 0 } else { m - 1 });
                let (pa, nrm_a, aa, ba) = samples[anchor];
                let dir = (pa - samples[other].0).norm();
                if dir.len() < 0.5 {
                    continue;
                }
                let step = total / n as f64;
                let bend = match back(anchor, 2 * span) {
                    Some(far) if (far.min(other)..=far.max(other)).all(|k| clean[k]) => {
                        let before = (samples[other].0 - samples[far].0).norm();
                        let turn = cross(before, dir).atan2(before.dot(dir));
                        let most = 1.0 / (aa.max(ba) + 1.0);
                        (turn / (span as f64 * step)).clamp(-most, most)
                    }
                    _ => 0.0,
                };
                let side = if dir.perp().dot(nrm_a) < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                let (mut p, mut gone) = (pa, 0.0);
                let order: Vec<usize> = if toward_end {
                    (first..end).collect()
                } else {
                    (first..end).rev().collect()
                };
                for q in order {
                    let along = rotate(dir, bend * (gone + step * 0.5));
                    p = p + along * step;
                    gone += step;
                    let course = rotate(dir, bend * gone);
                    spine[q] = (p, course.perp() * side, aa, ba);
                    free[q] = free_end(toward_end);
                }
            }
        }
    }
    // Each side as far as the stroke is wide there, or (carried on to an
    // end of the shape's own) to the outline if that's nearer: the stroke
    // cut off by the shape's end. And none past where it leaves the
    // shape.
    // Past the lap at an end in a join, a side goes on only where it's
    // the shape's edge (as far from the centreline as it was, or nearer,
    // the edge cutting across), and the stroke as far as either does.
    let sides: Vec<(f64, f64)> = spine
        .iter()
        .map(|&(p, nrm, _, _)| (side_of(outline, p, nrm), side_of(outline, p, -nrm)))
        .collect();
    let edge = |w: f64, d: f64| d.is_finite() && d <= w + 0.25 * w + 1.0;
    let mut past = vec![false; m];
    let mut gone = vec![false; m];
    for (e, lap) in laps.iter().enumerate() {
        let Some((lap, dir)) = *lap else { continue };
        // (The run of samples at that end past the lap, from the lap out.)
        let beyond = |k: usize| (spine[k].0 - lap).dot(dir) > 0.0;
        let order: Vec<usize> = if e == 1 {
            let first = (0..m).rev().take_while(|&k| beyond(k)).last().unwrap_or(m);
            (first..m).collect()
        } else {
            let last = (0..m).take_while(|&k| beyond(k)).count();
            (0..last).rev().collect()
        };
        let (mut stop, mut joined) = (false, false);
        for k in order {
            let (_, _, wa, wb) = spine[k];
            past[k] = true;
            // (And not on into a stroke that's its own again, both its
            // sides the shape's edge alike, once through the join: the
            // stem of a t beyond its bar is the stem's own. Short of the
            // join, it's still this stroke's.)
            let (sa, sb) = sides[k];
            // (A point the sides close in to evenly isn't one.)
            let own = sa.is_finite()
                && sb.is_finite()
                && (sa - sb).abs() <= STROKE_CENTRED * (sa + sb) + 1.0
                && sa + sb >= 0.75 * (wa + wb);
            joined |= !own;
            if stop || (own && joined) || !(edge(wa, sa) || edge(wb, sb)) {
                stop = true;
                gone[k] = true;
            }
        }
    }
    let mut a: Polyline = Vec::with_capacity(m);
    let mut b: Polyline = Vec::with_capacity(m);
    // (Which of them are rungs, the stitches square across there.)
    let mut rung: Vec<bool> = Vec::with_capacity(m);
    let mut kept = 0;
    for k in 0..m {
        let (p, nrm, wa, wb) = spine[k];
        if gone[k] || (!clean[k] && !point_inside(&outline.rings, FillRule::EvenOdd, p)) {
            continue;
        }
        let (da, db) = if past[k] {
            (
                if edge(wa, sides[k].0) {
                    wa.min(sides[k].0)
                } else {
                    wa
                },
                if edge(wb, sides[k].1) {
                    wb.min(sides[k].1)
                } else {
                    wb
                },
            )
        } else if clean[k] {
            (
                wa.min(side_of(outline, p, nrm)),
                wb.min(side_of(outline, p, -nrm)),
            )
        } else {
            // Through a join or on to an end: as wide as the stroke; cut
            // off by the outline at an end of the shape's own. (Not wider
            // into a bracket: its rails cut across the corners beside it,
            // and over every installed font that left fewer letters clean,
            // with more trims.)
            let width = |w: f64, d: f64| if free[k] { d.min(w) } else { w };
            (
                width(wa, side_of(outline, p, nrm)),
                width(wb, side_of(outline, p, -nrm)),
            )
        };
        // (Cut short by the shape's own end, at a slant: not a rung. The
        // rails run on along the outline to the end, and the stitches
        // turn from the last rung to lie along it, as a hand digitiser
        // ends a stroke cut at an angle; square across to where the
        // outline cuts them, they'd end in steps.)
        let cut_short = free[k] && (da < wa - 0.5 || db < wb - 0.5);
        // (A sample with no width — no outline either side of it, and no
        // clean stretch to take one from — is no rung.)
        if !(da + db >= 0.5)
            || ![p.x, p.y, nrm.x, nrm.y, da, db]
                .iter()
                .all(|v| v.is_finite())
        {
            continue;
        }
        a.push(p + nrm * da);
        b.push(p - nrm * db);
        rung.push(!cut_short);
        kept += 1;
    }
    if kept < 2 {
        return None;
    }
    let mut sync = Vec::new();
    let (mut la, mut lb) = (0.0, 0.0);
    for k in 1..kept {
        la += a[k - 1].dist(a[k]);
        lb += b[k - 1].dist(b[k]);
        if k + 1 < kept && rung[k] {
            sync.push((la, lb));
        }
    }
    if closed {
        // (Each sample's rung kept round a ring too: a ring that crosses
        // itself, an 8's, is sewn square to its centreline all round.)
        sync.push((la, lb));
        a.push(a[0]);
        b.push(b[0]);
    }
    Some(Rails { a, b, closed, sync })
}
