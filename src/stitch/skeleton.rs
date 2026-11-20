//! Satin columns for a filled shape whose strokes run into each other
//! without a sharp corner between them: a serif letter, its stem flowing
//! into its serifs round a bracket, its bowl into its stem.
//!
//! The shape is thinned to its skeleton (the line down the middle of every
//! stroke), and the skeleton says what the strokes are: where it forks,
//! strokes meet. At a fork the two branches that run straightest into each
//! other are one stroke going through, and any other butts into its side:
//! a stem into the slab serif across its foot, an arm or a bowl into the
//! stem. A stem much thicker than what meets it at its head (a flag serif
//! one side, an arm or a bowl the other) runs on to the outline instead,
//! and both butt into its sides. Where one stroke turns sharply into
//! another (a flag serif off the top of a stem, the two strokes of a V)
//! the thicker runs on to the outline and the thinner butts into it.
//!
//! The shape is then cut where strokes butt: across the stroke's mouth,
//! between the two places the outline comes nearest the fork either side
//! of it (the corners of the mouth, or the middle of each bracket), or
//! from the corner between two strokes straight on along the side of the
//! one that runs on. Each piece is one stroke, and its rails are its own
//! outline between its two ends, so the columns cover the shape exactly. A
//! stroke going through takes the cut across the other's mouth as part of
//! its side.
//!
//! The skeleton also keeps each column's stitches square to it: rungs are
//! set across it from the centreline, so a stroke that runs straight and
//! then curls doesn't lean all along for the difference in its rails.
//!
//! Anything this can't make sense of (pieces and strokes that don't match
//! up, a column whose stitches would leave the shape or cross) is `None`,
//! and the caller does without.

mod overlap;
pub(super) use overlap::overlapping;

use super::columns::{area, ray_on_line, sewable, valid};
use super::satin::{AUTO_MAX_WIDTH, Rails, auto_rails, lean, pairs};
use crate::geom::{FillRule, P, Polyline, cross, point_inside, polyline_length};

/// Pixels across a stroke of the shape's usual thickness, and the range a
/// pixel's size (units) is kept in.
const ACROSS: f64 = 14.0;
const PIXEL: (f64, f64) = (0.3, 2.0);
/// A stroke is at least this many pixels from its middle to its edge, or
/// the shape is thinned again with smaller pixels.
const HAIRLINE: f64 = 2.5;
/// The most pixels the shape's box is given.
const MAX_PIXELS: f64 = 4.0e6;
/// The most lines a skeleton may have before it's pruned.
const TANGLE: usize = 1500;
/// How many times the centreline is smoothed (the pixels' staircase).
const SMOOTH: usize = 12;
/// A branch that sticks out of its fork by less than this many times the
/// stroke's half-width there is a corner of the stroke, not a stroke.
const SPUR: f64 = 0.8;
/// …or by less than this (units), whatever the stroke.
const SPUR_MIN: f64 = 5.0;
/// Forks nearer each other than this many half-widths are one fork.
const MERGE: f64 = 0.8;
/// Two strokes that meet at a fork more than this far (radians) from
/// straight on aren't one stroke going through it.
const CROOKED: f64 = 0.7;
/// A centreline turning more than this (radians) within [`REACH`] times
/// the stroke's half-width either way is two strokes meeting at a corner
/// (a V's foot, rounded as it may be), not one stroke curving.
const BEND: f64 = 1.1;
const REACH: f64 = 2.0;
/// Where a stroke butts into the end of another that turns a corner into
/// it, the cut is made square across it if that's within this of the way
/// the other's side runs on (cosine: 40°).
const SQUARE_BUTT: f64 = 0.766;
/// …unless the columns that leaves lean more than this many times what
/// they would otherwise, and this many degrees.
const SQUARE_LEAN: (f64, f64) = (1.25, 1.0);
/// A cut along a stroke's side ends in the corner of a notch within this
/// many of the stroke's half-widths of where it lands, one that turns at
/// least this much (radians).
const NOTCH: f64 = 0.6;
const NOTCH_TURN: f64 = 0.6;
/// How far along a column's side, either way from its corner, a nick may
/// reach (units, the least first): one bridged if the column can't be
/// made otherwise. (No further: the crotch between the arches of an m is
/// the same shape, larger, and no nick.)
const NICK: [f64; 5] = [4.0, 8.0, 12.0, 16.0, 20.0];
/// A stroke runs straight whose centreline bows less than this, of its
/// length: one cut square across where it butts.
const STRAIGHT: f64 = 0.08;
/// How far a stroke's middle line is from the outline, over how far back
/// from its end that is, where the end is a blunt one and not a point.
const BLUNT: f64 = 0.5;
/// A point this near the outline is on it, units.
const ON_OUTLINE: f64 = 1e-3;
/// An end found for a column lies along the column's stroke, and so is
/// its side and no end, within this of the stroke's direction (cosine of
/// the angle: 25°).
const ALONG: f64 = 0.9;
/// A column's end runs across it: the outline either side of its tip, as
/// far as it stays within this of square to the column (cosine of the
/// angle from the column's own direction: 45°).
const CAP: f64 = 0.707;
/// How far a round end may stand out past a column's last stitch, units.
const BULGE: f64 = 1.5;
/// Stitches cross when they do so this far (units) from the ends of both.
const CROSS: f64 = 2.0;
/// How many of a column's stitches may pass a notch in its side (where a
/// ball joins its stroke, or a hairline serif was left on it).
const NOTCHED: usize = 3;
/// However thin a stroke, its stitches may be this long (units) where it
/// swells.
const SWELL: f64 = 20.0;
/// …or this, if it can't be made a column otherwise.
const SWELL_MOST: f64 = 32.0;
/// How far apart rungs are set, units.
const RUNG: f64 = 5.0;

/// The shape's outline: its outer ring first, the material on the left of
/// every ring.
struct Outline {
    rings: Vec<Polyline>,
}

/// A place on the outline: `t` of the way along the edge from point `edge`
/// of ring `ring`.
#[derive(Clone, Copy, Debug)]
struct Spot {
    ring: usize,
    edge: usize,
    t: f64,
    p: P,
}

/// The nearest point to `p` on the segment from `a` to `b`, and how far
/// along it that is.
pub(super) fn nearest_on(a: P, b: P, p: P) -> (f64, P) {
    let e = b - a;
    let len2 = e.dot(e);
    let t = if len2 > 0.0 {
        ((p - a).dot(e) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (t, a.lerp(b, t))
}

/// Whether direction `v` lies in the wedge swept from `lo` round to `hi`
/// (the way [`cross`] counts as positive).
fn within(lo: P, hi: P, v: P) -> bool {
    if cross(lo, hi) >= 0.0 {
        cross(lo, v) >= -1e-9 && cross(v, hi) >= -1e-9
    } else {
        !(cross(hi, v) > 1e-9 && cross(v, lo) > 1e-9)
    }
}

/// The angle between two directions, radians (0 to π).
fn angle(a: P, b: P) -> f64 {
    cross(a, b).abs().atan2(a.dot(b))
}

/// The point `d` along `line` from its start (its end, if it's shorter).
fn point_along(line: &[P], d: f64) -> P {
    let mut left = d;
    for w in line.windows(2) {
        let seg = w[0].dist(w[1]);
        if seg >= left && seg > 0.0 {
            return w[0].lerp(w[1], left / seg);
        }
        left -= seg;
    }
    *line.last().unwrap()
}

impl Outline {
    fn new(comp: &[Polyline]) -> Option<Outline> {
        let mut rings = Vec::new();
        for (k, ring) in comp.iter().enumerate() {
            let mut p: Polyline = Vec::with_capacity(ring.len());
            for &q in ring {
                if p.last().is_none_or(|l| l.dist(q) > 1e-6) {
                    p.push(q);
                }
            }
            while p.len() > 1 && p[0].dist(*p.last().unwrap()) <= 1e-6 {
                p.pop();
            }
            if p.len() < 3 {
                if k == 0 {
                    return None;
                }
                continue;
            }
            if (area(&p) > 0.0) != (k == 0) {
                p.reverse();
            }
            rings.push(p);
        }
        Some(Outline { rings })
    }

    fn edges(&self) -> impl Iterator<Item = (usize, usize, P, P)> + '_ {
        self.rings.iter().enumerate().flat_map(|(k, r)| {
            let n = r.len();
            (0..n).map(move |i| (k, i, r[i], r[(i + 1) % n]))
        })
    }

    /// How far `p` is from the outline: half the stroke's width, for a
    /// point on its centreline.
    fn distance(&self, p: P) -> f64 {
        self.edges()
            .map(|(_, _, a, b)| nearest_on(a, b, p).1.dist(p))
            .fold(f64::INFINITY, f64::min)
    }

    /// The nearest place on the outline to `from` among those lying in
    /// the wedge from direction `lo` round to `hi`.
    fn nearest_between(&self, from: P, lo: P, hi: P) -> Option<Spot> {
        let mut best: Option<(f64, Spot)> = None;
        for (ring, edge, a, b) in self.edges() {
            let e = b - a;
            // The nearest point of the part of the edge inside the wedge
            // is the nearest of the whole edge, an end of it, or where a
            // side of the wedge crosses it.
            let mut ts = [nearest_on(a, b, from).0, 0.0, 1.0, -1.0, -1.0];
            for (k, side) in [lo, hi].into_iter().enumerate() {
                let den = cross(side, e);
                if den.abs() > 1e-12 {
                    let w = a - from;
                    let s = cross(w, e) / den;
                    let t = cross(w, side) / den;
                    if s > 0.0 && (0.0..=1.0).contains(&t) {
                        ts[3 + k] = t;
                    }
                }
            }
            for t in ts {
                if t < 0.0 {
                    continue;
                }
                let p = a.lerp(b, t);
                let d = p.dist(from);
                if d > 1e-9 && within(lo, hi, p - from) && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, Spot { ring, edge, t, p }));
                }
            }
        }
        best.map(|(_, s)| s)
    }

    /// The first place a ray from `from` along `dir` meets the outline
    /// (not at `from` itself), within `reach`.
    /// The sharpest corner within `reach` of `p` at which the outline
    /// turns in on the material (a notch), if any turns as much as a
    /// corner does.
    fn inside_corner(&self, p: P, reach: f64) -> Option<Spot> {
        self.inside_corner_of(p, reach, None)
    }

    /// [`Outline::inside_corner`], of one ring only if `of` says which.
    fn inside_corner_of(&self, p: P, reach: f64, of: Option<usize>) -> Option<Spot> {
        let mut best: Option<(f64, Spot)> = None;
        for (k, ring) in self.rings.iter().enumerate() {
            if of.is_some_and(|o| o != k) {
                continue;
            }
            let n = ring.len();
            for i in 0..n {
                if ring[i].dist(p) > reach {
                    continue;
                }
                let (before, after) = (
                    (ring[i] - ring[(i + n - 1) % n]).norm(),
                    (ring[(i + 1) % n] - ring[i]).norm(),
                );
                // (The material on the left: a turn to the right.)
                let turn = -cross(before, after).atan2(before.dot(after));
                if turn >= NOTCH_TURN && best.as_ref().is_none_or(|b| turn > b.0) {
                    best = Some((
                        turn,
                        Spot {
                            ring: k,
                            edge: i,
                            t: 0.0,
                            p: ring[i],
                        },
                    ));
                }
            }
        }
        best.map(|b| b.1)
    }

    fn hit(&self, from: P, dir: P, reach: f64) -> Option<Spot> {
        let mut best: Option<(f64, Spot)> = None;
        for (ring, edge, a, b) in self.edges() {
            let e = b - a;
            let den = cross(dir, e);
            if den.abs() < 1e-12 {
                continue;
            }
            let w = a - from;
            let s = cross(w, e) / den;
            let t = cross(w, dir) / den;
            if s > 1e-3 && s <= reach && (-1e-9..=1.0 + 1e-9).contains(&t) {
                let t = t.clamp(0.0, 1.0);
                if best.is_none_or(|(bs, _)| s < bs) {
                    let p = a.lerp(b, t);
                    best = Some((s, Spot { ring, edge, t, p }));
                }
            }
        }
        best.map(|(_, s)| s)
    }

    /// Whether the straight line from `a` to `b` stays in the material,
    /// touching the outline only at its ends.
    fn clear(&self, a: P, b: P) -> bool {
        let len = a.dist(b);
        if len < 0.5 {
            return false;
        }
        let dir = (b - a).norm();
        if self
            .hit(a, dir, len - 1e-3)
            .is_some_and(|h| h.p.dist(b) > 1e-2 && h.p.dist(a) > 1e-2)
        {
            return false;
        }
        // (In it, and not along its edge: a line that lies on the outline
        // is in or out by how its points happen to round, and a cut
        // along the outline cuts nothing off.)
        [0.25, 0.5, 0.75].iter().all(|&t| {
            let p = a.lerp(b, t);
            point_inside(&self.rings, FillRule::EvenOdd, p) && self.distance(p) > ON_OUTLINE
        })
    }

    /// The point `d` along the outline from `at`, forward or back.
    fn walk(&self, at: Spot, d: f64, forward: bool) -> P {
        let r = &self.rings[at.ring];
        let n = r.len();
        let mut from = at.p;
        let mut k = if forward { (at.edge + 1) % n } else { at.edge };
        let mut left = d;
        for _ in 0..=n {
            let seg = from.dist(r[k]);
            if seg >= left && seg > 0.0 {
                return from.lerp(r[k], left / seg);
            }
            left -= seg;
            from = r[k];
            k = if forward {
                (k + 1) % n
            } else {
                (k + n - 1) % n
            };
        }
        from
    }
}

/// The shape as pixels: `on[j * w + i]` for the pixel whose middle is
/// `origin + (i + ½, j + ½) · px`. (A border of empty pixels all round.)
struct Grid {
    w: usize,
    px: f64,
    origin: P,
    on: Vec<bool>,
}

impl Grid {
    fn new(outline: &Outline, px: f64) -> Grid {
        let mut b = crate::geom::Bounds::empty();
        for r in &outline.rings {
            r.iter().for_each(|&p| b.add(p));
        }
        let origin = b.min - P::new(2.0 * px, 2.0 * px);
        let w = (b.width() / px).ceil() as usize + 5;
        let h = (b.height() / px).ceil() as usize + 5;
        let mut on = vec![false; w * h];
        let mut xs: Vec<f64> = Vec::new();
        for j in 0..h {
            let y = origin.y + (j as f64 + 0.5) * px;
            xs.clear();
            for (_, _, p, q) in outline.edges() {
                if (p.y <= y) != (q.y <= y) {
                    xs.push(p.x + (y - p.y) / (q.y - p.y) * (q.x - p.x));
                }
            }
            xs.sort_by(f64::total_cmp);
            for pair in xs.as_chunks::<2>().0 {
                let i0 = ((pair[0] - origin.x) / px - 0.5).ceil().max(0.0) as usize;
                let i1 = ((pair[1] - origin.x) / px - 0.5).floor();
                if i1 < 0.0 {
                    continue;
                }
                for i in i0..=(i1 as usize).min(w - 1) {
                    on[j * w + i] = true;
                }
            }
        }
        let mut grid = Grid { w, px, origin, on };
        grid.fill_pockets(outline);
        grid
    }

    /// A notch that narrows to nothing is pinched off, where it's thinner
    /// than a pixel, into pockets of empty pixels that look like holes in
    /// the shape: filled in, all but the largest empty patch inside each
    /// of the shape's real holes.
    fn fill_pockets(&mut self, outline: &Outline) {
        let w = self.w as isize;
        let n = self.on.len();
        // Which empty patch each empty pixel is part of (1: the outside).
        let mut patch = vec![0u32; n];
        let mut sizes: Vec<usize> = vec![0, 0];
        let mut stack: Vec<usize> = Vec::new();
        for start in 0..n {
            if self.on[start] || patch[start] != 0 {
                continue;
            }
            let id = if start == 0 { 1 } else { sizes.len() as u32 };
            if start != 0 {
                sizes.push(0);
            }
            patch[start] = id;
            stack.push(start);
            while let Some(k) = stack.pop() {
                sizes[id as usize] += 1;
                for d in [-w, 1, w, -1] {
                    let q = k as isize + d;
                    // (Off the row's end is the next row's start: the
                    // border is empty all round, so that's the outside
                    // meeting itself.)
                    if q < 0 || q as usize >= n {
                        continue;
                    }
                    let q = q as usize;
                    if !self.on[q] && patch[q] == 0 {
                        patch[q] = id;
                        stack.push(q);
                    }
                }
            }
        }
        if sizes.len() <= 2 {
            return;
        }
        // The largest patch inside each hole stays.
        let mut keep = vec![false; sizes.len()];
        keep[1] = true;
        let mut first = vec![usize::MAX; sizes.len()];
        for (k, &id) in patch.iter().enumerate() {
            let id = id as usize;
            if id > 1 && first[id] == usize::MAX {
                first[id] = k;
            }
        }
        for hole in &outline.rings[1..] {
            let inside = (2..sizes.len())
                .filter(|&id| {
                    point_inside(
                        std::slice::from_ref(hole),
                        FillRule::EvenOdd,
                        self.at(first[id]),
                    )
                })
                .max_by_key(|&id| sizes[id]);
            if let Some(id) = inside {
                keep[id] = true;
            }
        }
        for k in 0..n {
            if !self.on[k] && !keep[patch[k] as usize] {
                self.on[k] = true;
            }
        }
    }

    fn at(&self, k: usize) -> P {
        let (i, j) = (k % self.w, k / self.w);
        self.origin + P::new((i as f64 + 0.5) * self.px, (j as f64 + 0.5) * self.px)
    }

    /// The eight pixels round pixel `k`, in order round it (the first
    /// beside it, then cornerwise, and so on).
    fn around(&self, k: usize) -> [usize; 8] {
        let w = self.w as isize;
        [-w, -w + 1, 1, w + 1, w, w - 1, -1, -w - 1].map(|d| (k as isize + d) as usize)
    }

    /// Thin the shape to lines one pixel wide down the middle of its
    /// strokes, keeping it in one piece with the holes it had
    /// (Zhang and Suen's thinning).
    fn thin(&mut self) {
        let mut live: Vec<usize> = (0..self.on.len()).filter(|&k| self.on[k]).collect();
        loop {
            let mut any = false;
            for pass in 0..2 {
                let gone: Vec<usize> = live
                    .iter()
                    .copied()
                    .filter(|&k| {
                        if !self.on[k] {
                            return false;
                        }
                        let n = self.around(k).map(|q| self.on[q]);
                        let count = n.iter().filter(|&&v| v).count();
                        if !(2..=6).contains(&count) {
                            return false;
                        }
                        let steps = (0..8).filter(|&i| !n[i] && n[(i + 1) % 8]).count();
                        if steps != 1 {
                            return false;
                        }
                        if pass == 0 {
                            !(n[0] && n[2] && n[4]) && !(n[2] && n[4] && n[6])
                        } else {
                            !(n[0] && n[2] && n[6]) && !(n[0] && n[4] && n[6])
                        }
                    })
                    .collect();
                any |= !gone.is_empty();
                for k in gone {
                    self.on[k] = false;
                }
            }
            live.retain(|&k| self.on[k]);
            if !any {
                break;
            }
        }
        // That leaves steps two pixels thick here and there: off with any
        // pixel the line holds together without (and that isn't its end).
        loop {
            let mut any = false;
            for &k in &live {
                if !self.on[k] {
                    continue;
                }
                let n = self.around(k).map(|q| self.on[q]);
                if n.iter().filter(|&&v| v).count() < 2 {
                    continue;
                }
                let links = [0, 2, 4, 6]
                    .into_iter()
                    .filter(|&i| !n[i] && (n[i + 1] || n[(i + 2) % 8]))
                    .count();
                if links == 1 {
                    self.on[k] = false;
                    any = true;
                }
            }
            live.retain(|&k| self.on[k]);
            if !any {
                break;
            }
        }
    }
}

/// A place the skeleton ends, forks or (once found) turns a corner.
#[derive(Clone, Debug)]
struct Node {
    p: P,
    /// Half the stroke's width here.
    r: f64,
    bend: bool,
    alive: bool,
}

/// A line of the skeleton between two nodes, from `ends[0]` to `ends[1]`.
#[derive(Clone, Debug)]
struct Edge {
    ends: [usize; 2],
    line: Vec<P>,
    alive: bool,
}

struct Graph {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

impl Graph {
    /// The skeleton's lines and the places they meet.
    fn new(grid: &Grid, outline: &Outline) -> Option<Graph> {
        let on = &grid.on;
        let count = |k: usize| grid.around(k).iter().filter(|&&q| on[q]).count();
        // Which node each pixel is part of (0: none; else its number + 1):
        // an end by itself, the pixels of a fork together.
        let mut label = vec![0usize; on.len()];
        let mut nodes: Vec<Node> = Vec::new();
        for k in 0..on.len() {
            if !on[k] || label[k] != 0 {
                continue;
            }
            let c = count(k);
            if c == 2 || c == 0 {
                continue;
            }
            let id = nodes.len() + 1;
            let mut members = vec![k];
            label[k] = id;
            if c >= 3 {
                let mut next = 0;
                while next < members.len() {
                    for q in grid.around(members[next]) {
                        if on[q] && label[q] == 0 && count(q) >= 3 {
                            label[q] = id;
                            members.push(q);
                        }
                    }
                    next += 1;
                }
            }
            let sum = members
                .iter()
                .fold(P::new(0.0, 0.0), |s, &m| s + grid.at(m));
            let p = sum * (1.0 / members.len() as f64);
            nodes.push(Node {
                p,
                r: outline.distance(p),
                bend: false,
                alive: true,
            });
        }
        if nodes.is_empty() {
            return None;
        }
        let mut edges: Vec<Edge> = Vec::new();
        let mut walked = vec![false; on.len()];
        for k in 0..on.len() {
            if label[k] == 0 {
                continue;
            }
            for first in grid.around(k) {
                if !on[first] || walked[first] {
                    continue;
                }
                if label[first] != 0 {
                    // Two nodes side by side: a line with nothing between.
                    if label[first] > label[k] {
                        edges.push(Edge {
                            ends: [label[k] - 1, label[first] - 1],
                            line: vec![nodes[label[k] - 1].p, nodes[label[first] - 1].p],
                            alive: true,
                        });
                    }
                    continue;
                }
                let mut line = vec![nodes[label[k] - 1].p];
                let (mut before, mut at) = (k, first);
                let end = loop {
                    walked[at] = true;
                    line.push(grid.at(at));
                    let next = grid
                        .around(at)
                        .into_iter()
                        .filter(|&q| on[q] && q != before)
                        .find(|&q| label[q] != 0 || !walked[q]);
                    match next {
                        Some(q) if label[q] != 0 => break Some(label[q] - 1),
                        Some(q) => (before, at) = (at, q),
                        None => break None,
                    }
                };
                let Some(end) = end else { continue };
                line.push(nodes[end].p);
                edges.push(Edge {
                    ends: [label[k] - 1, end],
                    line,
                    alive: true,
                });
            }
        }
        // (Two nodes side by side were found from each: once is enough.)
        let mut g = Graph { nodes, edges };
        for e in &mut g.edges {
            smooth(&mut e.line);
        }
        Some(g)
    }

    /// The lines that leave node `v`: which, and by which of their ends.
    fn arms(&self, v: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (k, e) in self.edges.iter().enumerate() {
            if !e.alive {
                continue;
            }
            for end in 0..2 {
                if e.ends[end] == v {
                    out.push((k, end));
                }
            }
        }
        out
    }

    /// Line `e` as it runs away from its end `end`.
    fn arm(&self, (e, end): (usize, usize)) -> Vec<P> {
        let mut line = self.edges[e].line.clone();
        if end == 1 {
            line.reverse();
        }
        line
    }

    /// Whether two of the lines that leave node `v` meet again without
    /// coming back through it: the two halves of a bowl.
    fn looped(&self, v: usize, a: (usize, usize), b: (usize, usize)) -> bool {
        if a.0 == b.0 {
            return true;
        }
        let far = |(e, end): (usize, usize)| self.edges[e].ends[1 - end];
        let (from, to) = (far(a), far(b));
        if from == v || to == v {
            return false;
        }
        let mut seen = vec![false; self.nodes.len()];
        seen[from] = true;
        let mut stack = vec![from];
        while let Some(at) = stack.pop() {
            if at == to {
                return true;
            }
            for e in self.edges.iter().filter(|e| e.alive) {
                for end in 0..2 {
                    let next = e.ends[1 - end];
                    if e.ends[end] == at && next != v && !seen[next] {
                        seen[next] = true;
                        stack.push(next);
                    }
                }
            }
        }
        false
    }

    /// Node `v`, which two lines pass through and no more, taken out:
    /// the two are one line.
    fn splice(&mut self, v: usize) {
        let arms = self.arms(v);
        if arms.len() != 2 || arms[0].0 == arms[1].0 {
            return;
        }
        let mut line = self.arm(arms[0]);
        line.reverse();
        let other = self.arm(arms[1]);
        line.extend_from_slice(&other[1..]);
        let from = self.edges[arms[0].0].ends[1 - arms[0].1];
        let to = self.edges[arms[1].0].ends[1 - arms[1].1];
        self.edges[arms[0].0].alive = false;
        self.edges[arms[1].0].alive = false;
        self.nodes[v].alive = false;
        self.edges.push(Edge {
            ends: [from, to],
            line,
            alive: true,
        });
    }

    /// Without the branches that are only a stroke's corners, and with
    /// forks that sit on top of each other made one.
    fn simplify(&mut self, outline: &Outline) {
        loop {
            // The branch that sticks out least, if that's too little.
            let mut worst: Option<(f64, usize, usize)> = None;
            let mut degree = vec![0usize; self.nodes.len()];
            for e in self.edges.iter().filter(|e| e.alive) {
                degree[e.ends[0]] += 1;
                degree[e.ends[1]] += 1;
            }
            for (k, e) in self.edges.iter().enumerate() {
                if !e.alive || e.ends[0] == e.ends[1] {
                    continue;
                }
                let degrees = e.ends.map(|v| degree[v]);
                let (tip, fork) = match degrees {
                    [1, d] if d >= 3 => (0, 1),
                    [d, 1] if d >= 3 => (1, 0),
                    _ => continue,
                };
                let (tip, fork) = (&self.nodes[e.ends[tip]], (e.ends[fork], fork));
                let r = self.nodes[fork.0].r.max(0.5);
                let len = polyline_length(&e.line);
                let out = len + tip.r - r;
                let spur = out < (SPUR * r).max(SPUR_MIN);
                if spur && worst.is_none_or(|(w, _, _)| out / r < w) {
                    worst = Some((out / r, k, fork.0));
                }
            }
            if let Some((_, k, fork)) = worst {
                let e = &mut self.edges[k];
                e.alive = false;
                let tip = if e.ends[0] == fork {
                    e.ends[1]
                } else {
                    e.ends[0]
                };
                self.nodes[tip].alive = false;
                self.splice(fork);
                continue;
            }
            // Two forks nearer than they are wide: one.
            let short = self.edges.iter().position(|e| {
                e.alive
                    && e.ends[0] != e.ends[1]
                    && e.ends.iter().all(|&v| degree[v] >= 3)
                    && polyline_length(&e.line)
                        < MERGE * self.nodes[e.ends[0]].r.max(self.nodes[e.ends[1]].r)
            });
            let Some(k) = short else { break };
            let [a, b] = self.edges[k].ends;
            let p = self.nodes[a].p.lerp(self.nodes[b].p, 0.5);
            self.edges[k].alive = false;
            self.nodes[b].alive = false;
            self.nodes[a].p = p;
            self.nodes[a].r = outline.distance(p);
            for e in self.edges.iter_mut().filter(|e| e.alive) {
                for end in 0..2 {
                    if e.ends[end] == a || e.ends[end] == b {
                        e.ends[end] = a;
                        if end == 0 {
                            e.line.insert(0, p);
                        } else {
                            e.line.push(p);
                        }
                    }
                }
            }
        }
        for v in 0..self.nodes.len() {
            if self.nodes[v].alive {
                self.nodes[v].r = outline.distance(self.nodes[v].p);
            }
        }
    }

    /// A node put in wherever a line turns a sharp corner.
    fn bends(&mut self, outline: &Outline) {
        for k in 0..self.edges.len() {
            if !self.edges[k].alive {
                continue;
            }
            let line = self.edges[k].line.clone();
            let [from, to] = self.edges[k].ends;
            let mut arc = vec![0.0; line.len()];
            for i in 1..line.len() {
                arc[i] = arc[i - 1] + line[i - 1].dist(line[i]);
            }
            let total = arc[line.len() - 1];
            // (A line's ends hook toward a corner of its tip, or curve
            // into the fork: not corners of the stroke.)
            let margin = |v: usize| {
                let n = &self.nodes[v];
                if self.arms(v).len() == 1 {
                    2.0 * n.r
                } else {
                    0.9 * n.r
                }
            };
            let (lead, tail) = (margin(from), margin(to));
            let mut turns: Vec<(f64, usize, f64)> = Vec::new();
            for i in 1..line.len() - 1 {
                let s = arc[i];
                if s < lead || total - s < tail {
                    continue;
                }
                let r = outline.distance(line[i]);
                let w = (REACH * r).max(3.0).min(s).min(total - s);
                if w < 0.6 * r {
                    continue;
                }
                let turn = angle(
                    line[i] - point_along(&line, s - w),
                    point_along(&line, s + w) - line[i],
                );
                if turn > BEND {
                    turns.push((turn, i, w));
                }
            }
            // The sharpest point of each corner.
            turns.sort_by(|a, b| b.0.total_cmp(&a.0));
            let mut at: Vec<(usize, f64)> = Vec::new();
            for (_, i, w) in turns {
                if at
                    .iter()
                    .all(|&(j, wj)| (arc[i] - arc[j]).abs() > 1.5 * w.max(wj))
                {
                    at.push((i, w));
                }
            }
            if at.is_empty() {
                continue;
            }
            at.sort_by_key(|a| a.0);
            self.edges[k].alive = false;
            let (mut start, mut node) = (0, from);
            for (i, _) in at {
                let v = self.nodes.len();
                self.nodes.push(Node {
                    p: line[i],
                    r: outline.distance(line[i]),
                    bend: true,
                    alive: true,
                });
                self.edges.push(Edge {
                    ends: [node, v],
                    line: line[start..=i].to_vec(),
                    alive: true,
                });
                (start, node) = (i, v);
            }
            self.edges.push(Edge {
                ends: [node, to],
                line: line[start..].to_vec(),
                alive: true,
            });
        }
    }
}

/// `line` with the pixels' staircase smoothed out of it (its ends stay).
fn smooth(line: &mut [P]) {
    let n = line.len();
    if n < 3 {
        return;
    }
    for _ in 0..SMOOTH {
        let mut before = line[0];
        for i in 1..n - 1 {
            let here = line[i];
            line[i] = (before + here * 2.0 + line[i + 1]) * 0.25;
            before = here;
        }
    }
}

/// What becomes of a line's end.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Fate {
    /// Nothing yet.
    Open,
    /// The stroke's tip.
    Tip,
    /// The stroke runs on past a corner to the outline, which it meets
    /// along `dir` from `from`.
    Reach { from: P, dir: P },
    /// The stroke butts into another, at this cut.
    Butt(usize),
    /// The stroke goes on along this line (and by this end of it).
    Through(usize, usize),
}

/// A cut across a stroke's mouth, from `a` to `b`: the stroke that butts
/// there is on the left of it. Or (`between`) a cut between two strokes
/// that run side by side along it, neither ending there: the two rings
/// of an 8, across its waist.
#[derive(Clone, Copy, Debug)]
struct Cut {
    a: Spot,
    b: Spot,
    between: bool,
    /// Where two strokes cross, one going through: the cut of a half of
    /// the other, which butts into it from one side as the other half
    /// does from the other.
    crossing: bool,
}

/// What [`resolve`] makes of a skeleton: what becomes of every line's two
/// ends, the cuts, whether any cut was made square that wouldn't have
/// been, and whether an 8 was taken for two rings.
type Resolved = (Vec<[Fate; 2]>, Vec<Cut>, bool, bool);

/// What becomes of every line's two ends, and the cuts where strokes butt.
fn resolve(g: &Graph, outline: &Outline, how: Reading) -> Option<Resolved> {
    let Reading {
        square,
        notch,
        eight,
        strokes,
        ..
    } = how;
    // (Whether any cut was made square that wouldn't have been.)
    let squared = std::cell::Cell::new(false);
    let mut fate = vec![[Fate::Open; 2]; g.edges.len()];
    let mut cuts: Vec<Cut> = Vec::new();
    // A cut for the stroke along `line` (as it leaves the cut), if the
    // cut can be made.
    let cut_for = |a: Spot, b: Spot, line: &[P]| -> Option<Cut> {
        let len = a.p.dist(b.p);
        if len > AUTO_MAX_WIDTH || !outline.clear(a.p, b.p) {
            return None;
        }
        // Which side of the cut the stroke is: where its line is once
        // it has crossed the cut (or, a fork that's already past it,
        // where it ends).
        let d = b.p - a.p;
        let crossing = line.windows(2).position(|w| {
            cross(d, w[0] - a.p) * cross(d, w[1] - a.p) < 0.0
                && cross(w[1] - w[0], a.p - w[0]) * cross(w[1] - w[0], b.p - w[0]) < 0.0
        });
        let beyond = match crossing {
            Some(k) => point_along(&line[k + 1..], 1.0),
            None => *line.last().unwrap(),
        };
        Some(if cross(b.p - a.p, beyond - a.p) >= 0.0 {
            Cut {
                a,
                b,
                between: false,
                crossing: false,
            }
        } else {
            Cut {
                a: b,
                b: a,
                between: false,
                crossing: false,
            }
        })
    };
    // A cut for a stroke that butts into the side of one running on past
    // it to the outline: from `inside` (the corner between the two)
    // straight on along that side. `heading` is the way the stroke that
    // runs on leaves the corner, `rt` half its width. The cut, and the
    // way it runs.
    let carried_on = |inside: Spot, heading: P, rt: f64, line: &[P]| -> Option<(Cut, P)> {
        let wall = [true, false]
            .into_iter()
            .map(|f| {
                (
                    outline.walk(inside, rt, f),
                    outline.walk(inside, 3.0 * rt, f),
                )
            })
            .max_by(|x, y| {
                (x.1 - inside.p)
                    .dot(heading)
                    .total_cmp(&(y.1 - inside.p).dot(heading))
            })
            .map(|(near, far)| (far - near).norm())
            .filter(|d| d.dot(heading) > 0.8)
            .unwrap_or(heading);
        let onward = -wall;
        let far = outline.hit(inside.p, onward, AUTO_MAX_WIDTH)?;
        // (Or, with `notch`, to the corner of a notch right by where
        // that lands, if there is one: the nick between a bowl and the
        // foot of its stem is the other corner of the bowl's mouth, and
        // a cut that misses it leaves a sliver of the bowl on the stem.)
        let nick = notch
            .then_some(())
            .and_then(|_| outline.inside_corner(far.p, NOTCH * rt))
            .filter(|c| c.p.dist(inside.p) > 1.0)
            .and_then(|c| cut_for(inside, c, line));
        Some((nick.or_else(|| cut_for(inside, far, line))?, onward))
    };
    // (With `eight`.) An 8: two forks with three lines between them, the middle one its
    // waist, and its outline nipped in beside each fork. Two rings, one
    // over the other, the waist cut along between the two nips: each
    // ring's stitches go round its own counter. (Taken instead for a ring
    // with a bar across it, as a letter with a real bar is, the ring's
    // stitches would lean all the way round, its inside an hourglass.)
    let mut settled = vec![false; g.nodes.len()];
    for (m, edge) in g.edges.iter().enumerate() {
        let [v, w] = edge.ends;
        if !eight || !edge.alive || v == w || settled[v] || settled[w] {
            continue;
        }
        let (at_v, at_w) = (g.arms(v), g.arms(w));
        // The other two lines at each fork, the same two.
        let others = |arms: &[(usize, usize)]| -> Vec<(usize, usize)> {
            arms.iter().copied().filter(|a| a.0 != m).collect()
        };
        let (ov, ow) = (others(&at_v), others(&at_w));
        if at_v.len() != 3
            || at_w.len() != 3
            || ov.len() != 2
            || ow.len() != 2
            || ov[0].0 == ov[1].0
            || !ov.iter().all(|a| ow.iter().any(|b| b.0 == a.0))
        {
            continue;
        }
        let nip =
            |node: &Node| outline.inside_corner_of(node.p, 2.5 * node.r.max(0.5) + 2.0, Some(0));
        let (Some(a), Some(b)) = (nip(&g.nodes[v]), nip(&g.nodes[w])) else {
            continue;
        };
        if a.p.dist(b.p) > AUTO_MAX_WIDTH || a.p.dist(b.p) < 1.0 || !outline.clear(a.p, b.p) {
            continue;
        }
        // One ring is one of the two lines and the waist, round and
        // round; the other line is the other ring's.
        let (ring, rest) = (ov[0].0, ov[1].0);
        let end_at = |e: usize, node: usize| usize::from(g.edges[e].ends[0] != node);
        for node in [v, w] {
            fate[ring][end_at(ring, node)] = Fate::Through(m, end_at(m, node));
            fate[m][end_at(m, node)] = Fate::Through(ring, end_at(ring, node));
            fate[rest][end_at(rest, node)] = Fate::Tip;
        }
        cuts.push(Cut {
            a,
            b,
            between: true,
            crossing: false,
        });
        settled[v] = true;
        settled[w] = true;
    }
    for (v, node) in g.nodes.iter().enumerate() {
        if !node.alive || settled[v] {
            continue;
        }
        let arms = g.arms(v);
        let r = node.r.max(0.5);
        let lines: Vec<Vec<P>> = arms.iter().map(|&a| g.arm(a)).collect();
        let lens: Vec<f64> = lines.iter().map(|l| polyline_length(l)).collect();
        match arms.len() {
            0 => {}
            1 => fate[arms[0].0][arms[0].1] = Fate::Tip,
            2 if node.bend => {
                let through = |fate: &mut Vec<[Fate; 2]>| {
                    fate[arms[0].0][arms[0].1] = Fate::Through(arms[1].0, arms[1].1);
                    fate[arms[1].0][arms[1].1] = Fate::Through(arms[0].0, arms[0].1);
                };
                // The thicker stroke runs on (of two alike, the longer).
                let thick: Vec<f64> = (0..2)
                    .map(|k| outline.distance(point_along(&lines[k], (2.5 * r).min(lens[k]))))
                    .collect();
                // (How far the stroke's centreline bows, from a little
                // way off the corner to four half-widths along it.)
                let bow = |k: usize| {
                    let (from, to) = ((0.8 * r).min(lens[k] * 0.25), (4.0 * r).min(lens[k]));
                    let (a, b) = (point_along(&lines[k], from), point_along(&lines[k], to));
                    let along = (b - a).norm();
                    (1..8)
                        .map(|i| {
                            let p = point_along(&lines[k], from + (to - from) * f64::from(i) / 8.0);
                            cross(along, p - a).abs()
                        })
                        .fold(0.0, f64::max)
                        / (to - from).max(1.0)
                };
                let on = if (thick[0] - thick[1]).abs() > 0.08 * thick[0].max(thick[1]) {
                    usize::from(thick[1] > thick[0])
                } else {
                    usize::from(lens[1] > lens[0])
                };
                let butt = 1 - on;
                let aim: Vec<P> = (0..2)
                    .map(|k| (point_along(&lines[k], (1.2 * r).min(lens[k])) - node.p).norm())
                    .collect();
                let heading = (point_along(&lines[on], (2.5 * r).min(lens[on]))
                    - point_along(&lines[on], (0.8 * r).min(lens[on] * 0.5)))
                .norm();
                // The corner's inside: the outline nearest it between the
                // two strokes.
                let (lo, hi) = if cross(aim[0], aim[1]) > 0.0 {
                    (aim[0], aim[1])
                } else {
                    (aim[1], aim[0])
                };
                let inside = outline
                    .nearest_between(node.p, lo, hi)
                    .filter(|s| s.p.dist(node.p) < 2.0 * r + 2.0);
                let Some(inside) = inside else {
                    through(&mut fate);
                    continue;
                };
                let made = carried_on(inside, heading, thick[on].max(0.5), &lines[butt]);
                // Square across the stroke that butts, where that's much
                // the way the other's side runs on anyway (the bar of an
                // e into the end of its bowl): the bar ends square, its
                // stitches not fanning to meet a slanted cut.
                let made = made.map(|(cut, onward)| {
                    let along = (point_along(&lines[butt], (2.5 * r).min(lens[butt]))
                        - point_along(&lines[butt], (0.8 * r).min(lens[butt] * 0.5)))
                    .norm();
                    let across = if along.perp().dot(onward) >= 0.0 {
                        along.perp()
                    } else {
                        -along.perp()
                    };
                    // (The way the side runs on, taken further along
                    // it or right at the corner: where it curves into
                    // the corner, as a counter's does, they differ, and
                    // it's from the corner that the cut goes on.)
                    let rt = thick[on].max(0.5);
                    let at_corner = [true, false]
                        .into_iter()
                        .map(|f| {
                            (
                                outline.walk(inside, 0.15 * rt, f),
                                outline.walk(inside, 0.8 * rt, f),
                            )
                        })
                        .max_by(|x, y| {
                            (x.1 - inside.p)
                                .dot(heading)
                                .total_cmp(&(y.1 - inside.p).dot(heading))
                        })
                        .map(|(near, far)| (near - far).norm());
                    Some(across)
                        .filter(|a| {
                            square
                                // (Of a stroke that runs straight: there's
                                // no one way across a bowl's foot.)
                                && bow(butt) < STRAIGHT
                                && a.dot(onward) < 0.999
                                && (a.dot(onward) >= SQUARE_BUTT
                                    || at_corner.is_some_and(|d| a.dot(d) >= SQUARE_BUTT))
                        })
                        .and_then(|a| {
                            let far = outline.hit(inside.p, a, AUTO_MAX_WIDTH)?;
                            let cut = cut_for(inside, far, &lines[butt])?;
                            squared.set(true);
                            Some((cut, a))
                        })
                        .unwrap_or((cut, onward))
                });
                let Some((cut, onward)) = made else {
                    through(&mut fate);
                    continue;
                };
                fate[arms[butt].0][arms[butt].1] = Fate::Butt(cuts.len());
                cuts.push(cut);
                // (From back along the stroke, where it's surely on its
                // own side of the cut: a sharp corner's own point is all
                // but on it.)
                fate[arms[on].0][arms[on].1] = Fate::Reach {
                    from: point_along(&lines[on], (2.0 * thick[on]).min(lens[on])),
                    dir: onward,
                };
            }
            2 => {
                fate[arms[0].0][arms[0].1] = Fate::Through(arms[1].0, arms[1].1);
                fate[arms[1].0][arms[1].1] = Fate::Through(arms[0].0, arms[0].1);
            }
            n => {
                // Round the fork in order.
                let aim: Vec<P> = (0..n)
                    .map(|k| (point_along(&lines[k], r.min(lens[k])) - node.p).norm())
                    .collect();
                let mut order: Vec<usize> = (0..n).collect();
                order.sort_by(|&a, &b| {
                    aim[a]
                        .y
                        .atan2(aim[a].x)
                        .total_cmp(&aim[b].y.atan2(aim[b].x))
                });
                let heading: Vec<P> = (0..n)
                    .map(|k| {
                        (point_along(&lines[k], (3.0 * r).min(lens[k]))
                            - point_along(&lines[k], r.min(lens[k] * 0.5)))
                        .norm()
                    })
                    .collect();
                let thick: Vec<f64> = (0..n)
                    .map(|k| outline.distance(point_along(&lines[k], (2.0 * r).min(lens[k]))))
                    .collect();
                // The mouth of stroke `k` on the fork, between its
                // neighbours `before` and `after` round the fork: its two
                // corners (the one toward `before` first).
                let mouth = |k: usize, before: usize, after: usize| -> Option<(Spot, Spot)> {
                    // Where the outline comes nearest the fork, either
                    // side of the stroke…
                    let touch = || {
                        let near = |lo: P, hi: P| {
                            outline
                                .nearest_between(node.p, lo, hi)
                                .filter(|s| s.p.dist(node.p) < 2.0 * r + 2.0)
                        };
                        Some((near(aim[before], aim[k])?, near(aim[k], aim[after])?))
                            .filter(|m: &(Spot, Spot)| outline.clear(m.0.p, m.1.p))
                    };
                    // …or, out along the stroke, the first place its own
                    // two sides are either side of it: a thin stroke
                    // meeting a thick one at a slant comes nearest the
                    // fork along the thick one's side, not at its mouth.
                    let walked = || {
                        let half = thick[k].max(0.5);
                        let end = lens[k].min(3.0 * r);
                        let mut s = 0.3 * r;
                        while s <= end {
                            let p = point_along(&lines[k], s);
                            let d = (point_along(&lines[k], (s + 1.0).min(lens[k]))
                                - point_along(&lines[k], (s - 1.0).max(0.0)))
                            .norm();
                            let side = |sign: f64| {
                                let c = std::f64::consts::FRAC_1_SQRT_2;
                                let (x, y) = (d.rotate(c, c * sign), d.rotate(-c, c * sign));
                                let (lo, hi) = if sign > 0.0 { (x, y) } else { (y, x) };
                                outline
                                    .nearest_between(p, lo, hi)
                                    .filter(|q| q.p.dist(p) < 1.5 * half + 1.0)
                            };
                            if let (Some(a), Some(b)) = (side(-1.0), side(1.0))
                                && a.p.dist(b.p) <= 2.7 * half + 1.0
                                && outline.clear(a.p, b.p)
                            {
                                return Some((a, b));
                            }
                            s += 0.5;
                        }
                        None
                    };
                    if thick[k] < 0.6 * r {
                        walked().or_else(touch)
                    } else {
                        touch().or_else(walked)
                    }
                };
                // A short branch that ends at its tip: a serif, or what's
                // left past the fork of two strokes that meet at a point
                // (the top of an A).
                let stub = |k: usize| {
                    let far = g.edges[arms[k].0].ends[1 - arms[k].1];
                    g.arms(far).len() == 1 && lens[k] <= 4.0 * r
                };
                // The two that run straightest into each other, and are
                // most alike in thickness, go through (of two such pairs,
                // the thicker: a stem, not the bar across it). (A stub
                // tapers, and is as thick as whatever it's the end of:
                // the thicker of two strokes that meet at a point.)
                let mut best: Option<(f64, usize, usize)> = None;
                for i in 0..n {
                    for j in i + 1..n {
                        let off = std::f64::consts::PI - angle(heading[i], heading[j]);
                        let unlike = if stub(i) != stub(j) {
                            0.0
                        } else {
                            (thick[i].max(0.1) / thick[j].max(0.1)).ln().abs()
                        };
                        let thick = |k: usize| if stub(k) { f64::INFINITY } else { thick[k] };
                        let both = thick(i).min(thick(j));
                        let score = off + 0.6 * unlike - 0.2 * both.min(2.0 * r) / r;
                        if best.is_none_or(|(s, _, _)| score < s) {
                            best = Some((score, i, j));
                        }
                    }
                }
                let (_, mut i, mut j) = best?;
                // How far from straight on two of them meet: near the fork
                // as well as further off, since the two halves of a small
                // bowl curve away from each other but leave the fork in
                // line.
                let leaving = |k: usize| {
                    (point_along(&lines[k], (1.5 * r).min(lens[k]))
                        - point_along(&lines[k], (0.5 * r).min(lens[k] * 0.5)))
                    .norm()
                };
                let off = |a: usize, b: usize| {
                    (std::f64::consts::PI - angle(heading[a], heading[b]))
                        .min(std::f64::consts::PI - angle(leaving(a), leaving(b)))
                };
                // Where three meet, one may run on to the outline instead,
                // the other two butting into its sides.
                let mut on = None;
                if n == 3 {
                    let third = 3 - i - j;
                    if !(stub(i) && stub(j)) && thick[third] >= 1.25 * thick[i].max(thick[j]) {
                        // A stroke much thicker than the two that meet it:
                        // a stem, with a flag serif one side and an arm or
                        // a bowl the other. (But not a stem on the slab
                        // serif across its foot, which is a stroke of its
                        // own.)
                        on = Some(third);
                    } else if off(i, j) > CROOKED && !(strokes && (stub(i) || stub(j))) {
                        // No two run straight into each other. The two
                        // halves of a bowl are one stroke all the same (the
                        // bowl of a g, its ear and its link butting into
                        // it); failing that, the one that points most
                        // squarely away from the other two runs on (the
                        // stem of an N, its serif to one side and its
                        // diagonal to the other).
                        let bowl = [(0, 1), (0, 2), (1, 2)]
                            .into_iter()
                            .filter(|&(a, b)| g.looped(v, arms[a], arms[b]))
                            .min_by(|x, y| off(x.0, x.1).total_cmp(&off(y.0, y.1)));
                        if let Some(pair) = bowl {
                            (i, j) = pair;
                        } else {
                            let apart = |a: usize| {
                                (0..3)
                                    .filter(|&c| c != a)
                                    .map(|c| angle(-heading[a], heading[c]))
                                    .fold(f64::INFINITY, f64::min)
                            };
                            on = (0..3)
                                .max_by(|&a, &b| apart(a).total_cmp(&apart(b)))
                                .filter(|&a| apart(a) >= 1.0);
                        }
                    }
                }
                if let Some(k) = on {
                    let at = order.iter().position(|&o| o == k).unwrap();
                    let made: Vec<(usize, Cut, P)> = (0..3)
                        .filter(|&c| c != k)
                        .filter_map(|c| {
                            // From the corner of its mouth on the stem's
                            // side, straight on along the stem…
                            let other = 3 - k - c;
                            let (inside, far) = if order[(at + 1) % 3] == c {
                                mouth(c, k, other)?
                            } else {
                                let (far, inside) = mouth(c, other, k)?;
                                (inside, far)
                            };
                            let (cut, onward) =
                                carried_on(inside, heading[k], thick[k].max(0.5), &lines[c])?;
                            // …or across its mouth, if the other corner is
                            // on this stroke's side of the stem and that's
                            // the shorter way (an arm that leaves below
                            // the stem's top, a notch between them).
                            let across = Some(far)
                                .filter(|s| {
                                    let off = cross(heading[k], s.p - node.p);
                                    off * cross(heading[k], aim[c]) > 0.0
                                        && off.abs() > 0.4 * thick[k]
                                        && s.p.dist(inside.p) < cut.a.p.dist(cut.b.p)
                                })
                                .and_then(|s| cut_for(inside, s, &lines[c]));
                            Some((c, across.unwrap_or(cut), onward))
                        })
                        .collect();
                    // (If either can't be cut so, the fork is taken as two
                    // going through and one butting after all.)
                    if made.len() == 2 {
                        let dir = (made[0].2 + made[1].2).norm();
                        let from = point_along(&lines[k], r.min(lens[k]));
                        fate[arms[k].0][arms[k].1] = Fate::Reach { from, dir };
                        for (c, cut, _) in made {
                            fate[arms[c].0][arms[c].1] = Fate::Butt(cuts.len());
                            cuts.push(cut);
                        }
                        continue;
                    }
                }
                fate[arms[i].0][arms[i].1] = Fate::Through(arms[j].0, arms[j].1);
                fate[arms[j].0][arms[j].1] = Fate::Through(arms[i].0, arms[i].1);
                // The others butt: each cut between the places the outline
                // comes nearest the fork either side of the stroke.
                for at in 0..n {
                    let k = order[at];
                    if k == i || k == j {
                        continue;
                    }
                    let (before, after) = (order[(at + n - 1) % n], order[(at + 1) % n]);
                    let (a, b) = mouth(k, before, after)?;
                    let mut cut = cut_for(a, b, &lines[k])?;
                    // (Two that butt from either side of the one going
                    // through, running on into each other, are the halves
                    // of a stroke that crosses it.)
                    cut.crossing = n == 4 && {
                        let other = (0..n).find(|&o| o != i && o != j && o != k);
                        other.is_some_and(|o| off(k, o) <= CROOKED)
                    };
                    fate[arms[k].0][arms[k].1] = Fate::Butt(cuts.len());
                    cuts.push(cut);
                }
            }
        }
    }
    let eights = settled.contains(&true);
    Some((fate, cuts, squared.get(), eights))
}

/// One stroke of the shape: its centreline, and what its two ends are.
#[derive(Clone, Debug)]
struct Stroke {
    line: Vec<P>,
    ends: [Fate; 2],
    /// The forks it goes through (where, and half the width there): its
    /// centreline is pulled toward whatever butts into it at each.
    forks: Vec<(P, f64)>,
}

/// The lines joined end to end into strokes, as their fates say.
fn strokes(g: &Graph, fate: &[[Fate; 2]]) -> Vec<Stroke> {
    let mut used = vec![false; g.edges.len()];
    let mut out = Vec::new();
    let follow = |mut e: usize, mut end: usize, used: &mut Vec<bool>| {
        let first = fate[e][end];
        let mut line: Vec<P> = Vec::new();
        let mut forks: Vec<(P, f64)> = Vec::new();
        loop {
            used[e] = true;
            let part = g.arm((e, end));
            let skip = usize::from(!line.is_empty());
            line.extend_from_slice(&part[skip..]);
            match fate[e][1 - end] {
                Fate::Through(next, by) if !used[next] => {
                    let node = &g.nodes[g.edges[e].ends[1 - end]];
                    forks.push((node.p, node.r));
                    (e, end) = (next, by);
                }
                last => {
                    return Stroke {
                        line,
                        ends: [first, last],
                        forks,
                    };
                }
            }
        }
    };
    for e in 0..g.edges.len() {
        for (end, to) in fate[e].iter().enumerate() {
            if g.edges[e].alive && !used[e] && !matches!(to, Fate::Through(..)) {
                out.push(follow(e, end, &mut used));
            }
        }
    }
    // What's left goes round and round: rings.
    for e in 0..g.edges.len() {
        if g.edges[e].alive && !used[e] {
            out.push(follow(e, 0, &mut used));
        }
    }
    out
}

/// What an edge of a piece is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// The shape's own outline.
    Wall,
    /// A cut along the piece's side: another stroke butts into it there.
    Side,
    /// A cut that's the piece's end: this cut, where it butts into another.
    End(usize),
}

/// One piece of the shape, cut along the cuts: its outline (the material
/// on its left), and what the edge from each point to the next is.
#[derive(Clone, Debug)]
struct Face {
    pts: Vec<P>,
    kind: Vec<Kind>,
    holes: Vec<Polyline>,
}

/// The pieces the cuts leave the shape in.
fn faces(outline: &Outline, cuts: &[Cut]) -> Option<Vec<Face>> {
    // The outline's points, with a point put in wherever a cut ends (two
    // cuts that end together share one).
    const SAME: f64 = 0.05;
    let mut pts: Vec<P> = Vec::new();
    let mut next: Vec<usize> = Vec::new();
    // Each cut's two ends, as points.
    let mut ends = vec![[usize::MAX; 2]; cuts.len()];
    for (k, ring) in outline.rings.iter().enumerate() {
        let n = ring.len();
        let base = pts.len();
        let mut put: Vec<(usize, f64, usize, usize)> = Vec::new();
        for (c, cut) in cuts.iter().enumerate() {
            for (which, s) in [cut.a, cut.b].into_iter().enumerate() {
                if s.ring == k {
                    put.push((s.edge, s.t, c, which));
                }
            }
        }
        put.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        // (Those that fall on the ring's own points wait for the point.)
        let mut waiting: Vec<(usize, usize, usize)> = Vec::new();
        let mut first = vec![usize::MAX; n];
        let mut at = 0;
        for i in 0..n {
            first[i] = pts.len();
            pts.push(ring[i]);
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            let len = a.dist(b);
            while at < put.len() && put[at].0 == i {
                let (_, t, c, which) = put[at];
                at += 1;
                if t * len < SAME {
                    ends[c][which] = first[i];
                } else if (1.0 - t) * len < SAME {
                    waiting.push(((i + 1) % n, c, which));
                } else {
                    let p = a.lerp(b, t);
                    let last = pts.len() - 1;
                    if last > first[i] && pts[last].dist(p) < SAME {
                        ends[c][which] = last;
                    } else {
                        ends[c][which] = pts.len();
                        pts.push(p);
                    }
                }
            }
        }
        for (i, c, which) in waiting {
            ends[c][which] = first[i];
        }
        let count = pts.len() - base;
        next.extend((0..count).map(|i| base + (i + 1) % count));
    }
    if ends.iter().any(|e| e[0] == e[1] || e.contains(&usize::MAX)) {
        return None;
    }
    // No two cuts cross.
    for (x, a) in ends.iter().enumerate() {
        for b in &ends[x + 1..] {
            if a.iter().any(|v| b.contains(v)) {
                continue;
            }
            let (p, q, r, s) = (pts[a[0]], pts[a[1]], pts[b[0]], pts[b[1]]);
            if cross(q - p, r - p) * cross(q - p, s - p) < 0.0
                && cross(s - r, p - r) * cross(s - r, q - r) < 0.0
            {
                return None;
            }
        }
    }
    // Every edge both ways round: a wall is edge v (from point v to the
    // next); cut c is edges V + 2c (its stroke's end) and V + 2c + 1 (back
    // the other way: the side of the stroke it butts into).
    let walls = pts.len();
    let total = walls + 2 * cuts.len();
    let tail = |e: usize| {
        if e < walls {
            e
        } else {
            ends[(e - walls) / 2][(e - walls) % 2]
        }
    };
    let head = |e: usize| {
        if e < walls {
            next[e]
        } else {
            ends[(e - walls) / 2][1 - (e - walls) % 2]
        }
    };
    let mut leaving: Vec<Vec<usize>> = vec![Vec::new(); walls];
    for e in walls..total {
        leaving[tail(e)].push(e);
    }
    // Round a piece with it on the left: at each point, the edge that
    // turns back furthest toward where the last one came from.
    let after = |e: usize| -> usize {
        let v = head(e);
        let back = pts[tail(e)] - pts[v];
        let mut best = (f64::INFINITY, v);
        for &out in std::iter::once(&v).chain(&leaving[v]) {
            let d = pts[head(out)] - pts[v];
            let turn = cross(back, d).atan2(back.dot(d));
            let turn = if turn < -1e-12 {
                -turn
            } else {
                std::f64::consts::TAU - turn
            };
            if turn < best.0 {
                best = (turn, out);
            }
        }
        best.1
    };
    let mut seen = vec![false; total];
    let mut found: Vec<Face> = Vec::new();
    // (Each hole, and a point of it that's the shape's own: not on a cut.)
    let mut holes: Vec<(Polyline, P)> = Vec::new();
    for start in 0..total {
        if seen[start] {
            continue;
        }
        let mut face = Face {
            pts: Vec::new(),
            kind: Vec::new(),
            holes: Vec::new(),
        };
        let mut e = start;
        loop {
            if seen[e] {
                return None;
            }
            seen[e] = true;
            face.pts.push(pts[tail(e)]);
            face.kind.push(if e < walls {
                Kind::Wall
            } else if (e - walls).is_multiple_of(2) && !cuts[(e - walls) / 2].between {
                Kind::End((e - walls) / 2)
            } else {
                Kind::Side
            });
            e = after(e);
            if e == start {
                break;
            }
        }
        if area(&face.pts) > 0.0 {
            found.push(face);
        } else if face
            .kind
            .iter()
            .all(|k| matches!(k, Kind::Wall | Kind::Side))
        {
            // A hole no cut reached; or two that a stroke between them
            // was cut out of (the waist of an 8, taken for a bar across a
            // ring): one hole now, the cuts part of its edge, for the
            // ring to go round.
            let n = face.pts.len();
            let own = (0..n)
                .find(|&i| face.kind[i] == Kind::Wall && face.kind[(i + n - 1) % n] == Kind::Wall)
                .map(|i| face.pts[i]);
            holes.push((face.pts, own?));
        } else {
            return None;
        }
    }
    // A hole is a hole of the piece round it.
    for (hole, own) in holes {
        let round = found
            .iter_mut()
            .filter(|f| point_inside(std::slice::from_ref(&f.pts), FillRule::EvenOdd, own))
            .min_by(|a, b| area(&a.pts).total_cmp(&area(&b.pts)))?;
        round.holes.push(hole);
    }
    Some(found)
}

impl Face {
    fn len(&self) -> usize {
        self.pts.len()
    }

    fn inside(&self, p: P) -> bool {
        point_inside(std::slice::from_ref(&self.pts), FillRule::EvenOdd, p)
            && !self
                .holes
                .iter()
                .any(|h| point_inside(std::slice::from_ref(h), FillRule::EvenOdd, p))
    }

    /// The point where a ray from `from` along `dir` first meets the
    /// piece's wall, put into its outline: which point that is, and
    /// whether it's a new one (the points after it each one further on).
    fn tip(&mut self, from: P, dir: P) -> Option<(usize, bool)> {
        let n = self.len();
        let mut best: Option<(f64, usize, f64)> = None;
        for i in 0..n {
            if self.kind[i] != Kind::Wall {
                continue;
            }
            let (a, b) = (self.pts[i], self.pts[(i + 1) % n]);
            let e = b - a;
            let den = cross(dir, e);
            if den.abs() < 1e-12 {
                continue;
            }
            let w = a - from;
            let s = cross(w, e) / den;
            let t = cross(w, dir) / den;
            if s > 1e-6 && (-1e-9..=1.0 + 1e-9).contains(&t) && best.is_none_or(|(bs, _, _)| s < bs)
            {
                best = Some((s, i, t.clamp(0.0, 1.0)));
            }
        }
        let (_, i, t) = best?;
        let (a, b) = (self.pts[i], self.pts[(i + 1) % n]);
        let len = a.dist(b);
        if t * len < 0.05 {
            return Some((i, false));
        }
        if (1.0 - t) * len < 0.05 {
            return Some(((i + 1) % n, false));
        }
        self.pts.insert(i + 1, a.lerp(b, t));
        self.kind.insert(i + 1, Kind::Wall);
        Some((i + 1, true))
    }

    /// The end of a column whose tip is point `at`, the column running
    /// along `axis` to it: the outline either side of the tip for as far
    /// as it runs across the column (to a corner, or round a blunt end to
    /// where it turns along the column's sides). The points it's from and
    /// to; both the tip, for a column that comes to a point.
    fn cap(&self, at: usize, axis: P) -> (usize, usize) {
        let n = self.len();
        let back = -axis;
        let mut ends = [at, at];
        for (side, end) in ends.iter_mut().enumerate() {
            let mut k = at;
            let mut walked = 0.0;
            loop {
                let (edge, to) = if side == 1 {
                    (k, (k + 1) % n)
                } else {
                    ((k + n - 1) % n, (k + n - 1) % n)
                };
                let d = self.pts[to] - self.pts[k];
                if self.kind[edge] != Kind::Wall || d.norm().dot(back) > CAP {
                    *end = k;
                    break;
                }
                walked += d.len();
                if walked > AUTO_MAX_WIDTH || to == at {
                    // (Not an end at all: a point, then.)
                    return (at, at);
                }
                k = to;
            }
        }
        if self.pts[ends[0]].dist(self.pts[ends[1]]) > AUTO_MAX_WIDTH {
            return (at, at);
        }
        // A round end is closed in toward its tip until what's left
        // beyond the last stitch is too little to show.
        let [mut lo, mut hi] = ends;
        while lo != at || hi != at {
            let (a, b) = (self.pts[lo], self.pts[hi]);
            let across = (b - a).norm();
            let mut bulge: f64 = 0.0;
            let mut k = lo;
            while k != hi {
                k = (k + 1) % n;
                bulge = bulge.max(cross(across, self.pts[k] - a).abs());
            }
            if bulge <= BULGE {
                break;
            }
            if (at + n - lo) % n >= (hi + n - at) % n {
                lo = (lo + 1) % n;
            } else {
                hi = (hi + n - 1) % n;
            }
        }
        (lo, hi)
    }

    /// The outline from point `from` on round to point `to`.
    fn path(&self, from: usize, to: usize) -> Polyline {
        let n = self.len();
        let mut out = vec![self.pts[from]];
        let mut k = from;
        while k != to {
            k = (k + 1) % n;
            out.push(self.pts[k]);
        }
        out
    }

    fn walls(&self) -> Vec<(P, P)> {
        let n = self.len();
        let mut out: Vec<(P, P)> = (0..n)
            .filter(|&i| self.kind[i] == Kind::Wall)
            .map(|i| (self.pts[i], self.pts[(i + 1) % n]))
            .collect();
        for h in &self.holes {
            out.extend((0..h.len()).map(|i| (h[i], h[(i + 1) % h.len()])));
        }
        out
    }

    /// This piece as the column of `stroke`.
    fn column(&mut self, stroke: &Stroke, outline: &Outline, loose: bool) -> Option<Rails> {
        if !self.holes.is_empty() {
            // A ring round its hole (with whatever butts into it cut off).
            let cut_ends = self.kind.iter().any(|k| matches!(k, Kind::End(_)));
            if self.holes.len() != 1 || cut_ends {
                return None;
            }
            let rings = [self.pts.clone(), self.holes[0].clone()];
            let rails = auto_rails(&rings).ok()?;
            return valid(&rails, &rings, &self.walls()).then_some(rails);
        }
        // Its two ends: where it butts, or its tip.
        let line = &stroke.line;
        let total = polyline_length(line);
        // (Each with the way the column runs to it, by its middle line's
        // last stretch and by the stroke itself, and how thick the
        // stroke is there.)
        let mut tips: Vec<(usize, P, P, f64)> = Vec::new();
        let mut margin = [0.0; 2];
        for (which, fate) in stroke.ends.iter().enumerate() {
            let (from, dir, own, thick) = match *fate {
                Fate::Tip => {
                    // Along the stroke's last stretch (but for the very
                    // end, which may hook toward a corner).
                    let along: Vec<P> = if which == 0 {
                        line.iter().rev().copied().collect()
                    } else {
                        line.clone()
                    };
                    let end = *along.last().unwrap();
                    let r = outline.distance(end);
                    let aim = |r: f64| {
                        let (a, b) = (
                            point_along(&along, (total - 3.0 * r).max(total * 0.5)),
                            point_along(&along, (total - r).max(total * 0.75)),
                        );
                        (b, (b - a).norm())
                    };
                    // The stroke's own direction there: a blunt end's two
                    // corners each draw the middle line toward them, and
                    // thinning keeps one, so the line may run on into
                    // that corner and its last stretch point askew.
                    // Taken from back where the line is in the middle of
                    // the stroke: as far from the outline as it is
                    // anywhere it's [`BLUNT`] that far, or more, for how
                    // far back that is (a stroke that tapers to a point
                    // never is).
                    let reach = (total * 0.5).min(AUTO_MAX_WIDTH);
                    let thick = (0..=reach as usize)
                        .map(|t| t as f64)
                        .map(|t| (t, outline.distance(point_along(&along, total - t))))
                        .filter(|&(t, r)| r >= BLUNT * t)
                        .map(|(_, r)| r)
                        .fold(r, f64::max);
                    margin[which] = 1.5 * r;
                    let (from, dir) = aim(r);
                    (from, dir, aim(thick).1, thick)
                }
                Fate::Reach { from, dir } => {
                    margin[which] = 1.5 * outline.distance(from);
                    (from, dir, dir, f64::INFINITY)
                }
                _ => continue,
            };
            let (at, put) = self.tip(from, dir)?;
            if put {
                for t in tips.iter_mut().filter(|t| t.0 >= at) {
                    t.0 += 1;
                }
            }
            tips.push((at, dir, own, thick));
        }
        let n = self.len();
        let mut ends: Vec<(usize, usize)> = (0..n)
            .filter(|&i| matches!(self.kind[i], Kind::End(_)))
            .map(|i| (i, (i + 1) % n))
            .collect();
        ends.extend(tips.iter().map(|&(at, dir, own, thick)| {
            // (An end that lies along its own stroke, for further than
            // the stroke is wide, isn't one: the stroke's side, taken
            // for its end by a line gone askew. Then the end is where
            // the stroke itself points.)
            let (lo, hi) = self.cap(at, dir);
            let across = self.pts[hi] - self.pts[lo];
            if across.len() > 2.0 * thick && across.norm().dot(own).abs() > ALONG {
                self.cap(at, own)
            } else {
                (lo, hi)
            }
        }));
        let &[first, second] = ends.as_slice() else {
            return None;
        };
        let outlines = [self.pts.clone()];
        let walls = self.walls();
        let column = |(i, j): (usize, usize), (k, l): (usize, usize)| -> Option<Rails> {
            let a = self.path(j, k);
            let b: Polyline = self.path(l, i).into_iter().rev().collect();
            if a.len() < 2 || b.len() < 2 {
                return None;
            }
            let (a, b) = if loose {
                (bridged(&a, &b), bridged(&b, &a))
            } else {
                (a, b)
            };
            let mut rails = Rails {
                a,
                b,
                closed: false,
                sync: Vec::new(),
            };
            rails.sync = self.rungs(&rails, stroke, margin, outline);
            // (A stroke of much contrast, a bowl that's a hairline at
            // its ends and heavy round its middle, swells further than
            // most: let, if there's no making it a column otherwise.)
            let swell = if loose { SWELL_MOST } else { SWELL };
            (sewable(&rails, &outlines, &walls, NOTCHED, swell) && !crossed(&rails))
                .then_some(rails)
        };
        // With its ends as found; failing that, with a cut it butts at
        // taken for its side, and the column come to a point at one end of
        // the cut. (A heavy bowl leaves its stem all but along it: the
        // cut between them runs the way the bowl does, not across it, and
        // a stitch along it would cross the bowl's own. The bowl tapers
        // into the stem instead.)
        let cuts = ends.len() - tips.len();
        let ways = |k: usize, (i, j): (usize, usize)| -> Vec<(usize, usize)> {
            if k < cuts {
                vec![(i, j), (j, j), (i, i)]
            } else {
                vec![(i, j)]
            }
        };
        let (firsts, seconds) = (ways(0, first), ways(1, second));
        let mut tries: Vec<((usize, usize), (usize, usize))> = firsts
            .iter()
            .flat_map(|&f| seconds.iter().map(move |&s| (f, s)))
            .collect();
        // (Fewest cuts taken for sides first.)
        tries.sort_by_key(|&(f, s)| usize::from(f != first) + usize::from(s != second));
        tries.into_iter().find_map(|(f, s)| column(f, s))
    }

    /// Rungs square across the column from its centreline: how far along
    /// each rail the two ends of each are.
    fn rungs(
        &self,
        rails: &Rails,
        stroke: &Stroke,
        margin: [f64; 2],
        outline: &Outline,
    ) -> Vec<(f64, f64)> {
        let line = &stroke.line;
        let total = polyline_length(line);
        let (la, lb) = (polyline_length(&rails.a), polyline_length(&rails.b));
        let mut found: Vec<(f64, f64)> = Vec::new();
        let count = (total / RUNG).floor() as usize;
        for k in 1..count {
            let s = k as f64 * total / count as f64;
            // (Not by the stroke's own tip, where its centreline hooks
            // toward a corner or rounds one.)
            if s < margin[0] || total - s < margin[1] {
                continue;
            }
            let p = point_along(line, s);
            if !self.inside(p) || stroke.forks.iter().any(|&(f, r)| f.dist(p) < 1.3 * r) {
                continue;
            }
            // (The stroke's direction, taken over about its width: the
            // centreline wavers by a pixel or so.)
            let half = outline.distance(p);
            let w = half.max(3.0);
            let along = (point_along(line, (s + w).min(total))
                - point_along(line, (s - w).max(0.0)))
            .norm();
            let across = along.perp();
            let reach = 1.8 * half + 1.5;
            for n in [across, -across] {
                if let (Some((da, ta)), Some((db, tb))) =
                    (ray_on_line(p, n, &rails.a), ray_on_line(p, -n, &rails.b))
                    && da <= reach
                    && db <= reach
                {
                    if ta > 1.0 && ta < la - 1.0 && tb > 1.0 && tb < lb - 1.0 {
                        found.push((ta, tb));
                    }
                    break;
                }
            }
        }
        // The most of them that are in step along both rails.
        found.sort_by(|x, y| x.0.total_cmp(&y.0));
        let n = found.len();
        let mut best = vec![1usize; n];
        let mut from = vec![usize::MAX; n];
        for i in 0..n {
            for j in 0..i {
                if found[j].0 + 0.5 < found[i].0
                    && found[j].1 + 0.5 < found[i].1
                    && best[j] + 1 > best[i]
                {
                    best[i] = best[j] + 1;
                    from[i] = j;
                }
            }
        }
        let mut out = Vec::new();
        let mut at = (0..n).max_by_key(|&i| best[i]).unwrap_or(usize::MAX);
        while at != usize::MAX {
            out.push(found[at]);
            at = from[at];
        }
        out.reverse();
        out
    }
}

/// A column's side (`rail`) with the nicks in it bridged: where it turns
/// sharply in toward the column's other side (`other`) and out again
/// within one of the [`NICK`] reaches either way, straight across the
/// mouth of that.
/// (The nick between a bowl and the foot of its stem, in a letter sewn
/// as one stroke round both: stitches to either side of it would cross
/// each other.)
fn bridged(rail: &[P], other: &[P]) -> Polyline {
    let n = rail.len();
    let off = |p: P| {
        other
            .windows(2)
            .map(|w| nearest_on(w[0], w[1], p).1.dist(p))
            .fold(f64::INFINITY, f64::min)
    };
    let mut cum = vec![0.0; n];
    for i in 1..n {
        cum[i] = cum[i - 1] + rail[i].dist(rail[i - 1]);
    }
    let total = cum[n - 1];
    // The stretches of the rail to leave out: from and to, along it.
    let mut gaps: Vec<(f64, f64)> = Vec::new();
    for i in 1..n.saturating_sub(1) {
        let (before, after) = (
            (rail[i] - rail[i - 1]).norm(),
            (rail[i + 1] - rail[i]).norm(),
        );
        if cross(before, after).atan2(before.dot(after)).abs() < NOTCH_TURN {
            continue;
        }
        // The mouth: the rail as far as the nick reaches either side
        // (the least that takes it all in).
        let mouth = NICK.iter().find_map(|&reach| {
            let (from, to) = ((cum[i] - reach).max(0.0), (cum[i] + reach).min(total));
            let (a, b) = (point_along(rail, from), point_along(rail, to));
            let deep = nearest_on(a, b, rail[i]).1.dist(rail[i]);
            // (A nick: in toward the other side, no deeper than it
            // reaches, and the rail going on beyond it the way it came.
            // A corner the rail turns goes off another way.)
            let came = (a - point_along(rail, (from - 2.0).max(0.0))).norm();
            let goes = (point_along(rail, (to + 2.0).min(total)) - b).norm();
            // (Its corner the deepest of it: nearer the other side than
            // either end of its mouth. The foot of a bump out of the
            // column is in from the bump, but not from the rail.)
            ((1.5..=reach).contains(&deep)
                && off(rail[i]) < off(a) - 1.0
                && off(rail[i]) < off(b) - 1.0
                && from > 0.0
                && to < total
                && came.dot(goes) > 0.5
                && (b - a).norm().dot(came) > 0.5)
                .then_some((from, to))
        });
        gaps.extend(mouth);
    }
    if gaps.is_empty() {
        return rail.to_vec();
    }
    let mut out: Polyline = Vec::with_capacity(n);
    let put = |out: &mut Polyline, p: P| {
        if out.last().is_none_or(|l| l.dist(p) > 1e-6) {
            out.push(p);
        }
    };
    let mut done = 0.0;
    for &(from, to) in &gaps {
        if from < done {
            done = done.max(to);
            continue;
        }
        (0..n)
            .filter(|&k| cum[k] >= done && cum[k] <= from)
            .for_each(|k| put(&mut out, rail[k]));
        put(&mut out, point_along(rail, from));
        put(&mut out, point_along(rail, to));
        done = to;
    }
    (0..n)
        .filter(|&k| cum[k] >= done)
        .for_each(|k| put(&mut out, rail[k]));
    out
}

/// Whether any of a column's stitches crosses the next, by more than a
/// thread's width. (Stitches fanning round a notch in a rail meet there,
/// and may cross by a hair: that's a fan, and sews as one.)
pub(super) fn crossed(rails: &Rails) -> bool {
    let ps = pairs(rails, 4.0);
    ps.windows(2).any(|w| {
        let ((p, q), (r, s)) = (w[0], w[1]);
        let (d, e) = (q - p, s - r);
        let den = cross(d, e);
        if den.abs() < 1e-9 {
            return false;
        }
        // Where along each the two cross.
        let t = cross(r - p, e) / den;
        let u = cross(r - p, d) / den;
        let deep = |t: f64, len: f64| t.min(1.0 - t) * len > CROSS;
        deep(t, d.len()) && deep(u, e.len())
    })
}

/// The shape's outline and its skeleton, strokes that turn a corner told
/// from ones that curve.
fn skeleton_of(comp: &[Polyline]) -> Option<(Outline, Graph)> {
    let outline = Outline::new(comp)?;
    // How thick the shape is as a stroke: its area over half the length
    // of its outline.
    let (mut area_sum, mut around) = (0.0, 0.0);
    let mut bounds = crate::geom::Bounds::empty();
    for r in &outline.rings {
        area_sum += area(r);
        around += polyline_length(r) + r[r.len() - 1].dist(r[0]);
        r.iter().for_each(|&p| bounds.add(p));
    }
    let thick = 2.0 * area_sum / around.max(1.0);
    if thick <= 0.0 || thick > AUTO_MAX_WIDTH {
        return None;
    }
    let px = (thick / ACROSS)
        .clamp(PIXEL.0, PIXEL.1)
        .max((bounds.width() * bounds.height() / MAX_PIXELS).sqrt());
    let skeleton = |px: f64| -> Option<Graph> {
        let mut grid = Grid::new(&outline, px);
        grid.thin();
        // (A skeleton in this many pieces isn't lettering: a traced
        // texture, which would be a long time untangling to no end.)
        let mut graph = Graph::new(&grid, &outline).filter(|g| g.edges.len() <= TANGLE)?;
        graph.simplify(&outline);
        Some(graph)
    };
    let mut graph = skeleton(px)?;
    // A hairline only a pixel or two thick breaks up, or its skeleton
    // wanders: if a stroke is that thin, again, with smaller pixels.
    let thinnest = graph
        .edges
        .iter()
        .filter(|e| e.alive)
        .flat_map(|e| {
            let len = polyline_length(&e.line);
            [0.25, 0.5, 0.75].map(|t| outline.distance(point_along(&e.line, len * t)))
        })
        .fold(f64::INFINITY, f64::min);
    if thinnest < HAIRLINE * px {
        let floor = (bounds.width() * bounds.height() / MAX_PIXELS).sqrt();
        let finer = (thinnest / HAIRLINE).max(PIXEL.0 * 0.5).max(floor);
        if finer < px * 0.9 {
            graph = skeleton(finer)?;
        }
    }
    graph.bends(&outline);
    Some((outline, graph))
}

/// One way of reading a shape's skeleton as strokes, where it could be
/// read more than one way. [`columns`] tries them in order, the surest
/// first, and takes the first that makes every stroke a sound column.
#[derive(Clone, Copy, Debug, Default)]
struct Reading {
    /// A straight stroke that butts into the end of another is cut square
    /// across, not along the other's side (the bar of an e).
    square: bool,
    /// A cut along a stroke's side ends in the corner of a notch right
    /// by where it lands (the nick between a bowl and its stem's foot).
    notch: bool,
    /// The nicks in a column's sides are bridged (see [`bridged`]), and a
    /// stroke may swell further ([`SWELL_MOST`]).
    loose: bool,
    /// An 8 is two rings, one over the other.
    eight: bool,
    /// Read for strokes laid over each other ([`overlapping`]): a stub
    /// past a fork (a stem's end beyond where a bowl leaves it) is the end
    /// of the stroke it runs on from most nearly, however crooked, not
    /// left to its own.
    strokes: bool,
}

/// The satin columns for one filled component (its outer ring, then its
/// holes), a column a stroke: `None` if its strokes can't be told apart
/// this way, or a column's stitches wouldn't stay inside it.
///
/// And whether they were only to be had by one of the less sure readings
/// (see [`Reading`]).
pub(super) fn columns(comp: &[Polyline]) -> Option<(Vec<Rails>, bool)> {
    let (outline, graph) = skeleton_of(comp)?;
    // How many of the columns' stitches leave the shape.
    let astray = |columns: &[Rails]| -> usize {
        columns
            .iter()
            .map(|r| super::columns::astray(comp, r))
            .sum()
    };
    // An 8 as two rings first, if it is one and they come out as rings;
    // else, and anything else, as below.
    let with = |eight: bool| -> (Option<(Vec<Rails>, bool)>, bool) {
        let read = |square: bool, notch: bool, loose: bool| {
            let how = Reading {
                square,
                notch,
                loose,
                eight,
                strokes: false,
            };
            columns_read(&outline, &graph, how)
        };
        // With a stroke that butts into the end of another cut square
        // across where it can be; along the other's side instead, if that
        // can't be made into columns, sends more stitches out of the
        // shape or leaves the columns' stitches clearly less square to
        // them (a serif and the stroke it runs into, cut so that the
        // stroke ends along the top of both).
        let (first, squared, eights) = read(true, false, false);
        let found = match (first, squared) {
            (found, false) => found,
            (None, true) => read(false, false, false).0,
            (Some(square), true) => match read(false, false, false).0 {
                Some(along)
                    if astray(&along) < astray(&square)
                        || lean(&square) > SQUARE_LEAN.0 * lean(&along) + SQUARE_LEAN.1 =>
                {
                    Some(along)
                }
                _ => Some(square),
            },
        };
        // And failing that, the less sure readings: cuts ended in
        // notches, then columns let loose, then both.
        let found = found.map(|c| (c, false)).or_else(|| {
            [(true, false), (false, true), (true, true)]
                .into_iter()
                .flat_map(|(notch, loose)| [true, false].map(|square| (square, notch, loose)))
                .find_map(|(square, notch, loose)| Some((read(square, notch, loose).0?, true)))
        });
        (found, eights)
    };
    match with(true) {
        (None, true) => with(false).0,
        (found, _) => found,
    }
}

/// [`columns`], by one reading of the skeleton; whether any cut was made
/// square that wouldn't have been; and whether an 8 was taken for two
/// rings.
fn columns_read(
    outline: &Outline,
    graph: &Graph,
    how: Reading,
) -> (Option<Vec<Rails>>, bool, bool) {
    let Some((fate, cuts, squared, eights)) = resolve(graph, outline, how) else {
        return (None, false, false);
    };
    let strokes = strokes(graph, &fate);
    (
        columns_of(outline, &strokes, &cuts, how.loose),
        squared,
        eights,
    )
}

fn columns_of(
    outline: &Outline,
    strokes: &[Stroke],
    cuts: &[Cut],
    loose: bool,
) -> Option<Vec<Rails>> {
    let mut faces = faces(outline, cuts)?;
    if faces.len() != strokes.len() {
        return None;
    }
    // Which piece is which stroke's: the one its end is a cut of, or
    // (a stroke with a tip at each end) the one most of it lies in.
    let mut owner: Vec<Option<usize>> = vec![None; faces.len()];
    let mut out = Vec::new();
    for (s, stroke) in strokes.iter().enumerate() {
        let butt = stroke.ends.iter().find_map(|f| match f {
            Fate::Butt(c) => Some(*c),
            _ => None,
        });
        let face = match butt {
            Some(c) => faces.iter().position(|f| f.kind.contains(&Kind::End(c)))?,
            None => {
                let total = polyline_length(&stroke.line);
                let within = |f: &Face| {
                    (1..16)
                        .filter(|&k| f.inside(point_along(&stroke.line, total * k as f64 / 16.0)))
                        .count()
                };
                (0..faces.len()).max_by_key(|&f| within(&faces[f]))?
            }
        };
        if owner[face].replace(s).is_some() {
            return None;
        }
        out.push(faces[face].column(stroke, outline, loose)?);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nick_in_a_columns_side_is_bridged_and_a_corner_is_not() {
        let p = |x: f64, y: f64| P::new(x, y);
        // A side with a nick in it, 3 deep and 4 across, toward the other
        // side: straight across its mouth.
        let other = [p(0.0, 20.0), p(100.0, 20.0)];
        let loose = [
            p(0.0, 0.0),
            p(40.0, 0.0),
            p(48.0, 0.0),
            p(50.0, 3.0),
            p(52.0, 0.0),
            p(60.0, 0.0),
            p(100.0, 0.0),
        ];
        let side = bridged(&loose, &other);
        assert!(side.iter().all(|q| q.y < 0.5), "{side:?}");
        assert_eq!(
            (side[0], *side.last().unwrap()),
            (p(0.0, 0.0), p(100.0, 0.0))
        );
        // A bump out of the column, away from its other side, is its
        // shape: kept.
        let bumped: Vec<P> = loose.iter().map(|q| p(q.x, -q.y)).collect();
        assert_eq!(bridged(&bumped, &other), bumped);
        // And so is a corner the side turns: no fold, the way round it
        // hardly longer than across.
        let turned = [p(0.0, 0.0), p(50.0, 0.0), p(50.0, -40.0)];
        assert_eq!(bridged(&turned, &other), turned);
    }

    #[test]
    fn a_line_along_the_outline_is_not_a_cut_through_the_shape() {
        // A cut has to cross the material. One that lies along the
        // shape's own edge cuts nothing off, and whether its points are
        // in the shape or out of it is the last bit of their rounding:
        // the same letter was cut one way in one place on the design and
        // another way in another.
        let p = |x: f64, y: f64| P::new(x, y);
        for by in [
            p(0.0, 0.0),
            p(0.1, 0.7),
            p(1234.5678, -0.333),
            p(-7.7, 91.23),
        ] {
            let outline = Outline::new(&[vec![
                p(0.0, 0.0) + by,
                p(100.0, 0.0) + by,
                p(100.0, 30.0) + by,
                p(60.0, 31.0) + by,
                p(0.0, 30.0) + by,
            ]])
            .unwrap();
            // Across it, and corner to corner: through the material.
            assert!(outline.clear(p(20.0, 0.0) + by, p(20.0, 30.0 + 1.0 / 3.0) + by));
            assert!(outline.clear(p(0.0, 0.0) + by, p(100.0, 30.0) + by));
            // Along an edge, level or slanting, whole or in part: not.
            assert!(!outline.clear(p(0.0, 0.0) + by, p(100.0, 0.0) + by));
            assert!(!outline.clear(p(10.0, 0.0) + by, p(70.0, 0.0) + by));
            assert!(!outline.clear(p(60.0, 31.0) + by, p(0.0, 30.0) + by));
            assert!(!outline.clear(p(100.0, 30.0) + by, p(60.0, 31.0) + by));
        }
    }

    #[test]
    fn a_notch_is_found_by_where_a_cut_lands() {
        // A square with a nick in its bottom edge: the corner of the
        // nick, not the square's own corners (which turn the other way).
        let p = |x: f64, y: f64| P::new(x, y);
        let outline = Outline::new(&[vec![
            p(0.0, 0.0),
            p(100.0, 0.0),
            p(100.0, 100.0),
            p(55.0, 100.0),
            p(50.0, 90.0),
            p(45.0, 100.0),
            p(0.0, 100.0),
        ]])
        .unwrap();
        let found = outline
            .inside_corner(p(52.0, 99.0), 12.0)
            .expect("the nick");
        assert_eq!(found.p, p(50.0, 90.0));
        assert!(outline.inside_corner(p(98.0, 2.0), 12.0).is_none());
    }
}
