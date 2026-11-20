//! Satin columns for filled shapes that one column can't sew: letters with
//! a stem and a bowl, a bar across a stem, a counter (the hole in a P or an
//! e), a stroke that curls round on itself.
//!
//! A branching shape is cut where its branches part: at its sharp inside
//! corners, across to another such corner or straight on along an edge to
//! the outline opposite. Each cut either takes a piece off or opens a hole
//! onto the outline. A piece whose two ends are cuts is a column between
//! them; one with a single cut end runs from it to its tip. Cuts are tried
//! nearest-first and kept only if every piece left is a column whose
//! stitches stay inside it and aren't too wide; if no set of cuts does that
//! the caller fills the shape, as before. Columns that run straight or
//! curve are tried for first; ones that turn a sharp corner (their
//! stitches fanning round it) only if those won't do.
//!
//! A ring (an O) is one column round between its outline and its hole, but
//! not one whose two outlines turn corners out of step (a triangle with a
//! triangle cut out of it, a D): its stitches would lean, so it's cut at
//! its corners like any other shape.
//!
//! A stroke that only bends (a C, an S) has no such corners: its tips are
//! found as the pair of points that makes the narrowest column.

use super::satin::{AUTO_MAX_WIDTH, RING_LEAN, Rails, lean, pairs, split_at_tips};
use crate::geom::{FillRule, P, Polyline, cross, point_inside, polyline_length};

/// A corner turns at least this sharply (radians), measured a little way
/// either side of it: the flattened curves of a letter turn less.
const CORNER: f64 = 0.6;
/// How far either side of a vertex its turn is measured, units.
const WINDOW: f64 = 2.5;
/// A cut is a column's end when the outline turns at least this much at
/// both of its ends (else it lies along the column's side).
const END_TURN: f64 = 0.7;
/// A piece smaller than this (square units) isn't worth a column.
const MIN_AREA: f64 = 40.0;
/// How many cuts deep the search goes, and how many pieces it will try.
const MAX_DEPTH: usize = 7;
const BUDGET: usize = 600;
/// The same for a ring that would do as one column, its stitches leaning:
/// there's the ring to fall back on, so less looking for better is worth it.
const RING_BUDGET: usize = 60;
/// Cuts tried at each step, nearest first.
const TRIES: usize = 6;
/// Columns whose stitches lean less than this from square across them, on
/// average (degrees), are as good as columns get.
const SQUARE: f64 = 3.0;

/// A closed outline with the material on its left (an outer ring one way
/// round, a hole the other), and which of its edges are cuts.
#[derive(Clone, Debug)]
struct Ring {
    pts: Vec<P>,
    /// `cut[i]`: the edge from point `i` to the next is a cut.
    cut: Vec<bool>,
}

#[derive(Clone, Debug)]
struct Piece {
    outer: Ring,
    holes: Vec<Ring>,
}

/// What a piece is as one column (see [`Piece::column`]).
enum Found {
    None,
    /// A ring round its hole, if that's a column.
    Ring(Option<Rails>),
    /// A column between its ends, and whether its stitches stay inside
    /// the shape. (Whether it may turn corners is the asker's to say.)
    Open {
        rails: Rails,
        valid: bool,
    },
}

/// What a search has worked out about the pieces it has come to, by
/// [`Piece::key`]. Cutting a shape one way and then another leaves the
/// pieces that the other order does, and the search for columns that
/// turn corners goes over the ground of the one for columns that don't:
/// most pieces are met many times, and finding one's column (or that it
/// has none), its cuts, and what each cut leaves is nearly all the work.
#[derive(Default)]
struct Seen {
    columns: std::collections::HashMap<u64, Found>,
    cuts: std::collections::HashMap<u64, std::rc::Rc<Vec<(At, At)>>>,
    /// By piece and which of its cuts.
    pieces: std::collections::HashMap<(u64, usize), Option<std::rc::Rc<Vec<Piece>>>>,
}

pub(super) fn area(pts: &[P]) -> f64 {
    let n = pts.len();
    (0..n).map(|i| cross(pts[i], pts[(i + 1) % n])).sum::<f64>() * 0.5
}

impl Ring {
    /// `pts` without repeated points, turning the way that puts the
    /// material on the left (`outer`: anticlockwise in the plane's own
    /// sense; a hole, clockwise).
    fn new(pts: &[P], outer: bool) -> Option<Ring> {
        let mut p: Vec<P> = Vec::with_capacity(pts.len());
        for &q in pts {
            if p.last().is_none_or(|l| l.dist(q) > 1e-6) {
                p.push(q);
            }
        }
        while p.len() > 1 && p[0].dist(*p.last().unwrap()) <= 1e-6 {
            p.pop();
        }
        if p.len() < 3 {
            return None;
        }
        if (area(&p) > 0.0) != outer {
            p.reverse();
        }
        let n = p.len();
        Some(Ring {
            pts: p,
            cut: vec![false; n],
        })
    }

    fn len(&self) -> usize {
        self.pts.len()
    }

    /// The point `d` along the outline from point `i`, forward or back.
    fn along(&self, i: usize, d: f64, forward: bool) -> P {
        let n = self.len();
        let mut at = i;
        let mut left = d;
        for _ in 0..n {
            let next = if forward {
                (at + 1) % n
            } else {
                (at + n - 1) % n
            };
            let seg = self.pts[at].dist(self.pts[next]);
            if seg >= left && seg > 0.0 {
                return self.pts[at].lerp(self.pts[next], left / seg);
            }
            left -= seg;
            at = next;
        }
        self.pts[i]
    }

    /// How far the outline turns at point `i` (radians; negative: to the
    /// right, into the material: an inside corner), measured between
    /// points [`WINDOW`] either side so a corner rounded off over a few
    /// short edges still counts.
    fn turn(&self, i: usize) -> f64 {
        let v = self.pts[i];
        let (a, b) = (
            v - self.along(i, WINDOW, false),
            self.along(i, WINDOW, true) - v,
        );
        cross(a, b).atan2(a.x * b.x + a.y * b.y)
    }

    /// The outline's direction into and out of point `i` (unit vectors).
    fn directions(&self, i: usize) -> (P, P) {
        let v = self.pts[i];
        (
            (v - self.along(i, WINDOW, false)).norm(),
            (self.along(i, WINDOW, true) - v).norm(),
        )
    }

    /// Its sharp inside corners: the sharpest point of each.
    fn inside_corners(&self) -> Vec<usize> {
        let n = self.len();
        let turns: Vec<f64> = (0..n).map(|i| self.turn(i)).collect();
        let mut out = Vec::new();
        for i in 0..n {
            if turns[i] > -CORNER {
                continue;
            }
            // Not if a neighbour within the window turns more (the same
            // corner): the first of equals is the one kept.
            let mut sharpest = true;
            let mut d = 0.0;
            let mut k = i;
            while d < WINDOW {
                let next = (k + 1) % n;
                d += self.pts[k].dist(self.pts[next]);
                k = next;
                if k == i || d >= WINDOW {
                    break;
                }
                if turns[k] < turns[i] {
                    sharpest = false;
                }
            }
            let (mut d, mut k) = (0.0, i);
            while d < WINDOW {
                let prev = (k + n - 1) % n;
                d += self.pts[k].dist(self.pts[prev]);
                k = prev;
                if k == i || d >= WINDOW {
                    break;
                }
                if turns[k] <= turns[i] {
                    sharpest = false;
                }
            }
            if sharpest {
                out.push(i);
            }
        }
        out
    }
}

/// A place on a piece's outline: ring 0 is the outer one, ring k its hole
/// k − 1; the point is `t` of the way along the edge from point `edge`.
#[derive(Clone, Copy, Debug)]
struct At {
    ring: usize,
    edge: usize,
    t: f64,
    p: P,
}

impl Piece {
    fn ring(&self, k: usize) -> &Ring {
        if k == 0 {
            &self.outer
        } else {
            &self.holes[k - 1]
        }
    }

    fn rings(&self) -> impl Iterator<Item = &Ring> {
        std::iter::once(&self.outer).chain(&self.holes)
    }

    fn outlines(&self) -> Vec<Polyline> {
        self.rings().map(|r| r.pts.clone()).collect()
    }

    /// The edges of the outline that are the shape's own (not cuts).
    fn walls(&self) -> Vec<(P, P)> {
        let mut out = Vec::new();
        for r in self.rings() {
            let n = r.len();
            for i in 0..n {
                if !r.cut[i] {
                    out.push((r.pts[i], r.pts[(i + 1) % n]));
                }
            }
        }
        out
    }

    fn inside(&self, outlines: &[Polyline], p: P) -> bool {
        point_inside(outlines, FillRule::EvenOdd, p)
    }

    /// The first place a ray from `from` along `dir` meets the outline (not
    /// at `from` itself), within `reach`.
    fn hit(&self, from: P, dir: P, reach: f64) -> Option<At> {
        let mut best: Option<(f64, At)> = None;
        for (k, r) in self.rings().enumerate() {
            let n = r.len();
            for i in 0..n {
                let (a, b) = (r.pts[i], r.pts[(i + 1) % n]);
                let e = b - a;
                let den = cross(dir, e);
                if den.abs() < 1e-12 {
                    continue;
                }
                let w = a - from;
                let s = cross(w, e) / den; // along the ray
                let t = cross(w, dir) / den; // along the edge
                if s > 1e-3 && s <= reach && (-1e-9..=1.0 + 1e-9).contains(&t) {
                    let t = t.clamp(0.0, 1.0);
                    if best.is_none_or(|(bs, _)| s < bs) {
                        best = Some((
                            s,
                            At {
                                ring: k,
                                edge: i,
                                t,
                                p: a.lerp(b, t),
                            },
                        ));
                    }
                }
            }
        }
        best.map(|(_, at)| at)
    }

    /// Whether the straight line from `a` to `b` stays in the material,
    /// touching the outline only at its ends.
    fn clear(&self, outlines: &[Polyline], a: P, b: P) -> bool {
        let d = b - a;
        let len = d.len();
        if len < 1.0 {
            return false;
        }
        let dir = d.norm();
        // Nothing of the outline in the way…
        if self
            .hit(a, dir, len - 1e-3)
            .is_some_and(|h| h.p.dist(b) > 1e-3 && h.p.dist(a) > 1e-3)
        {
            return false;
        }
        // …and it's material, not a hole or outside, that it crosses.
        [0.25, 0.5, 0.75]
            .iter()
            .all(|&t| self.inside(outlines, a.lerp(b, t)))
    }

    /// The cuts worth trying, best first: each from a sharp inside corner,
    /// across to another, or on along one of the corner's edges (or
    /// between them) to the outline opposite.
    fn cuts(&self) -> Vec<(At, At)> {
        let outlines = self.outlines();
        let mut corners: Vec<At> = Vec::new();
        for (k, r) in self.rings().enumerate() {
            for i in r.inside_corners() {
                corners.push(At {
                    ring: k,
                    edge: i,
                    t: 0.0,
                    p: r.pts[i],
                });
            }
        }
        // (rank, length, from, to): corner to corner before corner to edge.
        let mut found: Vec<(u8, f64, At, At)> = Vec::new();
        for (x, a) in corners.iter().enumerate() {
            for b in &corners[x + 1..] {
                let len = a.p.dist(b.p);
                if len <= AUTO_MAX_WIDTH && self.clear(&outlines, a.p, b.p) {
                    found.push((0, len, *a, *b));
                }
            }
            let (into, out) = self.ring(a.ring).directions(a.edge);
            let between = (into - out).norm();
            for (rank, dir) in [(1, into), (1, -out), (2, between)] {
                if dir.len() < 0.5 {
                    continue;
                }
                let Some(h) = self.hit(a.p, dir, AUTO_MAX_WIDTH) else {
                    continue;
                };
                // (One that lands on a corner is that corner's own cut.)
                if corners.iter().any(|c| c.p.dist(h.p) < 1.5) {
                    continue;
                }
                let len = a.p.dist(h.p);
                if self.clear(&outlines, a.p, h.p) {
                    found.push((rank, len, *a, h));
                }
            }
        }
        found.sort_by(|x, y| (x.0, x.1).partial_cmp(&(y.0, y.1)).unwrap());
        // Much the same cut twice is one cut.
        let mut out: Vec<(At, At)> = Vec::new();
        for (_, _, a, b) in found {
            let same = |p: &(At, At)| {
                (p.0.p.dist(a.p) < 1.5 && p.1.p.dist(b.p) < 1.5)
                    || (p.0.p.dist(b.p) < 1.5 && p.1.p.dist(a.p) < 1.5)
            };
            if !out.iter().any(same) {
                out.push((a, b));
            }
        }
        out
    }

    /// The pieces a cut from `a` to `b` leaves: two, when both ends are on
    /// the outer outline; one with a hole fewer, when it joins a hole to
    /// the outer outline or to another hole. `None` for a cut that can't
    /// be made (both ends on one hole, or a piece too small to sew).
    fn cut(&self, a: At, b: At) -> Option<Vec<Piece>> {
        let mut piece = self.clone();
        // The places as points of their rings (new points where they fall
        // inside an edge; the later one on a ring first, so the earlier
        // one's number doesn't move).
        let put = |piece: &mut Piece, at: At| -> usize {
            let r = if at.ring == 0 {
                &mut piece.outer
            } else {
                &mut piece.holes[at.ring - 1]
            };
            let n = r.len();
            if at.t < 1e-6 || r.pts[at.edge].dist(at.p) < 1e-6 {
                return at.edge;
            }
            if at.t > 1.0 - 1e-6 || r.pts[(at.edge + 1) % n].dist(at.p) < 1e-6 {
                return (at.edge + 1) % n;
            }
            r.pts.insert(at.edge + 1, at.p);
            let tag = r.cut[at.edge];
            r.cut.insert(at.edge + 1, tag);
            at.edge + 1
        };
        let (ia, ib);
        if a.ring == b.ring {
            let a_later = (a.edge, a.t) > (b.edge, b.t);
            let (later, earlier) = if a_later { (a, b) } else { (b, a) };
            let mut hi = put(&mut piece, later);
            let before = piece.ring(a.ring).len();
            let lo = put(&mut piece, earlier);
            // (A point put in at or before the later one moves it on.)
            if piece.ring(a.ring).len() > before && lo <= hi {
                hi += 1;
            }
            (ia, ib) = if a_later { (hi, lo) } else { (lo, hi) };
        } else {
            ia = put(&mut piece, a);
            ib = put(&mut piece, b);
        }
        if a.ring == b.ring {
            if a.ring != 0 || ia == ib {
                return None;
            }
            let r = &piece.outer;
            let n = r.len();
            let (lo, hi) = (ia.min(ib), ia.max(ib));
            // lo … hi and back across the cut; hi … round … lo and back.
            let one = Ring {
                pts: r.pts[lo..=hi].to_vec(),
                cut: r.cut[lo..hi]
                    .iter()
                    .copied()
                    .chain(std::iter::once(true))
                    .collect(),
            };
            let two = Ring {
                pts: r.pts[hi..n].iter().chain(&r.pts[0..=lo]).copied().collect(),
                cut: r.cut[hi..n]
                    .iter()
                    .chain(&r.cut[0..lo])
                    .copied()
                    .chain(std::iter::once(true))
                    .collect(),
            };
            if one.len() < 3
                || two.len() < 3
                || area(&one.pts) < MIN_AREA
                || area(&two.pts) < MIN_AREA
            {
                return None;
            }
            let mut out = vec![
                Piece {
                    outer: one,
                    holes: Vec::new(),
                },
                Piece {
                    outer: two,
                    holes: Vec::new(),
                },
            ];
            for h in &piece.holes {
                let k = point_inside(
                    std::slice::from_ref(&out[0].outer.pts),
                    FillRule::EvenOdd,
                    h.pts[0],
                );
                out[if k { 0 } else { 1 }].holes.push(h.clone());
            }
            return Some(out);
        }
        // Two rings into one: round the first from the cut and back to it,
        // across, round the second, and back across.
        let (first, second, (ix, iy)) = if a.ring < b.ring {
            (a.ring, b.ring, (ia, ib))
        } else {
            (b.ring, a.ring, (ib, ia))
        };
        let (x, y) = (piece.ring(first).clone(), piece.ring(second).clone());
        let round = |r: &Ring, i: usize| -> (Vec<P>, Vec<bool>) {
            let n = r.len();
            let pts: Vec<P> = (0..=n).map(|k| r.pts[(i + k) % n]).collect();
            let cut: Vec<bool> = (0..n).map(|k| r.cut[(i + k) % n]).collect();
            (pts, cut)
        };
        let (mut pts, mut cuts) = round(&x, ix);
        cuts.push(true);
        let (p2, c2) = round(&y, iy);
        pts.extend(p2);
        cuts.extend(c2);
        cuts.push(true);
        let joined = Ring { pts, cut: cuts };
        piece.holes.remove(second - 1);
        if first == 0 {
            piece.outer = joined;
        } else {
            piece.holes[first - 1] = joined;
        }
        Some(vec![piece])
    }

    /// A fingerprint of the piece: every point of its outlines, and which
    /// of their edges are cuts.
    fn key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.holes.len().hash(&mut h);
        for r in self.rings() {
            r.pts.len().hash(&mut h);
            for p in &r.pts {
                p.x.to_bits().hash(&mut h);
                p.y.to_bits().hash(&mut h);
            }
            r.cut.hash(&mut h);
        }
        h.finish()
    }

    /// This piece as one column, if it is one: between its two cut ends,
    /// from its one cut end to its tip, between its own two tips, or (a
    /// ring) round between its outer outline and its hole.
    /// With `turns`, a column may turn sharp corners (its stitches fanning
    /// round each); without, only straight and curving columns will do.
    fn column(&self, turns: bool, seen: &mut Seen) -> Option<Rails> {
        let found = seen
            .columns
            .entry(self.key())
            .or_insert_with(|| self.found());
        match found {
            Found::None => None,
            Found::Ring(rails) => rails.clone(),
            Found::Open { rails, valid } => {
                if !turns && turns_corner(rails) {
                    return None;
                }
                valid.then(|| rails.clone())
            }
        }
    }

    /// What [`Piece::column`] goes by, whichever columns will do: the
    /// work of finding it, done once for a piece however often the
    /// search comes to it.
    fn found(&self) -> Found {
        let outlines = self.outlines();
        let walls = self.walls();
        match self.holes.len() {
            0 => match self.open_column(&outlines, &walls) {
                Some(rails) => {
                    let rails = squared(rails);
                    let valid = valid(&rails, &outlines, &walls);
                    Found::Open { rails, valid }
                }
                None => Found::None,
            },
            // (Not a ring whose stitches would lean, its outlines turning
            // corners out of step: that one is cut at the corners.)
            1 if self.rings().all(|r| !r.cut.contains(&true)) => Found::Ring(
                super::satin::auto_rails(&outlines)
                    .ok()
                    .filter(|r| lean(std::slice::from_ref(r)) <= RING_LEAN),
            ),
            _ => Found::None,
        }
    }

    fn open_column(&self, outlines: &[Polyline], walls: &[(P, P)]) -> Option<Rails> {
        let r = &self.outer;
        let n = r.len();
        // The cuts that are ends of the column: (first point, last point).
        // (Cut edges one after another are one cut while they run straight
        // on; where they turn a corner, two that meet there.)
        let dir = |a: usize, b: usize| (r.pts[b] - r.pts[a]).norm();
        let turn = |a: P, b: P| cross(a, b).atan2(a.x * b.x + a.y * b.y);
        let edge = |i: usize| dir(i, (i + 1) % n);
        let runs_on = |i: usize| {
            let next = (i + 1) % n;
            r.cut[i] && r.cut[next] && turn(edge(i), edge(next)).abs() < END_TURN
        };
        let mut ends: Vec<(usize, usize)> = Vec::new();
        for i in 0..n {
            if !r.cut[i] || runs_on((i + n - 1) % n) {
                continue;
            }
            let mut j = i;
            let mut steps = 0;
            while runs_on(j) {
                j = (j + 1) % n;
                steps += 1;
                if steps > n {
                    return None;
                }
            }
            // The cut is from point i to the point after edge j.
            let j = (j + 1) % n;
            // The outline turns onto the cut and off it again, or the cut
            // lies along the column's side.
            let before = edge((i + n - 1) % n);
            let after = edge(j);
            let face = (r.pts[j] - r.pts[i]).norm();
            if turn(before, face) > END_TURN && turn(face, after) > END_TURN {
                ends.push((i, j));
            }
        }
        // The outline from point `from` on to point `to`.
        let path = |from: usize, to: usize| -> Polyline {
            let mut out = vec![r.pts[from]];
            let mut k = from;
            while k != to {
                k = (k + 1) % n;
                out.push(r.pts[k]);
            }
            out
        };
        match ends.as_slice() {
            [] => tip_rails(&r.pts, outlines, walls),
            &[(i, j)] => {
                // From the cut's far point round to its near one, with the
                // column's other end somewhere between: a flat end (an edge
                // between two corners) or a point, whichever makes the
                // narrowest column.
                let whole = path(j, i);
                let rails = |a: &[P], b: &[P]| Rails {
                    a: a.to_vec(),
                    b: b.iter().rev().copied().collect(),
                    closed: false,
                    sync: Vec::new(),
                };
                let mut tries: Vec<Rails> = Vec::new();
                let m = whole.len();
                for k in 1..m.saturating_sub(2) {
                    let d = |a: usize, b: usize| (whole[b] - whole[a]).norm();
                    let turn = |a: P, b: P| cross(a, b).atan2(a.x * b.x + a.y * b.y);
                    if whole[k].dist(whole[k + 1]) <= AUTO_MAX_WIDTH
                        && turn(d(k - 1, k), d(k, k + 1)) > END_TURN
                        && turn(d(k, k + 1), d(k + 1, k + 2)) > END_TURN
                    {
                        tries.push(rails(&whole[..=k], &whole[k + 1..]));
                    }
                }
                let flat = tries.len();
                let step = (polyline_length(&whole) / 200.0).max(1.0);
                let pts = crate::geom::resample_keep_corners(&whole, step);
                for k in 1..pts.len().saturating_sub(1) {
                    tries.push(rails(&pts[..=k], &pts[k..]));
                }
                narrowest(tries, flat, outlines, walls)
            }
            &[(i, j), (k, l)] => {
                let a = path(j, k);
                let b: Polyline = path(l, i).into_iter().rev().collect();
                Some(Rails {
                    a,
                    b,
                    closed: false,
                    sync: Vec::new(),
                })
            }
            _ => None,
        }
    }
}

/// Where a ray from `from` along `dir` first meets the line `line`: how far
/// along the ray, and how far along the line.
pub(super) fn ray_on_line(from: P, dir: P, line: &[P]) -> Option<(f64, f64)> {
    let mut best: Option<(f64, f64)> = None;
    let mut arc = 0.0;
    for w in line.windows(2) {
        let e = w[1] - w[0];
        let len = e.len();
        let den = cross(dir, e);
        if den.abs() > 1e-12 {
            let d = w[0] - from;
            let s = cross(d, e) / den;
            let t = cross(d, dir) / den;
            if s > 1e-6 && (0.0..=1.0).contains(&t) && best.is_none_or(|(bs, _)| s < bs) {
                best = Some((s, arc + t * len));
            }
        }
        arc += len;
    }
    best
}

/// Rungs for a column that turns sharp corners, so its stitches stay
/// square to it along the straights and fan only round each corner: at an
/// inside corner, the places on the far rail square across from it, on the
/// way in and on the way out. (Without them each rail is followed in
/// proportion to its length, and a corner slants every stitch before it.)
fn corner_rungs(r: &Rails) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (near, far, swap) in [(&r.a, &r.b, false), (&r.b, &r.a, true)] {
        let mut arc = 0.0;
        for k in 1..near.len().saturating_sub(1) {
            arc += near[k - 1].dist(near[k]);
            let (d1, d2) = (
                (near[k] - near[k - 1]).norm(),
                (near[k + 1] - near[k]).norm(),
            );
            if cross(d1, d2).atan2(d1.x * d2.x + d1.y * d2.y).abs() < CORNER {
                continue;
            }
            // Square across from the corner, by the leg before it and by
            // the leg after: the nearer of the two sides each time (which
            // side, and how far along the far rail).
            let across = |d: P| -> Option<(P, f64)> {
                [P::new(-d.y, d.x), P::new(d.y, -d.x)]
                    .into_iter()
                    .filter_map(|n| ray_on_line(near[k], n, far).map(|(s, t)| (s, n, t)))
                    .filter(|(s, _, _)| *s <= AUTO_MAX_WIDTH * 1.2)
                    .min_by(|x, y| x.0.total_cmp(&y.0))
                    .map(|(_, n, t)| (n, t))
            };
            // The far rail's direction a little way along it.
            let far_dir = |t: f64| {
                let at = |s: f64| -> P {
                    let mut gone = 0.0;
                    for w in far.windows(2) {
                        let d = w[0].dist(w[1]);
                        if gone + d >= s {
                            return w[0].lerp(w[1], if d > 0.0 { (s - gone) / d } else { 0.0 });
                        }
                        gone += d;
                    }
                    *far.last().unwrap()
                };
                (at(t + 2.0) - at((t - 2.0).max(0.0))).norm()
            };
            if let (Some((n1, t1)), Some((_, t2))) = (across(d1), across(d2))
                && t2 > t1 + 1.0
                // An inside corner: the rail turns away from the far rail.
                // (A corner of the column's own end turns toward it.)
                && cross(d1, n1) * cross(d1, d2) < 0.0
                // And the column turns there: the far rail too, between the
                // two places across from the corner. (Not where the near
                // rail only bulges toward it and back, a counter running
                // into a stem: that side straight, the stitches square to
                // it.)
                && {
                    let (u, v) = (far_dir(t1), far_dir(t2));
                    cross(u, v).atan2(u.dot(v)).abs() > CORNER * 0.5
                }
            {
                let pair = |near_arc: f64, far_arc: f64| {
                    if swap {
                        (far_arc, near_arc)
                    } else {
                        (near_arc, far_arc)
                    }
                };
                out.push(pair(arc - 0.25, t1));
                out.push(pair(arc + 0.25, t2));
            }
        }
    }
    out.sort_by(|x, y| x.0.total_cmp(&y.0));
    // In step along both rails, or not a rung.
    let mut kept: Vec<(f64, f64)> = Vec::new();
    for (a, b) in out {
        if kept.last().is_none_or(|l| a > l.0 + 0.1 && b > l.1 + 0.1) {
            kept.push((a, b));
        }
    }
    kept
}

/// `rails` with the rungs its corners want.
fn squared(mut rails: Rails) -> Rails {
    rails.sync = corner_rungs(&rails);
    rails
}

/// Whether a column's stitches stay inside the shape, aren't too wide and
/// each cross one stroke of it: what an automatic satin has to be.
/// (`walls`: the shape's own outline, without the cuts made in it.)
pub(super) fn valid(rails: &Rails, outlines: &[Polyline], walls: &[(P, P)]) -> bool {
    sewable(rails, outlines, walls, 0, 12.0)
}

/// [`valid`], for a column known to be one stroke. It forgives `spare`
/// stitches that span a notch besides the few any column may have: a
/// stroke that ends in a ball has one where the ball joins it, and the
/// stitches across the ball pass it, however short the stroke. And its
/// stitches may be as long as `swell` (units) however thin it is elsewhere:
/// a hairline widens where it runs into a point or takes a corner.
pub(super) fn sewable(
    rails: &Rails,
    outlines: &[Polyline],
    walls: &[(P, P)],
    spare: usize,
    swell: f64,
) -> bool {
    let ps = pairs(rails, 8.0);
    if ps.len() < 2 {
        return false;
    }
    let mut bad = 0;
    // How far each stitch's middle is from the shape's outline (for the
    // stitches long enough for it to matter): see below.
    let mut nears: Vec<Option<f64>> = Vec::with_capacity(ps.len());
    let mut widths: Vec<f64> = Vec::with_capacity(ps.len());
    let last = ps.len() - 1;
    for (k, &(a, b)) in ps.iter().enumerate() {
        let w = a.dist(b);
        if w > AUTO_MAX_WIDTH {
            return false;
        }
        widths.push(w);
        // (The first and last stitch lie along the column's ends: on the
        // outline, neither in nor out.)
        if k > 0
            && k < last
            && w > 1.0
            && [0.25, 0.5, 0.75]
                .iter()
                .any(|&t| !point_inside(outlines, FillRule::EvenOdd, a.lerp(b, t)))
        {
            bad += 1;
        }
        nears.push((w > 6.0).then(|| {
            let mid = a.lerp(b, 0.5);
            walls
                .iter()
                .map(|&(p, q)| {
                    let e = q - p;
                    let len2 = e.x * e.x + e.y * e.y;
                    let t = if len2 > 0.0 {
                        (((mid.x - p.x) * e.x + (mid.y - p.y) * e.y) / len2).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    p.lerp(q, t).dist(mid)
                })
                .fold(f64::INFINITY, f64::min)
        }));
    }
    // A column is about as wide all along: a stitch several times the
    // usual length is one side doubling back on itself (round a branch
    // that should have been cut off), not a wide place.
    let mut sorted = widths.clone();
    sorted.sort_by(f64::total_cmp);
    let (median, widest) = (sorted[sorted.len() / 2], sorted[sorted.len() - 1]);
    if widest > (median * 3.0).max(swell) {
        return false;
    }
    // A stitch across a stroke is as long as the stroke is thick, so its
    // middle is half its length from the outline. One whose middle is much
    // nearer the outline than that spans two strokes and the notch between
    // them (a tail and the stem it leaves): not a column. (Not counting
    // the stitches within half a width of the column's own ends, which
    // are that near the outline there.)
    let margin = (median * 0.5 / 8.0).ceil() as usize + 1;
    let spans = (margin..ps.len().saturating_sub(margin))
        .filter(|&k| nears[k].is_some_and(|near| widths[k] > near * 3.6))
        .count();
    if spans.saturating_sub(spare) as f64 > ps.len() as f64 * 0.1 {
        return false;
    }
    (bad as f64) <= ps.len() as f64 * 0.06
}

/// The narrowest of `tries` that's a column. The first `flat` of them end
/// square (not at a point): preferred unless a pointed one is clearly
/// narrower.
fn narrowest(
    tries: Vec<Rails>,
    flat: usize,
    outlines: &[Polyline],
    walls: &[(P, P)],
) -> Option<Rails> {
    let mut ranked: Vec<(f64, usize)> = tries
        .iter()
        .enumerate()
        .map(|(k, rails)| {
            let (la, lb) = (polyline_length(&rails.a), polyline_length(&rails.b));
            // (Sides too unlike each other aren't the two sides of a column.)
            if la.min(lb) < la.max(lb) * 0.45 {
                return (f64::INFINITY, k);
            }
            let ps = pairs(rails, (la.max(lb) / 24.0).max(1.0));
            let w = ps.iter().map(|(a, b)| a.dist(*b)).sum::<f64>() / ps.len().max(1) as f64;
            (if k < flat { w * 0.8 } else { w }, k)
        })
        .collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut tries: Vec<Option<Rails>> = tries.into_iter().map(Some).collect();
    ranked
        .into_iter()
        .take(8)
        .filter(|(w, _)| w.is_finite())
        .filter_map(|(_, k)| tries[k].take())
        .map(squared)
        .find(|rails| valid(rails, outlines, walls))
}

/// A column for a ring with no cuts: split at its tips as ever, or (a
/// stroke that curls round, whose tips are near each other) at the pair of
/// points that makes the narrowest column.
fn tip_rails(ring: &[P], outlines: &[Polyline], walls: &[(P, P)]) -> Option<Rails> {
    if let Some(r) = split_at_tips(ring).filter(|r| valid(r, outlines, walls)) {
        return Some(r);
    }
    let mut closed = ring.to_vec();
    closed.push(ring[0]);
    let step = (polyline_length(&closed) / 96.0).max(1.0);
    let mut pts = crate::geom::resample_keep_corners(&closed, step);
    pts.pop();
    let n = pts.len();
    if n < 8 {
        return None;
    }
    let rails_for = |i: usize, j: usize| -> Rails {
        let a: Polyline = (i..=j).map(|k| pts[k]).collect();
        let b: Polyline = (0..=(n - j + i)).map(|k| pts[(i + n - k) % n]).collect();
        Rails {
            a,
            b,
            closed: false,
            sync: Vec::new(),
        }
    };
    let mut ranked: Vec<(f64, usize, usize)> = Vec::new();
    for i in 0..n {
        for j in i + 2..n {
            let rails = rails_for(i, j);
            let (la, lb) = (polyline_length(&rails.a), polyline_length(&rails.b));
            if la.min(lb) < la.max(lb) * 0.45 {
                continue;
            }
            let ps = pairs(&rails, la.max(lb) / 12.0);
            let w = ps.iter().map(|(a, b)| a.dist(*b)).sum::<f64>() / ps.len().max(1) as f64;
            ranked.push((w, i, j));
        }
    }
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
    ranked
        .into_iter()
        .take(12)
        .map(|(_, i, j)| rails_for(i, j))
        .find(|rails| valid(rails, outlines, walls))
}

/// The columns `piece` can be sewn as, cutting it up to `depth` more times
/// (`turns`: see [`Piece::column`]).
fn solve(
    piece: &Piece,
    depth: usize,
    budget: &mut usize,
    turns: bool,
    seen: &mut Seen,
) -> Option<Vec<Rails>> {
    if *budget == 0 {
        return None;
    }
    *budget -= 1;
    if let Some(rails) = piece.column(turns, seen) {
        return Some(vec![rails]);
    }
    if depth == 0 {
        return None;
    }
    let key = piece.key();
    let cuts = seen
        .cuts
        .entry(key)
        .or_insert_with(|| std::rc::Rc::new(piece.cuts()))
        .clone();
    let mut tried = 0;
    for (k, &(a, b)) in cuts.iter().enumerate() {
        let Some(pieces) = seen
            .pieces
            .entry((key, k))
            .or_insert_with(|| piece.cut(a, b).map(std::rc::Rc::new))
            .clone()
        else {
            continue;
        };
        tried += 1;
        if tried > TRIES {
            break;
        }
        let mut all = Vec::new();
        let mut ok = true;
        for p in pieces.iter() {
            match solve(p, depth - 1, budget, turns, seen) {
                Some(rails) => all.extend(rails),
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            return Some(all);
        }
    }
    None
}

/// Whether a column turns a corner: one of its corner rungs (see
/// [`corner_rungs`]) is away from its ends. A kink in an outline within
/// [`END_KINK`] of the end of a column (where a cut meets it, a point or
/// two from a corner of the shape's own) is part of the end, not a
/// corner the column turns.
fn turns_corner(r: &Rails) -> bool {
    let (la, lb) = (polyline_length(&r.a), polyline_length(&r.b));
    r.sync
        .iter()
        .any(|&(sa, sb)| sa > END_KINK && sa < la - END_KINK && sb > END_KINK && sb < lb - END_KINK)
}

/// See [`turns_corner`] (units).
const END_KINK: f64 = 4.0;

/// Whether the one column a shape was taken for (see
/// [`super::satin::auto_rails`]) is fit to sew as it is: its stitches each
/// cross one stroke, and none crosses the next. A V passes for a column
/// between its two serifs, its stitches fanning across each and crossing
/// one another at its foot: that one is better cut into its strokes (see
/// [`split`]), if it can be.
pub fn sound(comp: &[Polyline], rails: &Rails) -> bool {
    if rails.closed {
        return true;
    }
    let walls: Vec<(P, P)> = comp
        .iter()
        .flat_map(|ring| (0..ring.len()).map(move |i| (ring[i], ring[(i + 1) % ring.len()])))
        .collect();
    valid(rails, comp, &walls) && !super::skeleton::crossed(rails)
}

/// Whether a column of the shape still sews with its ends put where the
/// shape's are (see [`super::satin::flat_ends`]): its stitches inside the
/// shape (no more of them astray than `was`, the column before), and none
/// crossing the next.
pub fn holds(comp: &[Polyline], rails: &Rails, was: &Rails) -> bool {
    let walls: Vec<(P, P)> = comp
        .iter()
        .flat_map(|ring| (0..ring.len()).map(move |i| (ring[i], ring[(i + 1) % ring.len()])))
        .collect();
    sewable(rails, comp, &walls, 3, 20.0)
        && !super::skeleton::crossed(rails)
        && astray(comp, rails) <= astray(comp, was)
}

/// A stitch no further than this outside a shape's outline is on it,
/// for counting those that leave it (units).
const ON_EDGE: f64 = 0.5;

/// How many of a column's stitches leave the shape.
pub(crate) fn astray(comp: &[Polyline], rails: &Rails) -> usize {
    astray_in(comp, FillRule::EvenOdd, rails)
}

/// How many of a column's stitches are more than 30° from square to the
/// shape's own edge where they end on it (to both edges, taken together,
/// where both ends do): the stitches that lean, as the eye judges them,
/// against the stroke as it's drawn. Not against the column's middle
/// line, which bends toward a cut where two strokes are cut apart on a
/// mitre. (Nor its last stitch, lying along its end, nor the short ones
/// into a point.)
pub(crate) fn off_square(rings: &[Polyline], rails: &Rails) -> usize {
    // The outline's edge nearest a point: how far, and which way it runs.
    let edge = |p: P| -> (f64, P) {
        let mut best = (f64::INFINITY, P::new(1.0, 0.0));
        for ring in rings {
            for i in 0..ring.len() {
                let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                if a.dist(b) < 1e-9 {
                    continue;
                }
                let d = super::skeleton::nearest_on(a, b, p).1.dist(p);
                if d < best.0 {
                    best = (d, (b - a).norm());
                }
            }
        }
        best
    };
    pairs(rails, 3.0)
        .iter()
        .filter(|&&(a, b)| {
            let across = b - a;
            if across.len() < 10.0 || edge(a.lerp(b, 0.5)).0 <= 1.0 {
                return false;
            }
            let ((da, ta), (db, tb)) = (edge(a), edge(b));
            let along = match (da <= 1.0, db <= 1.0) {
                (true, true) => ta + if ta.dot(tb) < 0.0 { -tb } else { tb },
                (true, false) => ta,
                (false, true) => tb,
                (false, false) => return false,
            };
            along.len() > 1e-6 && along.norm().dot(across.norm()).abs() > 0.5
        })
        .count()
}

/// [`astray`], in a shape of several parts (`rings`, filled by `rule`).
///
/// A stitch lying along the outline hasn't left the shape: the last
/// stitches of a column that runs out to a flat end lie along that end.
/// (Which side of the line rounding puts them is chance: counted, the
/// same letter would be cut one way here and another way there.)
pub(crate) fn astray_in(rings: &[Polyline], rule: FillRule, rails: &Rails) -> usize {
    let off = |p: P| {
        !point_inside(rings, rule, p)
            && rings.iter().all(|ring| {
                (0..ring.len()).all(|i| {
                    let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                    super::skeleton::nearest_on(a, b, p).1.dist(p) > ON_EDGE
                })
            })
    };
    let ps = pairs(rails, 4.0);
    ps.iter()
        .skip(1)
        .take(ps.len().saturating_sub(2))
        .filter(|(a, b)| a.dist(*b) > 1.0 && [0.25, 0.5, 0.75].iter().any(|&t| off(a.lerp(*b, t))))
        .count()
}

/// The component's strokes as whole columns overlapping where they meet,
/// for a satin set to overlap them: see [`super::skeleton::overlapping`].
pub fn overlapping(comp: &[Polyline]) -> Option<Vec<Rails>> {
    super::skeleton::overlapping(comp)
}

/// The satin columns for one filled component (its outer ring, then its
/// holes) that isn't a single column: `None` if it can't be made into
/// columns whose stitches stay inside it.
pub fn split(comp: &[Polyline]) -> Option<Vec<Rails>> {
    (|| {
        let outer = Ring::new(comp.first()?, true)?;
        let holes: Vec<Ring> = comp[1..]
            .iter()
            .filter_map(|h| Ring::new(h, false))
            .collect();
        // Straight and curving columns, butted where they meet, if the
        // shape can be cut into those: cleaner than a column turning a
        // sharp corner. Else columns that turn corners too.
        let piece = Piece { outer, holes };
        let tries = if piece.holes.len() == 1 && super::satin::auto_rails(comp).is_ok() {
            RING_BUDGET
        } else {
            BUDGET
        };
        let mut seen = Seen::default();
        let mut cut = |turns: bool| {
            let mut budget = tries;
            let first = solve(&piece, MAX_DEPTH, &mut budget, turns, &mut seen);
            // The search takes the first way of cutting the shape that
            // works, trying the shortest cuts first, and that needn't be
            // the best. So it starts again from each of the first few
            // cuts it could make, each with a search of its own, and the
            // squarest result is taken.
            let mut best = first.map(|c| (lean(&c), c));
            if best.as_ref().is_some_and(|(l, _)| *l < SQUARE) {
                return best.map(|(_, c)| c);
            }
            let cuts = piece.cuts();
            for &(a, b) in cuts.iter().take(TRIES) {
                let Some(pieces) = piece.cut(a, b) else {
                    continue;
                };
                let mut budget = tries;
                let mut all = Vec::new();
                let whole = pieces.iter().all(|p| {
                    solve(p, MAX_DEPTH - 1, &mut budget, turns, &mut seen)
                        .map(|rails| all.extend(rails))
                        .is_some()
                });
                if !whole {
                    continue;
                }
                let l = lean(&all);
                if best.as_ref().is_none_or(|(bl, _)| l < *bl) {
                    best = Some((l, all));
                }
            }
            best.map(|(_, c)| c)
        };
        // Cut at its corners, if that leaves columns that run straight or
        // curve; else, or if the columns lean (a letter with brackets for
        // corners: the cuts that could be found weren't where its strokes
        // meet), a column a stroke of its skeleton, where those are
        // squarer; and failing both, columns that turn corners.
        let cornered = cut(false);
        let leaning = cornered.as_deref().map(lean);
        if leaning.is_some_and(|l| l < SQUARE) {
            return cornered;
        }
        // (And no more of their stitches off the shape, give or take one:
        // a stroke's column may span a nick in the outline that the cut
        // pieces stop at.)
        let off = |c: &[Rails]| c.iter().map(|r| astray(comp, r)).sum::<usize>();
        let stroked = super::skeleton::columns(comp).filter(|(c, _)| {
            leaning.is_none_or(|l| lean(c) < l * 0.7)
                && cornered.as_deref().is_none_or(|was| off(c) <= off(was) + 1)
        });
        match stroked {
            Some((columns, false)) => Some(columns),
            // (Strokes only to be had by one of the less sure readings:
            // held to columns that turn corners, likewise.)
            Some((columns, true)) => match cornered.or_else(|| cut(true)) {
                Some(was) if off(&columns) > off(&was) + 1 => Some(was),
                _ => Some(columns),
            },
            None => cornered.or_else(|| cut(true)),
        }
    })()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poly(pts: &[(f64, f64)]) -> Polyline {
        pts.iter().map(|&(x, y)| P::new(x, y)).collect()
    }

    /// Every column's stitches inside the shape, and the columns together
    /// covering about all of it.
    fn covers(comp: &[Polyline], rails: &[Rails]) {
        let total: f64 =
            area(&comp[0]).abs() - comp[1..].iter().map(|h| area(h).abs()).sum::<f64>();
        let mut sewn = 0.0;
        for r in rails {
            let walls: Vec<(P, P)> = comp
                .iter()
                .flat_map(|ring| {
                    (0..ring.len()).map(move |i| (ring[i], ring[(i + 1) % ring.len()]))
                })
                .collect();
            assert!(valid(r, comp, &walls), "a column leaves the shape");
            let ring: Polyline = r.a.iter().chain(r.b.iter().rev()).copied().collect();
            sewn += area(&ring).abs();
        }
        assert!(
            (sewn - total).abs() < total * 0.03,
            "columns cover {sewn} of {total}"
        );
    }

    #[test]
    fn a_t_is_a_bar_and_a_stem() {
        // Bar 100 × 16 on top, stem 16 wide below its middle.
        let t = vec![poly(&[
            (0.0, 0.0),
            (100.0, 0.0),
            (100.0, 16.0),
            (58.0, 16.0),
            (58.0, 120.0),
            (42.0, 120.0),
            (42.0, 16.0),
            (0.0, 16.0),
        ])];
        assert!(
            super::super::satin::auto_rails(&t).is_err(),
            "not one column"
        );
        let rails = split(&t).expect("columns");
        assert_eq!(rails.len(), 2);
        covers(&t, &rails);
        // One runs the bar's length, the other the stem's.
        let spans: Vec<(f64, f64)> = rails
            .iter()
            .map(|r| {
                let xs = r.a.iter().chain(&r.b).map(|p| p.x);
                let ys = r.a.iter().chain(&r.b).map(|p| p.y);
                (
                    xs.clone().fold(f64::MIN, f64::max) - xs.fold(f64::MAX, f64::min),
                    ys.clone().fold(f64::MIN, f64::max) - ys.fold(f64::MAX, f64::min),
                )
            })
            .collect();
        assert!(spans.contains(&(100.0, 16.0)), "{spans:?}");
        assert!(spans.contains(&(16.0, 104.0)), "{spans:?}");
    }

    #[test]
    fn a_column_turns_a_corner_only_where_both_its_sides_do() {
        // An L, 3 mm wide: its inner side turns at (30, 100), its outer at
        // (0, 130). Square across from the inner corner on the way in and
        // on the way out are two places on the outer side.
        let l = Rails {
            a: poly(&[(0.0, 0.0), (0.0, 130.0), (100.0, 130.0)]),
            b: poly(&[(30.0, 0.0), (30.0, 100.0), (100.0, 100.0)]),
            closed: false,
            sync: Vec::new(),
        };
        let rungs = corner_rungs(&l);
        assert_eq!(rungs.len(), 2, "{rungs:?}");
        assert!(turns_corner(&squared(l)));
        // A stem whose inner side bulges into it and back (a counter
        // running into it), its outer side straight: no corner.
        let stem = Rails {
            a: poly(&[(0.0, 0.0), (0.0, 200.0)]),
            b: poly(&[
                (30.0, 0.0),
                (30.0, 80.0),
                (20.0, 100.0),
                (30.0, 120.0),
                (30.0, 200.0),
            ]),
            closed: false,
            sync: Vec::new(),
        };
        assert!(corner_rungs(&stem).is_empty(), "{:?}", corner_rungs(&stem));
        // Nor is a kink where a column's side meets its end (a cut a point
        // from a corner of the shape) a corner it turns.
        let kinked = Rails {
            a: poly(&[(0.0, 0.0), (0.0, 200.0)]),
            b: poly(&[(30.0, 0.0), (30.0, 197.0), (28.0, 200.0)]),
            closed: false,
            sync: vec![(196.5, 196.75), (197.5, 197.25)],
        };
        assert!(!turns_corner(&kinked));
        assert!(turns_corner(&Rails {
            sync: vec![(100.0, 100.0)],
            ..kinked.clone()
        }));
    }

    #[test]
    fn a_p_is_a_stem_and_a_bowl_round_its_counter() {
        // Stem 16 wide and 140 tall; a square bowl to its right, 16 thick.
        let outer = poly(&[
            (0.0, 0.0),
            (80.0, 0.0),
            (80.0, 76.0),
            (16.0, 76.0),
            (16.0, 140.0),
            (0.0, 140.0),
        ]);
        let counter = poly(&[(16.0, 16.0), (64.0, 16.0), (64.0, 60.0), (16.0, 60.0)]);
        let p = vec![outer, counter];
        assert!(super::super::satin::auto_rails(&p).is_err());
        let rails = split(&p).expect("columns");
        covers(&p, &rails);
        assert!(rails.len() >= 2 && rails.len() <= 4, "{}", rails.len());
        // No stitch wider than the strokes are thick (give or take a mitre).
        for r in &rails {
            let widest = pairs(r, 5.0)
                .iter()
                .map(|(a, b)| a.dist(*b))
                .fold(0.0, f64::max);
            assert!(widest < 16.0 * 1.6, "{widest}");
        }
    }

    #[test]
    fn an_e_with_three_bars_is_four_columns() {
        let e = vec![poly(&[
            (0.0, 0.0),
            (70.0, 0.0),
            (70.0, 16.0),
            (16.0, 16.0),
            (16.0, 52.0),
            (60.0, 52.0),
            (60.0, 68.0),
            (16.0, 68.0),
            (16.0, 104.0),
            (70.0, 104.0),
            (70.0, 120.0),
            (0.0, 120.0),
        ])];
        let rails = split(&e).expect("columns");
        covers(&e, &rails);
        assert_eq!(rails.len(), 4, "a stem and three bars");
    }

    #[test]
    fn a_stroke_that_curls_round_is_one_column_between_its_ends() {
        // A C: three quarters of a ring 12 wide, its ends near each other.
        let mut c: Polyline = Vec::new();
        let (from, to) = (0.6f64, std::f64::consts::TAU - 0.6);
        for k in 0..=60 {
            let t = from + (to - from) * k as f64 / 60.0;
            c.push(P::new(50.0 * t.cos(), 50.0 * t.sin()));
        }
        for k in (0..=60).rev() {
            let t = from + (to - from) * k as f64 / 60.0;
            c.push(P::new(38.0 * t.cos(), 38.0 * t.sin()));
        }
        let c = vec![c];
        let rails = split(&c).expect("a column");
        assert_eq!(rails.len(), 1);
        covers(&c, &rails);
        let mean = {
            let ps = pairs(&rails[0], 5.0);
            ps.iter().map(|(a, b)| a.dist(*b)).sum::<f64>() / ps.len() as f64
        };
        assert!((mean - 12.0).abs() < 1.0, "as wide as the stroke: {mean}");
    }

    #[test]
    fn a_blob_is_still_not_a_satin() {
        // 30 mm across: wider than any satin stitch should be.
        let blob: Polyline = (0..40)
            .map(|k| {
                let t = k as f64 / 40.0 * std::f64::consts::TAU;
                P::new(150.0 * t.cos(), 150.0 * t.sin())
            })
            .collect();
        assert!(split(&[blob]).is_none());
    }

    /// A 30 mm triangle with a triangle cut out of it, its stroke 3 mm
    /// thick (a Δ).
    fn delta() -> Vec<Polyline> {
        vec![
            poly(&[(0.0, 0.0), (300.0, 0.0), (150.0, -259.8)]),
            poly(&[(51.96, -30.0), (248.04, -30.0), (150.0, -199.8)]),
        ]
    }

    #[test]
    fn a_triangle_round_a_triangle_is_three_columns_not_a_leaning_ring() {
        let delta = delta();
        // Round between the two outlines its stitches stay inside it, so
        // that was taken for a column: but they point at its middle, leaning
        // more the nearer a corner.
        let ring = super::super::satin::auto_rails(&delta).expect("a ring");
        let leaning = lean(std::slice::from_ref(&ring));
        assert!(leaning > RING_LEAN, "the ring leans {leaning}°");
        let rails = split(&delta).expect("columns");
        assert_eq!(rails.len(), 3, "a column a side");
        assert!(rails.iter().all(|r| !r.closed));
        covers(&delta, &rails);
        let now = lean(&rails);
        assert!(
            now < leaning * 0.5,
            "{now}° across, where the ring's {leaning}°"
        );
    }

    /// A polygon with its corners rounded: each point, and the radius of
    /// the arc that takes its place (0: left sharp). Inside corners
    /// become brackets, outside ones round ends.
    fn rounded(pts: &[(f64, f64, f64)]) -> Polyline {
        let n = pts.len();
        let mut out = Vec::new();
        for i in 0..n {
            let at = |k: usize| P::new(pts[k % n].0, pts[k % n].1);
            let (before, v, after) = (at(i + n - 1), at(i), at(i + 1));
            let r = pts[i].2;
            if r <= 0.0 {
                out.push(v);
                continue;
            }
            let (u, w) = ((before - v).norm(), (after - v).norm());
            let half = (u.dot(w).clamp(-1.0, 1.0).acos()) * 0.5;
            let centre = v + (u + w).norm() * (r / half.sin());
            let (from, to) = (v + u * (r / half.tan()), v + w * (r / half.tan()));
            let (a, b) = (from - centre, to - centre);
            let sweep = cross(a, b).atan2(a.dot(b));
            for k in 0..=8 {
                let t = sweep * k as f64 / 8.0;
                out.push(centre + a.rotate(t.cos(), t.sin()));
            }
        }
        out
    }

    /// Where a column's points lie: (least x, least y, most x, most y).
    fn extent(r: &Rails) -> (f64, f64, f64, f64) {
        r.a.iter().chain(&r.b).fold(
            (f64::MAX, f64::MAX, f64::MIN, f64::MIN),
            |(x0, y0, x1, y1), p| (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)),
        )
    }

    /// How far a column's stitches run across (x) and up and down (y), on
    /// average: which way they lie.
    fn lie(r: &Rails) -> (f64, f64) {
        let ps = pairs(r, 4.0);
        let n = ps.len() as f64;
        ps.iter().fold((0.0, 0.0), |(x, y), (a, b)| {
            (x + (b.x - a.x).abs() / n, y + (b.y - a.y).abs() / n)
        })
    }

    /// Every column fit to sew, none with crossed stitches, and together
    /// covering the shape (but for the slivers past the last stitch of a
    /// round end).
    fn sews(comp: &[Polyline], rails: &[Rails]) {
        let total: f64 =
            area(&comp[0]).abs() - comp[1..].iter().map(|h| area(h).abs()).sum::<f64>();
        let mut sewn = 0.0;
        for r in rails {
            assert!(!super::super::skeleton::crossed(r), "crossed stitches");
            assert!(
                pairs(r, 4.0)
                    .iter()
                    .all(|(a, b)| a.dist(*b) <= AUTO_MAX_WIDTH),
                "a stitch too long"
            );
            let ring: Polyline = r.a.iter().chain(r.b.iter().rev()).copied().collect();
            sewn += area(&ring).abs();
        }
        assert!(
            (sewn - total).abs() < total * 0.05,
            "columns cover {sewn} of {total}"
        );
    }

    /// A stem 16 wide standing on a slab serif 12 thick, the corners
    /// between them bracketed and the serif's ends round.
    fn stem_on_slab() -> Vec<Polyline> {
        vec![rounded(&[
            (42.0, 0.0, 0.0),
            (58.0, 0.0, 0.0),
            (58.0, 100.0, 6.0),
            (90.0, 100.0, 4.0),
            (90.0, 112.0, 4.0),
            (10.0, 112.0, 4.0),
            (10.0, 100.0, 4.0),
            (42.0, 100.0, 6.0),
        ])]
    }

    #[test]
    fn a_stem_stands_on_its_slab_serif() {
        let t = stem_on_slab();
        // No sharp inside corner to cut it at: it was filled.
        assert!(super::super::satin::auto_rails(&t).is_err());
        let rails = split(&t).expect("columns");
        assert_eq!(rails.len(), 2, "a stem and a serif");
        sews(&t, &rails);
        // The serif is a column of its own right across the foot, its
        // stitches lying the way the stem runs; the stem stops on top of it.
        let serif = rails
            .iter()
            .find(|r| extent(r).2 - extent(r).0 > 70.0)
            .expect("the serif");
        let (across, along) = lie(serif);
        assert!(
            along > 10.0 && across < 2.0,
            "{across} across, {along} along"
        );
        let stem = rails
            .iter()
            .find(|r| extent(r).3 - extent(r).1 > 90.0)
            .expect("the stem");
        let (across, along) = lie(stem);
        assert!(
            across > 14.0 && along < 2.0,
            "{across} across, {along} along"
        );
        assert!(extent(stem).1 < 1.0 && extent(stem).3 < 106.0);
    }

    /// A slab-serif n, 110 tall: a stem with a flag serif at its head, an
    /// arch off its side into a second stem, a slab serif across the foot
    /// of each, every join bracketed.
    fn serif_n() -> Vec<Polyline> {
        vec![rounded(&[
            (0.0, 0.0, 3.0),
            (36.0, 0.0, 2.0),
            (36.0, 14.0, 3.0),
            (90.0, 14.0, 26.0),
            (90.0, 100.0, 5.0),
            (100.0, 100.0, 3.0),
            (100.0, 110.0, 3.0),
            (64.0, 110.0, 3.0),
            (64.0, 100.0, 3.0),
            (74.0, 100.0, 5.0),
            (74.0, 30.0, 12.0),
            (36.0, 30.0, 4.0),
            (36.0, 100.0, 5.0),
            (46.0, 100.0, 3.0),
            (46.0, 110.0, 3.0),
            (10.0, 110.0, 3.0),
            (10.0, 100.0, 3.0),
            (20.0, 100.0, 5.0),
            (20.0, 10.0, 5.0),
            (0.0, 10.0, 3.0),
        ])]
    }

    #[test]
    fn a_serif_n_is_its_strokes() {
        let n = serif_n();
        let rails = split(&n).expect("columns");
        sews(&n, &rails);
        assert_eq!(rails.len(), 5, "two stems, a flag and two foot serifs");
        // The first stem runs its full height, head to foot serif, the
        // flag and the arch butting into its sides.
        let stem = rails
            .iter()
            .find(|r| {
                let (x0, y0, x1, y1) = extent(r);
                x0 > 17.0 && x1 < 40.0 && y0 < 2.0 && y1 > 95.0
            })
            .expect("a stem its full height");
        let (across, along) = lie(stem);
        assert!(
            across > 14.0 && along < 2.5,
            "{across} across, {along} along"
        );
        // Each foot serif right across under its stem.
        let feet = rails
            .iter()
            .filter(|r| extent(r).1 > 90.0 && extent(r).2 - extent(r).0 > 30.0)
            .count();
        assert_eq!(feet, 2);
        assert!(lean(&rails) < 6.0, "{}°", lean(&rails));
    }

    /// A slab-serif v: two strokes 16 wide meeting at a point, a serif
    /// across the head of each.
    fn serif_v() -> Vec<Polyline> {
        vec![rounded(&[
            (4.0, 0.0, 3.0),
            (40.0, 0.0, 3.0),
            (40.0, 10.0, 3.0),
            (30.0, 10.0, 4.0),
            (50.0, 74.0, 1.0),
            (70.0, 10.0, 4.0),
            (60.0, 10.0, 3.0),
            (60.0, 0.0, 3.0),
            (96.0, 0.0, 3.0),
            (96.0, 10.0, 3.0),
            (86.0, 10.0, 4.0),
            (58.0, 100.0, 3.0),
            (42.0, 100.0, 3.0),
            (14.0, 10.0, 4.0),
            (4.0, 10.0, 3.0),
        ])]
    }

    #[test]
    fn a_serif_v_is_two_strokes_and_two_serifs_with_no_stitch_crossing() {
        let v = serif_v();
        // Taken for one column, serif to serif round the foot, its
        // stitches cross there: not fit to sew as that.
        if let Ok(one) = super::super::satin::auto_rails(&v) {
            assert!(!sound(&v, &one));
        }
        let rails = split(&v).expect("columns");
        sews(&v, &rails);
        assert_eq!(rails.len(), 4);
        let serifs = rails.iter().filter(|r| extent(r).3 < 20.0).count();
        assert_eq!(serifs, 2);
        // One stroke runs on to the foot; the other butts into its side.
        let to_foot = rails.iter().filter(|r| extent(r).3 > 99.0).count();
        assert_eq!(to_foot, 1);
    }

    /// A slab-serif P: a stem 16 wide with a flag serif at its head and a
    /// slab serif across its foot, and a bowl 10 thick round a counter,
    /// every join bracketed.
    fn serif_p() -> Vec<Polyline> {
        vec![
            rounded(&[
                (4.0, 0.0, 3.0),
                (90.0, 0.0, 28.0),
                (90.0, 66.0, 28.0),
                (36.0, 66.0, 5.0),
                (36.0, 120.0, 5.0),
                (50.0, 120.0, 3.0),
                (50.0, 130.0, 3.0),
                (6.0, 130.0, 3.0),
                (6.0, 120.0, 3.0),
                (20.0, 120.0, 5.0),
                (20.0, 10.0, 5.0),
                (4.0, 10.0, 3.0),
            ]),
            rounded(&[
                (36.0, 10.0, 4.0),
                (78.0, 10.0, 16.0),
                (78.0, 56.0, 16.0),
                (36.0, 56.0, 4.0),
            ]),
        ]
    }

    #[test]
    fn a_bracketed_p_is_columns_not_a_fill() {
        let p = serif_p();
        assert!(super::super::satin::auto_rails(&p).is_err());
        let rails = split(&p).expect("columns");
        sews(&p, &rails);
        assert_eq!(rails.len(), 4, "a stem, a flag, a bowl and a foot serif");
        // The stem its full height, the bowl butting into it at both ends.
        let stem = rails
            .iter()
            .find(|r| {
                let (x0, y0, x1, y1) = extent(r);
                x0 > 17.0 && x1 < 40.0 && y0 < 2.0 && y1 > 115.0
            })
            .expect("a stem its full height");
        let (across, along) = lie(stem);
        assert!(
            across > 14.0 && along < 2.5,
            "{across} across, {along} along"
        );
        assert!(lean(&rails) < 6.0, "{}°", lean(&rails));
    }

    #[test]
    fn a_plain_bar_is_sound_as_one_column() {
        let bar = vec![poly(&[
            (0.0, 0.0),
            (120.0, 0.0),
            (120.0, 16.0),
            (0.0, 16.0),
        ])];
        let one = super::super::satin::auto_rails(&bar).expect("a column");
        assert!(sound(&bar, &one));
    }

    #[test]
    fn a_round_ring_is_still_one_column_round_its_hole() {
        let circle = |r: f64| -> Polyline {
            (0..64)
                .map(|i| {
                    let t = i as f64 / 64.0 * std::f64::consts::TAU;
                    P::new(r * t.cos(), r * t.sin())
                })
                .collect()
        };
        let o = vec![circle(60.0), circle(40.0)];
        let rails = split(&o).expect("a column");
        assert_eq!(rails.len(), 1);
        assert!(rails[0].closed);
        assert!(lean(&rails) < 2.0);
    }
}
