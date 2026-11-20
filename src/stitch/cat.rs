//! Satin columns for a filled shape by its chordal axis: the
//! shape's outline, sampled closely, is triangulated (Delaunay), and each
//! triangle inside it says what it is by how many of its sides are
//! chords (sides shared with another triangle inside the shape) rather
//! than outline: one, the end of a stroke; two, the length of one; three,
//! where strokes meet. A stroke is a run of triangles of the second kind,
//! and the chords across it are its rungs: square to it, as satin
//! stitches should be.
//!
//! The chordal axis transform: L. Prasad, "Morphological analysis of
//! shapes" (1997); used to find the strokes of glyphs and handwriting.
//!
//! How ZigZag reads a filled satin's strokes by default (see
//! `design::part_columns`; the `cut` setting cuts it into columns the
//! other way instead).

use super::satin::Rails;
use crate::geom::{FillRule, P, Polyline, cross, point_inside};
use std::collections::HashMap;

/// How far apart the outline is sampled (units).
const SAMPLE: f64 = 1.5;

/// A triangle inside the shape: its corners (indices into the samples),
/// and its neighbours across each side (the side opposite each corner;
/// `None` where that side is the outline's, or has been unlinked).
/// [`Tri::chords`]: how many it has.
#[derive(Clone, Debug)]
pub struct Tri {
    pub v: [usize; 3],
    pub next: [Option<usize>; 3],
}

impl Tri {
    pub fn chords(&self) -> usize {
        self.next.iter().filter(|n| n.is_some()).count()
    }
}

/// The shape's outline sampled about every [`SAMPLE`] units, and which
/// ring each sample is on and where.
pub struct Samples {
    pub pts: Vec<P>,
    /// (ring, place on it, how many on it)
    pub at: Vec<(usize, usize, usize)>,
}

fn sample(comp: &[Polyline]) -> Samples {
    let mut pts = Vec::new();
    let mut at = Vec::new();
    for (k, ring) in comp.iter().enumerate() {
        let n = ring.len();
        if n < 3 {
            continue;
        }
        // (A ring smaller than the sampling can show is no counter: a
        // speck left by a union or a trace. Three samples of it would be
        // read as a hole for strokes to go round.)
        let area = (0..n)
            .map(|i| {
                let (a, b) = (ring[i], ring[(i + 1) % n]);
                a.x * b.y - b.x * a.y
            })
            .sum::<f64>()
            .abs()
            / 2.0;
        if area < SAMPLE * SAMPLE {
            continue;
        }
        let mut here: Vec<P> = Vec::new();
        for i in 0..n {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            let d = a.dist(b);
            let m = (d / SAMPLE).ceil().max(1.0) as usize;
            for j in 0..m {
                here.push(a.lerp(b, j as f64 / m as f64));
            }
        }
        let count = here.len();
        for (i, p) in here.into_iter().enumerate() {
            pts.push(p);
            at.push((k, i, count));
        }
    }
    Samples { pts, at }
}

/// Delaunay triangles of `pts` (Bowyer–Watson).
fn delaunay(pts: &[P]) -> Vec<[usize; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    let (mut lo, mut hi) = (pts[0], pts[0]);
    for p in pts {
        lo = P::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = P::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    let span = (hi.x - lo.x).max(hi.y - lo.y).max(1.0) * 20.0;
    let mid = lo.lerp(hi, 0.5);
    let mut all: Vec<P> = pts.to_vec();
    all.push(P::new(mid.x - span, mid.y - span));
    all.push(P::new(mid.x + span, mid.y - span));
    all.push(P::new(mid.x, mid.y + span));
    let circle = |t: [usize; 3], all: &[P]| -> (P, f64) {
        let (a, b, c) = (all[t[0]], all[t[1]], all[t[2]]);
        let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
        if d.abs() < 1e-12 {
            return (a, f64::INFINITY);
        }
        let (a2, b2, c2) = (a.dot(a), b.dot(b), c.dot(c));
        let ux = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
        let uy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
        let u = P::new(ux, uy);
        (u, u.dist(a).powi(2))
    };
    let mut tris: Vec<([usize; 3], P, f64)> = Vec::new();
    let sup = [n, n + 1, n + 2];
    let (c, r2) = circle(sup, &all);
    tris.push((sup, c, r2));
    for i in 0..n {
        let p = all[i];
        let mut bad = Vec::new();
        for (k, (_, c, r2)) in tris.iter().enumerate() {
            if c.dist(p).powi(2) < *r2 * (1.0 + 1e-9) {
                bad.push(k);
            }
        }
        let mut edges: HashMap<(usize, usize), usize> = HashMap::new();
        for &k in &bad {
            let t = tris[k].0;
            for e in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *edges.entry((e.0.min(e.1), e.0.max(e.1))).or_insert(0) += 1;
            }
        }
        // (The cavity's rim: the bad triangles' sides not shared.)
        let mut rim = Vec::new();
        for &k in &bad {
            let t = tris[k].0;
            for e in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                if edges[&(e.0.min(e.1), e.0.max(e.1))] == 1 {
                    rim.push(e);
                }
            }
        }
        for &k in bad.iter().rev() {
            tris.swap_remove(k);
        }
        for (a, b) in rim {
            let t = [a, b, i];
            let (c, r2) = circle(t, &all);
            tris.push((t, c, r2));
        }
    }
    tris.into_iter()
        .map(|(t, _, _)| t)
        .filter(|t| t.iter().all(|&v| v < n))
        .collect()
}

/// The triangles inside the shape, each with its neighbours inside it,
/// and the samples they're made of.
pub fn triangles(comp: &[Polyline]) -> (Samples, Vec<Tri>) {
    let s = sample(comp);
    let all = delaunay(&s.pts);
    let inside: Vec<[usize; 3]> = all
        .into_iter()
        .filter(|t| {
            let c = P::new(
                (s.pts[t[0]].x + s.pts[t[1]].x + s.pts[t[2]].x) / 3.0,
                (s.pts[t[0]].y + s.pts[t[1]].y + s.pts[t[2]].y) / 3.0,
            );
            point_inside(comp, FillRule::EvenOdd, c)
        })
        .collect();
    // A side is the outline's if its two samples are next to each other
    // on a ring; any other side shared with another triangle inside is a
    // chord between them.
    let outline = |a: usize, b: usize| {
        let ((ra, ia, n), (rb, ib, _)) = (s.at[a], s.at[b]);
        ra == rb && ((ia + 1) % n == ib || (ib + 1) % n == ia)
    };
    let mut by_side: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (k, t) in inside.iter().enumerate() {
        for j in 0..3 {
            let (a, b) = (t[(j + 1) % 3], t[(j + 2) % 3]);
            by_side.entry((a.min(b), a.max(b))).or_default().push(k);
        }
    }
    let tris = inside
        .iter()
        .enumerate()
        .map(|(k, t)| {
            let mut next = [None; 3];
            for (j, slot) in next.iter_mut().enumerate() {
                let (a, b) = (t[(j + 1) % 3], t[(j + 2) % 3]);
                if outline(a, b) {
                    continue;
                }
                // (Only between the two triangles that share it: where the
                // triangulation overlaps itself, more do, and a link to one of
                // them wouldn't be answered by a link back.)
                let sharing = &by_side[&(a.min(b), a.max(b))];
                if sharing.len() == 2 {
                    *slot = sharing.iter().copied().find(|&o| o != k);
                }
            }
            Tri { v: *t, next }
        })
        .collect();
    (s, tris)
}

/// The most two strokes may turn from straight on (radians) where they
/// meet and still be one stroke going round: further, a corner they each
/// end at.
const CORNER: f64 = 1.05;

/// How sharply (radians) the outline must turn at a point, over a little
/// of it either side (see `CORNER_SPAN`), to be a corner.
const CORNER_AT: f64 = 0.6;

/// Over how much of a join's width either side of a point the outline's
/// turn there is measured: the same share of a letter at any size (a
/// fixed two samples read the same corner differently at 8 and 15 mm).
/// A quarter: half gave as clean letters with more seams.
const CORNER_SPAN: f64 = 0.25;

/// How much of a shape its columns must cover (by area), or it's cut into
/// columns the usual way: a square or a dot has no strokes to read, only
/// the corners of one, and a shape the axis misreads leaves cloth bare.
/// (Measured on the wider letter census at 15 mm: with no such floor, 2.3%
/// of letters show bare cloth; at 0.97, 0.6%, at the outer corners of flat
/// ends a column's last stitch runs diagonally across; at 0.985, none, for
/// 2% more trims. Stricter gave fewer clean letters.)
const COVERED: f64 = 0.985;

/// How far two strokes may turn from each other at a corner (radians:
/// 120°, strokes meeting at less than 60°) and still be mitred, when only
/// clear joins are wanted. Sharper, a mitre's stitches fan into the
/// corner's point, piled into one place: the point of a V, the corners of
/// an N or an M. (Judged by eye on the letters in tools/census/judged.txt
/// and sheets of them, against the census: at 150° none of those letters
/// is cut, at 120° the V's, W's, M's and A's are, for 0.3% fewer clean
/// letters by the census, which counts the squared corners' stitches as
/// leaning, and fewer crowded needle points.)
const MITRE_LIMIT: f64 = 2.0 * std::f64::consts::PI / 3.0;

/// How much straighter (by the dot product of their headings) the pair
/// of strokes that goes through a join of three must be than the next
/// pair, when only clear joins are wanted. Closer, either reading is a
/// guess: the top of a bold N, where the stem and the diagonal both run
/// into one flat, read with the diagonal going through, the stem fanned
/// into its side. (0.15 sent many plain joins the other way, losing 3% of
/// clean letters; 0.03 catches the ties.)
const AMBIGUOUS: f64 = 0.03;

/// How far along a stroke its direction is taken from where it leaves a
/// join, in the join's widths: past the join's own slant (one width is
/// still in it), not so far as to follow a bend (measured: one width and
/// two at mitres alone gave 7 more trims a size on the letter census).
const REACH: f64 = 2.0;

/// How many rungs either side each rung is averaged over.
const SMOOTH: usize = 4;

/// A branch of the axis found for pruning: its length, the join it
/// leaves, its first step, the triangles it's made of, and its tips.
type Branch = (f64, usize, usize, Vec<usize>, Vec<usize>);

fn side(t: &Tri, j: usize) -> (usize, usize) {
    (t.v[(j + 1) % 3], t.v[(j + 2) % 3])
}

fn mid(s: &Samples, (a, b): (usize, usize)) -> P {
    s.pts[a].lerp(s.pts[b], 0.5)
}

/// The triangles from `from` on across its side `j` while each has two
/// links, to the first that hasn't (or back to `from`): the chords
/// crossed (the first is side `j`), that triangle, and those passed
/// through on the way.
fn branch(tris: &[Tri], from: usize, j: usize) -> (Vec<(usize, usize)>, usize, Vec<usize>) {
    let mut chords = vec![side(&tris[from], j)];
    let mut path = Vec::new();
    let (mut prev, mut here) = (from, tris[from].next[j].unwrap());
    // (Never round a loop that doesn't come back to `from`: it stops at the
    // first triangle it comes to again.)
    let mut seen = std::collections::HashSet::from([from]);
    loop {
        if !seen.insert(here) {
            return (chords, here, path);
        }
        path.push(here);
        let h = &tris[here];
        if h.chords() != 2 {
            return (chords, here, path);
        }
        let k = (0..3)
            .find(|&k| h.next[k].is_some() && h.next[k] != Some(prev))
            .unwrap();
        chords.push(side(h, k));
        prev = here;
        here = h.next[k].unwrap();
        if here == from {
            return (chords, here, path);
        }
    }
}

/// Cut the link between triangle `t` and its neighbour across side `j`.
fn unlink(tris: &mut [Tri], t: usize, j: usize) {
    if let Some(o) = tris[t].next[j].take()
        && let Some(k) = (0..3).find(|&k| tris[o].next[k] == Some(t))
    {
        tris[o].next[k] = None;
    }
}

/// How a stroke ends at a triangle the chain stops at.
#[derive(Clone, Debug, Default)]
struct End {
    /// Spurs pruned off it: the samples at their tips.
    tips: Vec<usize>,
    /// The side it butts into another stroke along.
    cut: Option<(usize, usize)>,
    /// A corner two strokes meet at, at an angle: each ends on the line
    /// from the inside of the corner to this sample, its tip.
    mitre: Option<usize>,
}

/// The samples along the outline from `i` to `j` the shorter way round
/// (both on one ring), not including `i`.
fn walk(s: &Samples, i: usize, j: usize) -> Option<Vec<usize>> {
    let ((ri, ii, n), (rj, ij, _)) = (s.at[i], s.at[j]);
    if ri != rj {
        return None;
    }
    let base = i - ii;
    let fwd = (ij + n - ii) % n;
    let back = (ii + n - ij) % n;
    let (steps, forward) = if fwd <= back {
        (fwd, true)
    } else {
        (back, false)
    };
    Some(
        (1..=steps)
            .map(|k| {
                base + if forward {
                    (ii + k) % n
                } else {
                    (ii + n - k) % n
                }
            })
            .collect(),
    )
}

/// Each chord of a chain with its first sample on rail a's side: the
/// sample two chords share stays on its side.
fn oriented(chords: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut out = vec![chords[0]];
    for &(c, d) in &chords[1..] {
        let (pa, pb) = *out.last().unwrap();
        out.push(if c == pa || d == pa {
            (pa, if c == pa { d } else { c })
        } else {
            (if c == pb { d } else { c }, pb)
        });
    }
    out
}

/// The shape's columns: its strokes by its chordal axis; `None` where
/// that reads no strokes, or they leave much of it out (see `COVERED`),
/// for it to be cut into columns the other way (see `design`). And, with
/// `clear_joins`, `None` where its strokes meet at a corner sharper than
/// a mitre sews cleanly (`MITRE_LIMIT`), or three meet with no two of
/// them clearly the stroke that goes through (`AMBIGUOUS`): the other way
/// squares those corners, one stroke run on and the other ended against
/// it.
pub fn columns(comp: &[Polyline], clear_joins: bool) -> Option<Vec<Rails>> {
    let (s, mut tris) = triangles(comp);
    if tris.is_empty() {
        return None;
    }
    let n = tris.len();
    // (Where strokes meet, as triangulated: no side of one of these is
    // across a stroke, to be a rung.)
    let join: Vec<bool> = tris.iter().map(|t| t.chords() == 3).collect();
    let mut gone = vec![false; n];
    let mut ends: Vec<End> = vec![End::default(); n];
    // A join's width: its sides, on average.
    let width = |tris: &[Tri], t: usize| {
        let v = tris[t].v;
        (s.pts[v[0]].dist(s.pts[v[1]])
            + s.pts[v[1]].dist(s.pts[v[2]])
            + s.pts[v[2]].dist(s.pts[v[0]]))
            / 3.0
    };
    // Spurs off joins, shortest first, until there are none: short
    // branches out to a point (along a curve, the sampling's; at a flat
    // end, its corners). Not a branch out to an end of its own, a stroke
    // however short (a crossbar's arm): its end has had its corners pruned
    // first, being shorter.
    loop {
        let mut best: Option<Branch> = None;
        for t in 0..n {
            let links = tris[t].chords();
            if gone[t] || !(links == 3 || (links == 2 && !ends[t].tips.is_empty())) {
                continue;
            }
            for j in 0..3 {
                if tris[t].next[j].is_none() {
                    continue;
                }
                let (chords, end, path) = branch(&tris, t, j);
                if tris[end].chords() != 1 || end == t {
                    continue;
                }
                // Out to a point (its tip), or to an end of its own (its
                // corners, pruned already).
                let own = ends[end].tips.clone();
                let e = &tris[end];
                let k = (0..3).find(|&k| e.next[k].is_some()).unwrap();
                let tip = e.v[k];
                let mut len = 0.0;
                for w2 in chords.windows(2) {
                    len += mid(&s, w2[0]).dist(mid(&s, w2[1]));
                }
                let far = if own.is_empty() {
                    mid(&s, *chords.last().unwrap()).dist(s.pts[tip])
                } else {
                    0.0
                };
                len += far;
                // Not a stroke if all of it lies within the circle whose
                // diameter is the chord it leaves the join by (Prasad's
                // test): its corners too, if it ends in its own.
                let (a, b) = chords[0];
                let (c, r) = (s.pts[a].lerp(s.pts[b], 0.5), s.pts[a].dist(s.pts[b]) * 0.5);
                let within = path
                    .iter()
                    .flat_map(|&u| tris[u].v)
                    .chain(own.iter().copied())
                    .all(|v| s.pts[v].dist(c) <= r * (1.0 + 1e-9));
                if within && best.as_ref().is_none_or(|b| len < b.0) {
                    let tips = if own.is_empty() { vec![tip] } else { own };
                    best = Some((len, t, j, tips, path));
                }
            }
        }
        let Some((_, t, j, tips, path)) = best else {
            break;
        };
        unlink(&mut tris, t, j);
        for p in path {
            gone[p] = true;
        }
        ends[t].tips.extend(tips);
    }
    let centre = |tris: &[Tri], t: usize| {
        let v = tris[t].v;
        P::new(
            (s.pts[v[0]].x + s.pts[v[1]].x + s.pts[v[2]].x) / 3.0,
            (s.pts[v[0]].y + s.pts[v[1]].y + s.pts[v[2]].y) / 3.0,
        )
    };
    // Which way the stroke across side `j` of `t` runs, from `t`: to its
    // middle some way along (`REACH` times the join's width).
    let heading = |tris: &[Tri], t: usize, j: usize| {
        let c = centre(tris, t);
        let w = width(tris, t);
        let (chords, _, _) = branch(tris, t, j);
        let mut at = mid(&s, chords[0]);
        let mut gone_by = 0.0;
        for w2 in chords.windows(2) {
            gone_by += mid(&s, w2[0]).dist(mid(&s, w2[1]));
            at = mid(&s, w2[1]);
            if gone_by > REACH * w {
                break;
            }
        }
        (at - c).norm()
    };
    // A spur pruned off where two strokes meet at an angle (the corner of
    // a Z, the point of a V) was the corner's: they're two strokes, each
    // ending on the mitre, not one going round. (Only where the outline
    // turns a corner at the spur's tip: along a curve, a spur is the
    // sampling's, and the stroke goes on round.)
    let sharp = |i: usize, w: f64| {
        let (_, at, count) = s.at[i];
        let base = i - at;
        let reach = ((CORNER_SPAN * w / SAMPLE).round() as usize).clamp(1, (count / 4).max(1));
        let (b, a) = (
            base + (at + count - reach) % count,
            base + (at + reach) % count,
        );
        let (u, v) = ((s.pts[i] - s.pts[b]).norm(), (s.pts[a] - s.pts[i]).norm());
        cross(u, v).atan2(u.dot(v)).abs() > CORNER_AT
    };
    // The triangle a little way along link `j` of `t` (less than the join's
    // width at `t`) that `wanted` picks, and its link back the way it was
    // come to: two joins that near are one (the chordal axis's junction
    // merge: the corners of one point, the two joins where strokes cross).
    let partner = |tris: &[Tri], t: usize, j: usize, wanted: &dyn Fn(usize) -> bool| {
        let w = width(tris, t);
        let (chords, _, path) = branch(tris, t, j);
        let mut gone_by = 0.0;
        for (k, &p) in path.iter().enumerate() {
            if k > 0 {
                gone_by += mid(&s, chords[k - 1]).dist(mid(&s, chords[k]));
            }
            if gone_by > w {
                return None;
            }
            if p != t && wanted(p) {
                let before = if k == 0 { t } else { path[k - 1] };
                let back = (0..3).find(|&x| tris[p].next[x] == Some(before))?;
                return Some((p, back));
            }
        }
        None
    };
    // (A point cut off flat, a W's foot, has two corners: the stroke kept
    // here runs on along the flat to the far one, the other's, so that
    // both end on the line from the inside to it and the flat is sewn.
    // The corners of one point may be pruned onto two triangles a little
    // way apart, a wide flat, a W's middle point: they're `partner`s, one
    // join with both corners, and the stroke kept runs on through the
    // stretch between them.)
    let corner = |tris: &[Tri], ends: &[End], t: usize| {
        let tips = &ends[t].tips;
        !gone[t]
            && tris[t].chords() == 2
            && !tips.is_empty()
            && tips.len() <= 2
            && tips.iter().any(|&i| sharp(i, width(tris, t)))
    };
    for t in 0..n {
        if !corner(&tris, &ends, t) {
            continue;
        }
        let js: Vec<usize> = (0..3).filter(|&j| tris[t].next[j].is_some()).collect();
        // The join it's part of: its own, or with the corner a little way
        // along one of its links (`far`, and the link to it).
        let far = js.iter().find_map(|&j| {
            let (p, back) = partner(&tris, t, j, &|p| corner(&tris, &ends, p))?;
            let out = (0..3).find(|&x| x != back && tris[p].next[x].is_some())?;
            Some((j, p, out))
        });
        // The two strokes, from the join: the one kept (from `t`), and the
        // one that ends on the mitre (from `at`, across its side `out`).
        let (kept, at, out) = match far {
            Some((j, p, out)) => (*js.iter().find(|&&x| x != j).unwrap(), p, out),
            None => (js[0], t, js[1]),
        };
        let (h0, h1) = (heading(&tris, t, kept), heading(&tris, at, out));
        // (Straight on, the two headings are opposite.)
        let turn = std::f64::consts::PI - h0.dot(h1).clamp(-1.0, 1.0).acos();
        if turn > CORNER {
            // (Sharper than a mitre sews cleanly: see `MITRE_LIMIT`.)
            if clear_joins && turn > MITRE_LIMIT {
                return None;
            }
            let theirs = mid(&s, side(&tris[at], out));
            let tip = *ends[at]
                .tips
                .iter()
                .min_by(|&&x, &&y| s.pts[x].dist(theirs).total_cmp(&s.pts[y].dist(theirs)))
                .unwrap();
            let o = tris[at].next[out].unwrap();
            unlink(&mut tris, at, out);
            ends[t].tips.clear();
            ends[at].tips.clear();
            ends[at].mitre = Some(tip);
            ends[o].mitre = Some(tip);
        }
    }
    // Where two strokes cross, the triangulation makes two joins a short
    // way apart, not one: taken one at a time, each would pair an arm with
    // the way to the other (an x read as > and <). Taken together, the
    // pair of arms across them that runs straightest through goes through,
    // and the other two butt into it.
    let butt_at = |tris: &mut Vec<Tri>, ends: &mut Vec<End>, t: usize, j: usize| {
        // (Two joins beside each other, each the other's partner, can
        // both butt at the side they share: once is enough.)
        let Some(o) = tris[t].next[j] else { return };
        let cut = side(&tris[t], j);
        unlink(tris, t, j);
        ends[o].cut = Some(cut);
    };
    for t in 0..n {
        if gone[t] || tris[t].chords() != 3 {
            continue;
        }
        let link = (0..3).find_map(|j| {
            let (u, ju) = partner(&tris, t, j, &|p| !gone[p] && tris[p].chords() == 3)?;
            Some((j, u, ju))
        });
        let Some((jt, u, ju)) = link else { continue };
        let arms_t: Vec<usize> = (0..3).filter(|&j| j != jt).collect();
        let arms_u: Vec<usize> = (0..3).filter(|&k| k != ju).collect();
        let ht: Vec<P> = arms_t.iter().map(|&j| heading(&tris, t, j)).collect();
        let hu: Vec<P> = arms_u.iter().map(|&k| heading(&tris, u, k)).collect();
        // The pair across them (an arm of each) that runs straightest
        // through goes through, as at a single join (a k's stem, an x's
        // diagonal); the other arm at each butts into it.
        let mut best = (0, 0, f64::INFINITY);
        for (a, &ha) in ht.iter().enumerate() {
            for (b, &hb) in hu.iter().enumerate() {
                let d = ha.dot(hb);
                if d < best.2 {
                    best = (a, b, d);
                }
            }
        }
        // (Not a crossing if the straightest pair of all is at one join,
        // the stem of a k whose leg leaves its arm: each join on its own.)
        if ht[0].dot(ht[1]) < best.2 || hu[0].dot(hu[1]) < best.2 {
            continue;
        }
        let halves = (1 - best.0, 1 - best.1);
        butt_at(&mut tris, &mut ends, t, arms_t[halves.0]);
        butt_at(&mut tris, &mut ends, u, arms_u[halves.1]);
    }
    // At each join left, the two branches that run straightest into each
    // other go through; the third butts into them along its side.
    for t in 0..n {
        if gone[t] || tris[t].chords() != 3 {
            continue;
        }
        let h = [
            heading(&tris, t, 0),
            heading(&tris, t, 1),
            heading(&tris, t, 2),
        ];
        let straight = |b: usize| h[(b + 1) % 3].dot(h[(b + 2) % 3]);
        let butt = (0..3)
            .min_by(|&x, &y| straight(x).total_cmp(&straight(y)))
            .unwrap();
        // (No pair clearly straighter than another: see `AMBIGUOUS`.)
        if clear_joins {
            let mut by: Vec<f64> = (0..3).map(straight).collect();
            by.sort_by(f64::total_cmp);
            if by[1] - by[0] < AMBIGUOUS {
                return None;
            }
        }
        let o = tris[t].next[butt].unwrap();
        let cut = side(&tris[t], butt);
        unlink(&mut tris, t, butt);
        ends[o].cut = Some(cut);
    }
    // The strokes: from each end, on through each triangle with two links.
    let mut out = Vec::new();
    let mut used = vec![false; n];
    let starts: Vec<usize> = (0..n)
        .filter(|&t| !gone[t] && tris[t].chords() == 1)
        .collect();
    for start in starts {
        if used[start] {
            continue;
        }
        let j = (0..3).find(|&j| tris[start].next[j].is_some()).unwrap();
        let (chords, end, path) = branch(&tris, start, j);
        used[start] = true;
        path.iter().for_each(|&p| used[p] = true);
        let chords = oriented(&chords);
        // (A chord is a rung unless it's a side of a join.)
        let across: Vec<bool> = (0..chords.len())
            .map(|i| {
                let before = if i == 0 { start } else { path[i - 1] };
                let after = path.get(i).copied().unwrap_or(end);
                !(join[before] || join[after])
            })
            .collect();
        if let Some(r) = rails(&s, &tris, &ends, start, end, &chords, &across, &path) {
            out.push(r);
        }
    }
    // And what's left: rings, strokes with neither end nor join (an o, the
    // bowl of a Q a tail crosses), each sewn round as a closed column.
    for t in 0..n {
        if gone[t] || used[t] || tris[t].chords() != 2 {
            continue;
        }
        let j = (0..3).find(|&j| tris[t].next[j].is_some()).unwrap();
        let (chords, end, path) = branch(&tris, t, j);
        used[t] = true;
        path.iter().for_each(|&p| used[p] = true);
        if end != t || chords.len() < 3 {
            continue;
        }
        if let Some(r) = ring(&s, &oriented(&chords)) {
            out.push(r);
        }
    }
    // (Not if they leave much of the shape out: a square, the dot of an
    // i, has no stroke to read, only the corners of one; it's cut into
    // columns the usual way.)
    // (Nor if a column is wider than a satin's stitches can be: a fat
    // shape that isn't strokes, as `auto_rails` says of one column.)
    let widest = |r: &Rails| {
        super::satin::pairs(r, 10.0)
            .iter()
            .map(|(a, b)| a.dist(*b))
            .fold(0.0, f64::max)
    };
    if out.is_empty()
        || out.iter().any(|r| widest(r) > super::satin::AUTO_MAX_WIDTH)
        || covered(comp, &out) < COVERED
    {
        return None;
    }
    Some(out)
}

/// How much of the shape (by area, of samples a sampling step apart
/// inside it) the columns cover.
fn covered(comp: &[Polyline], columns: &[Rails]) -> f64 {
    let outlines: Vec<Vec<Polyline>> = columns.iter().map(Rails::outline).collect();
    let (mut lo, mut hi) = (
        P::new(f64::INFINITY, f64::INFINITY),
        P::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    for p in comp.iter().flatten() {
        lo = P::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = P::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    let (mut inside, mut sewn) = (0usize, 0usize);
    let mut y = lo.y + SAMPLE * 0.5;
    while y < hi.y {
        let mut x = lo.x + SAMPLE * 0.5;
        while x < hi.x {
            let p = P::new(x, y);
            if point_inside(comp, FillRule::EvenOdd, p) {
                inside += 1;
                sewn += usize::from(
                    outlines
                        .iter()
                        .any(|o| point_inside(o, FillRule::EvenOdd, p)),
                );
            }
            x += SAMPLE;
        }
        y += SAMPLE;
    }
    if inside == 0 {
        1.0
    } else {
        sewn as f64 / inside as f64
    }
}

/// A ring's rails from its chords all round (each a rung), closed.
fn ring(s: &Samples, chords: &[(usize, usize)]) -> Option<Rails> {
    let mut a: Vec<usize> = Vec::new();
    let mut b: Vec<usize> = Vec::new();
    let mut rung = Vec::new();
    for &(pa, pb) in chords {
        if a.last() != Some(&pa) {
            a.push(pa);
        }
        if b.last() != Some(&pb) {
            b.push(pb);
        }
        rung.push((a.len() - 1, b.len() - 1));
    }
    // (Round to where it began: the first chord's samples, the same way
    // round.)
    if a.len() > 1 && a.last() == a.first() {
        a.pop();
    }
    if b.len() > 1 && b.last() == b.first() {
        b.pop();
    }
    if a.len() < 3 || b.len() < 3 {
        return None;
    }
    let mut pa: Polyline = a.iter().map(|&i| s.pts[i]).collect();
    let mut pb: Polyline = b.iter().map(|&i| s.pts[i]).collect();
    let (ca, cb) = (cumulative(&pa), cumulative(&pb));
    let mut sync: Vec<(f64, f64)> = rung
        .iter()
        .skip(1)
        .filter(|&&(i, k)| i < ca.len() && k < cb.len() && (i > 0 || k > 0))
        .map(|&(i, k)| (ca[i], cb[k]))
        .collect();
    sync.dedup_by(|x, y| x.0 <= y.0 || x.1 <= y.1);
    sync.push((*ca.last().unwrap(), *cb.last().unwrap()));
    pa.push(pa[0]);
    pb.push(pb[0]);
    Some(Rails {
        a: pa,
        b: pb,
        closed: true,
        sync,
    })
}

/// How far along a line each of its points is.
fn cumulative(l: &[P]) -> Vec<f64> {
    let mut c = vec![0.0];
    for w in l.windows(2) {
        c.push(c.last().unwrap() + w[0].dist(w[1]));
    }
    c
}

/// A stroke's rails from its chords (those `across` it its rungs),
/// carried on at each end as the end triangle says: to its point, to the
/// corners of a flat end, along a mitre, or along the side it butts into.
#[allow(clippy::too_many_arguments)]
fn rails(
    s: &Samples,
    tris: &[Tri],
    ends: &[End],
    start: usize,
    end: usize,
    chords: &[(usize, usize)],
    across: &[bool],
    path: &[usize],
) -> Option<Rails> {
    let pts = |v: Vec<usize>| -> Vec<P> { v.into_iter().map(|i| s.pts[i]).collect() };
    // Points on rail a and rail b beyond the chord at an end.
    let finish = |t: usize, (pa, pb): (usize, usize)| -> (Vec<P>, Vec<P>) {
        let tri = &tris[t];
        let r = *tri.v.iter().find(|&&v| v != pa && v != pb).unwrap_or(&pa);
        let e = &ends[t];
        if let Some(tip) = e.mitre {
            // The rail on the corner's outside runs on to its tip; the
            // other stays at the inside: the end along the mitre.
            let (wa, wb) = (walk(s, pa, tip), walk(s, pb, tip));
            return match (wa, wb) {
                (Some(x), Some(y)) if x.len() <= y.len() => (pts(x), vec![]),
                (_, Some(y)) => (vec![], pts(y)),
                (Some(x), None) => (pts(x), vec![]),
                _ => (vec![], vec![]),
            };
        }
        if let Some((c, d)) = e.cut {
            // Along the side it butts into: r is on one rail's side.
            if (c == pb && d == r) || (d == pb && c == r) {
                return (vec![s.pts[r]], vec![]);
            }
            return (vec![], vec![s.pts[r]]);
        }
        match e.tips.len() {
            0 => (vec![s.pts[r]], vec![s.pts[r]]),
            1 => {
                let t0 = e.tips[0];
                (
                    pts(walk(s, pa, t0).unwrap_or_default()),
                    pts(walk(s, pb, t0).unwrap_or_default()),
                )
            }
            _ => {
                // Round the end, each rail as far as the tip nearest its
                // middle on that rail's side: a flat end's two corners,
                // the end straight across between them; a round end's
                // spurs, most of the way round it.
                let Some(round) = walk(s, pa, pb) else {
                    return (vec![], vec![]);
                };
                let half = round.len() / 2;
                let at = |t: usize| round.iter().position(|&x| x == t);
                let ta = e
                    .tips
                    .iter()
                    .filter_map(|&t| at(t).filter(|&i| i <= half))
                    .max();
                let tb = e
                    .tips
                    .iter()
                    .filter_map(|&t| at(t).filter(|&i| i > half))
                    .min();
                let a_side = ta.map(|i| round[..=i].to_vec()).unwrap_or_default();
                let b_side = tb
                    .map(|i| round[i..round.len() - 1].iter().rev().copied().collect())
                    .unwrap_or_default();
                (pts(a_side), pts(b_side))
            }
        }
    };
    let (ha, hb) = finish(start, chords[0]);
    let (ta, tb) = finish(end, *chords.last().unwrap());
    let mut a: Polyline = ha.into_iter().rev().collect();
    let mut b: Polyline = hb.into_iter().rev().collect();
    let mut rung = Vec::new();
    // Across a triangle a spur was pruned off where the stroke goes on
    // through (the outside of a bend, round or sharp), each rail follows
    // the outline round where the spur was, not straight across its foot;
    // and the rungs either side aren't averaged over it (`stretch`: see
    // `sync` below).
    let round = |k: usize| k > 0 && path.get(k - 1).is_some_and(|&t| !ends[t].tips.is_empty());
    let mut stretch: Vec<usize> = Vec::new();
    let mut stretches = 0;
    let mut last: Option<(usize, usize)> = None;
    for (k, (&(pa, pb), &ok)) in chords.iter().zip(across).enumerate() {
        for (rail, from, to) in [
            (&mut a, last.map(|l| l.0), pa),
            (&mut b, last.map(|l| l.1), pb),
        ] {
            if let Some(from) = from.filter(|&f| f != to && round(k))
                && let Some(way) = walk(s, from, to)
                && way.len() > 1
            {
                way.iter().for_each(|&i| rail.push(s.pts[i]));
                stretches += 1;
            }
            if rail.last().is_none_or(|l| l.dist(s.pts[to]) > 1e-9) {
                rail.push(s.pts[to]);
            }
        }
        last = Some((pa, pb));
        if ok {
            rung.push((a.len() - 1, b.len() - 1));
            stretch.push(stretches);
        }
    }
    a.extend(ta);
    b.extend(tb);
    if a.len() < 2 || b.len() < 2 {
        return None;
    }
    let (ca, cb) = (cumulative(&a), cumulative(&b));
    let (la, lb) = (*ca.last().unwrap(), *cb.last().unwrap());
    // (Each chord joins a sample to one a sample along on the other side
    // or not, so their slant alternates a little: each rung where its
    // neighbours either side put it, on average.)
    let raw: Vec<(f64, f64)> = rung.iter().map(|&(i, k)| (ca[i], cb[k])).collect();
    let reach = SMOOTH;
    let sync: Vec<(f64, f64)> = (0..raw.len())
        .map(|i| {
            let lo = (i.saturating_sub(reach)..=i)
                .find(|&k| stretch[k] == stretch[i])
                .unwrap_or(i);
            let hi = (i + 1..(i + reach + 1).min(raw.len()))
                .rev()
                .find(|&k| stretch[k] == stretch[i])
                .map_or(i + 1, |k| k + 1);
            let n = (hi - lo) as f64;
            let (x, y) = raw[lo..hi]
                .iter()
                .fold((0.0, 0.0), |(x, y), r| (x + r.0, y + r.1));
            (x / n, y / n)
        })
        .filter(|&(x, y)| x > 1e-6 && y > 1e-6 && x < la - 1e-6 && y < lb - 1e-6)
        .collect();
    Some(Rails {
        a,
        b,
        closed: false,
        sync,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(pts: &[(f64, f64)]) -> Vec<Polyline> {
        vec![pts.iter().map(|&(x, y)| P::new(x, y)).collect()]
    }

    #[test]
    fn a_corner_too_sharp_to_mitre_is_left_to_be_cut() {
        // A V, 15 mm tall, its arms meeting at 34°: read by its axis, the
        // two strokes mitred into the point; only clear joins wanted, it's
        // left to be cut, corners squared.
        let v = shape(&[
            (0.0, 0.0),
            (30.0, 0.0),
            (60.0, 110.0),
            (90.0, 0.0),
            (120.0, 0.0),
            (75.0, 150.0),
            (45.0, 150.0),
        ]);
        assert!(columns(&v, false).is_some());
        assert!(columns(&v, true).is_none());
        // An L's square corner mitres cleanly either way.
        let l = shape(&[
            (0.0, 0.0),
            (30.0, 0.0),
            (30.0, 120.0),
            (100.0, 120.0),
            (100.0, 150.0),
            (0.0, 150.0),
        ]);
        assert!(columns(&l, false).is_some());
        assert!(columns(&l, true).is_some());
    }

    #[test]
    fn a_speck_of_a_hole_is_no_counter() {
        // A T and an L, each with a hole a few hundredths of a millimetre
        // across (as a union or a trace leaves): read as the letter is
        // without it, and nothing fails on the three samples of it.
        let t: Vec<P> = [
            (0.0, 0.0),
            (100.0, 0.0),
            (100.0, 16.0),
            (58.0, 16.0),
            (58.0, 120.0),
            (42.0, 120.0),
            (42.0, 16.0),
            (0.0, 16.0),
        ]
        .iter()
        .map(|&(x, y)| P::new(x, y))
        .collect();
        let l: Vec<P> = [
            (0.0, 0.0),
            (30.0, 0.0),
            (30.0, 120.0),
            (100.0, 120.0),
            (100.0, 150.0),
            (0.0, 150.0),
        ]
        .iter()
        .map(|&(x, y)| P::new(x, y))
        .collect();
        let speck =
            |pts: &[(f64, f64)]| -> Polyline { pts.iter().map(|&(x, y)| P::new(x, y)).collect() };
        let cases = [
            (t, speck(&[(41.37, 15.41), (41.37, 15.71), (41.67, 15.41)])),
            (l, speck(&[(0.37, 124.41), (0.37, 124.71), (0.67, 124.41)])),
        ];
        for (letter, hole) in cases {
            for clear in [false, true] {
                let plain = columns(std::slice::from_ref(&letter), clear).map(|c| c.len());
                let holed = columns(&[letter.clone(), hole.clone()], clear).map(|c| c.len());
                assert_eq!(holed, plain);
            }
        }
    }
}
