//! Satin columns between two rails.
//!
//! Every satin, however it was drawn, becomes a pair of rails sewn with zigzag
//! stitches between them:
//! - a single stroke: rails are its centerline offset by ± width/2;
//! - an Ink/Stitch-style satin column: a path with two rails (and optional
//!   rungs that keep the rails in step);
//! - a filled shape ("auto satin"): a ring with a hole uses outer and inner
//!   outlines as rails; a single ring is split at its two tips.
//!
//! The column is sewn as one continuous path: underlay passes (center walk,
//! contour, zigzag) followed by the top satin, with pull compensation and
//! short stitches on the inside of tight curves.

use crate::geom::{FillRule, P, Polyline, cross, point_inside, polyline_length};
use crate::params::underlay;

#[derive(Clone, Debug)]
pub struct Rails {
    pub a: Polyline,
    pub b: Polyline,
    /// Rails are closed loops (e.g. a ring); first and last points coincide.
    pub closed: bool,
    /// Rung positions as (arc length on a, arc length on b), increasing.
    pub sync: Vec<(f64, f64)>,
}

impl Rails {
    /// What the column covers, as rings: a closed one's two rails; an
    /// open one's rails joined at their ends, one ring.
    pub fn outline(&self) -> Vec<Polyline> {
        if self.closed {
            vec![self.a.clone(), self.b.clone()]
        } else {
            let mut ring = self.a.clone();
            ring.extend(self.b.iter().rev());
            vec![ring]
        }
    }
}

#[derive(Clone, Debug)]
pub struct SatinParams {
    /// Distance between zigzag peaks along the column, units.
    pub spacing: f64,
    /// Extra width per side, units.
    pub pull: f64,
    /// Underlay passes (`params::underlay` flags).
    pub underlay: u8,
    /// Underlay stitch length, units.
    pub run_len: f64,
    /// Shorten alternate stitches where needle points on a rail crowd together.
    pub short_stitches: bool,
    /// Split top stitches longer than this, units (auto-split for wide columns).
    pub split: Option<f64>,
    /// How the top stitches cross the column.
    pub style: SatinStyle,
    /// How far the top stitches lean from square across the column,
    /// radians (0: straight across). Positive leans the far rail's end of
    /// each stitch on along the column.
    pub lean: f64,
    /// Share each step along the column evenly between the stitch over
    /// and the stitch back (a true zigzag). Otherwise one goes straight
    /// across and the other takes the whole step.
    pub even: bool,
}

/// The pattern a satin column's top stitches make (Ink/Stitch's satin,
/// S-stitch and zigzag satin).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SatinStyle {
    /// Plain satin: each stitch straight across.
    #[default]
    Plain,
    /// S-stitch: every stitch across is split into a shallow S (three
    /// stitches, the middle one offset), a curvy, less shiny texture.
    SStitch,
    /// Zigzag: a lighter, open column, as a zigzag stitch rather than a
    /// solid satin: the spacing is multiplied up and nothing is split.
    Zigzag,
}

impl SatinStyle {
    pub fn name(self) -> &'static str {
        match self {
            SatinStyle::Plain => "satin",
            SatinStyle::SStitch => "s",
            SatinStyle::Zigzag => "zigzag",
        }
    }
    pub fn parse(s: &str) -> Option<SatinStyle> {
        match s {
            "satin" | "plain" => Some(SatinStyle::Plain),
            "s" | "sstitch" | "s-stitch" => Some(SatinStyle::SStitch),
            "zigzag" | "zz" => Some(SatinStyle::Zigzag),
            _ => None,
        }
    }
}

/// The most of a column's width a short stitch stops short by: where its
/// rail doesn't advance at all (the stitches fan from a corner), the
/// stitches either side of it are a full step apart half way across.
const SHORT_MOST: f64 = 0.5;
/// Underlay sits this far inside the rails (units), at most a quarter of the width.
const UNDERLAY_INSET: f64 = 4.0;
/// Zigzag underlay peak spacing, units.
const ZIGZAG_UNDERLAY_SPACING: f64 = 30.0;

struct Arc<'a> {
    pts: &'a [P],
    cum: Vec<f64>,
}

impl<'a> Arc<'a> {
    fn new(pts: &'a [P]) -> Self {
        let mut cum = Vec::with_capacity(pts.len());
        let mut s = 0.0;
        for (i, p) in pts.iter().enumerate() {
            if i > 0 {
                s += pts[i - 1].dist(*p);
            }
            cum.push(s);
        }
        Arc { pts, cum }
    }
    fn len(&self) -> f64 {
        *self.cum.last().unwrap_or(&0.0)
    }
    fn at(&self, s: f64) -> P {
        let n = self.pts.len();
        if n == 1 {
            return self.pts[0];
        }
        let s = s.clamp(0.0, self.len());
        let i = self.cum.partition_point(|&c| c <= s).clamp(1, n - 1);
        let seg = self.cum[i] - self.cum[i - 1];
        let t = if seg > 1e-12 {
            (s - self.cum[i - 1]) / seg
        } else {
            0.0
        };
        self.pts[i - 1].lerp(self.pts[i], t)
    }
}

/// A column's length along its center, and where on each rail a distance
/// along it falls. Between sync points (rungs, or the offsets of a
/// centerline) each rail is followed in proportion, so stitches stay square
/// to the column even where one rail is much longer than the other (round
/// a bend).
struct Along<'a> {
    a: Arc<'a>,
    b: Arc<'a>,
    /// Sync points as arc lengths on (a, b), first to last.
    syncs: Vec<(f64, f64)>,
    /// The center distance at each sync point.
    at: Vec<f64>,
    /// The distance along the longer rail at each sync point: of each
    /// stretch between sync points, whichever rail is longer there.
    long: Vec<f64>,
    closed: bool,
}

impl<'a> Along<'a> {
    fn new(r: &'a Rails) -> Option<Along<'a>> {
        if r.a.len() < 2 || r.b.len() < 2 {
            return None;
        }
        let (a, b) = (Arc::new(&r.a), Arc::new(&r.b));
        let mut syncs = vec![(0.0, 0.0)];
        for &(sa, sb) in &r.sync {
            let (pa, pb) = *syncs.last().unwrap();
            if sa > pa && sb > pb && sa < a.len() && sb < b.len() {
                syncs.push((sa, sb));
            }
        }
        syncs.push((a.len(), b.len()));
        let (mut at, mut long) = (vec![0.0], vec![0.0]);
        for w in syncs.windows(2) {
            let (da, db) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            at.push(at.last().unwrap() + (da + db) * 0.5);
            long.push(long.last().unwrap() + da.max(db));
        }
        Some(Along {
            a,
            b,
            syncs,
            at,
            long,
            closed: r.closed,
        })
    }

    fn total(&self) -> f64 {
        *self.at.last().unwrap()
    }

    /// The column's length along its longer rail.
    fn longest(&self) -> f64 {
        *self.long.last().unwrap()
    }

    /// The center distance where the longer rail is `u` along: equal steps
    /// of `u` put needle points no further apart than the step on either
    /// rail, and exactly the step apart on the outside of a bend. (Before
    /// the column's start and past its end, one for one.)
    fn at_long(&self, u: f64) -> f64 {
        let longest = self.longest();
        if u <= 0.0 {
            return u;
        }
        if u >= longest {
            return self.total() + (u - longest);
        }
        let i = self
            .long
            .partition_point(|&c| c <= u)
            .clamp(1, self.long.len() - 1);
        let span = self.long[i] - self.long[i - 1];
        let t = if span > 1e-12 {
            (u - self.long[i - 1]) / span
        } else {
            0.0
        };
        self.at[i - 1] + t * (self.at[i] - self.at[i - 1])
    }

    /// The arc lengths on the rails at center distance `s` (past either
    /// end: the end, or on round a closed column).
    fn arcs(&self, s: f64) -> (f64, f64) {
        let total = self.total();
        let s = if self.closed && total > 0.0 {
            s.rem_euclid(total)
        } else {
            s.clamp(0.0, total)
        };
        let i = self
            .at
            .partition_point(|&c| c <= s)
            .clamp(1, self.at.len() - 1);
        let span = self.at[i] - self.at[i - 1];
        let t = if span > 1e-12 {
            (s - self.at[i - 1]) / span
        } else {
            0.0
        };
        let ((a0, b0), (a1, b1)) = (self.syncs[i - 1], self.syncs[i]);
        (a0 + t * (a1 - a0), b0 + t * (b1 - b0))
    }

    /// The matched points on the rails at center distance `s`.
    fn pair(&self, s: f64) -> (P, P) {
        let (sa, sb) = self.arcs(s);
        (self.a.at(sa), self.b.at(sb))
    }
}

/// A column run the other way: from its end to its start.
pub fn reversed(r: &Rails) -> Rails {
    let (la, lb) = (polyline_length(&r.a), polyline_length(&r.b));
    Rails {
        a: r.a.iter().rev().copied().collect(),
        b: r.b.iter().rev().copied().collect(),
        closed: r.closed,
        sync: r
            .sync
            .iter()
            .rev()
            .map(|&(sa, sb)| (la - sa, lb - sb))
            .collect(),
    }
}

/// Matched points on both rails, evenly about `step` apart along the column
/// center.
pub fn pairs(r: &Rails, step: f64) -> Vec<(P, P)> {
    let Some(al) = Along::new(r) else {
        return Vec::new();
    };
    let total = al.total();
    let n = (total / step).ceil().max(1.0) as usize;
    let step = total / n as f64;
    let mut out: Vec<(P, P)> = (0..n).map(|k| al.pair(k as f64 * step)).collect();
    out.push((al.a.at(al.a.len()), al.b.at(al.b.len())));
    out
}

/// A column's flat end is a run of its outline from its tip to a corner
/// that turns at least [`FACE_TURN`] (radians, within [`FACE_CORNER`]
/// units), bending no more than [`FACE_BEND`] in all on the way, from 0.6
/// to 1.7 times as long as the column is wide there, and within 55° of
/// square to it ([`FACE_SHEAR`]: the cosine of the angle from the column's
/// own direction).
const FACE_BEND: f64 = 0.7;
const FACE_TURN: f64 = 0.5;
const FACE_CORNER: f64 = 2.0;
const FACE_LONG: (f64, f64) = (0.6, 1.7);
const FACE_SHEAR: f64 = 0.82;
/// How far the column's other side may curve in toward a flat end, of
/// the column's width.
const FACE_SAG: f64 = 0.12;
/// The stitches turn to lie along a flat end over this many times its
/// length.
pub(crate) const FACE_TURNING: f64 = 1.5;

/// `r`, a column made from a shape's own outline, with its ends where the
/// shape's are: `None` if they already are.
///
/// A stroke cut off at a slant (the foot of an e, level where the stroke
/// is still climbing; the arm of a k; the top of a v) has a sharp corner
/// and a blunt one, and what finds the column's ends takes the sharp one
/// for its tip: the cut then lies along one side, and the stitches fan in
/// to the point. The end is the cut. It's taken off the rail it was left
/// on, so the last stitch lies along it, and the stitches turn to meet it
/// over the last of the column instead of all at once: the same for every
/// column, however it was made.
pub fn flat_ends(r: &Rails, comp: &[Polyline]) -> Option<Rails> {
    if r.closed {
        return None;
    }
    let turned = |r: &Rails| -> Rails {
        let (la, lb) = (polyline_length(&r.a), polyline_length(&r.b));
        Rails {
            a: r.a.iter().rev().copied().collect(),
            b: r.b.iter().rev().copied().collect(),
            closed: false,
            sync: r
                .sync
                .iter()
                .rev()
                .map(|&(a, b)| (la - a, lb - b))
                .collect(),
        }
    };
    let mut out = r.clone();
    let mut changed = false;
    for _ in 0..2 {
        if let Some(flat) = flat_start(&out, comp) {
            out = flat;
            changed = true;
        }
        out = turned(&out);
    }
    changed.then_some(out)
}

/// [`flat_ends`], for the end a column starts at.
fn flat_start(r: &Rails, comp: &[Polyline]) -> Option<Rails> {
    let al = Along::new(r)?;
    let turn = |a: P, b: P| cross(a, b).atan2(a.dot(b));
    let tip = r.a[0].dist(r.b[0]);
    // How far `p` is from the segment from `a` to `b`.
    let off = |a: P, b: P, p: P| {
        let e = b - a;
        let len2 = e.dot(e);
        let t = if len2 > 0.0 {
            ((p - a).dot(e) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        a.lerp(b, t).dist(p)
    };
    // Only an end on the shape's own outline: one that's a cut across
    // the shape, where the column butts into another, stays where it is.
    let mid = r.a[0].lerp(r.b[0], 0.5);
    let on_outline = comp
        .iter()
        .flat_map(|ring| (0..ring.len()).map(move |i| (ring[i], ring[(i + 1) % ring.len()])))
        .any(|(a, b)| off(a, b, mid) < 0.3);
    if tip >= 1.0 && !on_outline {
        return None;
    }
    // The way a rail runs on from `from` along it, over half a width.
    let side = |x: &Arc, from: f64, width: f64| (x.at(from + 0.5 * width) - x.at(from)).norm();
    // A flat end left along rail `x`: how many of its points it runs to,
    // how long it is, and how wide the column is there.
    let face = |x: &Arc, y: &Arc| -> Option<(usize, f64, f64)> {
        let n = x.pts.len();
        // From the tip to the first corner the rail turns, however it
        // bends on the way (a piece of a round outline is an end too).
        let mut k = (1..n).find(|&i| x.cum[i] > 1e-6)?;
        let mut came = (x.pts[k] - x.pts[0]).norm();
        let mut bend = 0.0;
        loop {
            if k + 1 >= n {
                return None;
            }
            let leaves = (x.at(x.cum[k] + FACE_CORNER) - x.pts[k]).norm();
            if turn(came, leaves).abs() >= FACE_TURN {
                break;
            }
            if x.cum[k + 1] - x.cum[k] > 1e-6 {
                let on = (x.pts[k + 1] - x.pts[k]).norm();
                bend += turn(came, on).abs();
                if bend > FACE_BEND {
                    return None;
                }
                came = on;
            }
            k += 1;
        }
        let corner = x.pts[k];
        let long = x.pts[0].dist(corner);
        let width = y
            .pts
            .windows(2)
            .map(|w| off(w[0], w[1], corner))
            .fold(f64::INFINITY, f64::min);
        if width < 1.0
            || tip > 0.5 * long.min(width)
            || long < FACE_LONG.0 * width
            || long > (FACE_LONG.1 * width).min(AUTO_MAX_WIDTH)
        {
            return None;
        }
        // Its other side runs straight on to the tip, or bows out: one
        // that curves in toward the cut, as a serif's bracket does, is
        // the stroke flowing into a serif, whose own end is the column's.
        let far = y.at(long.min(y.len()));
        if far.dist(y.pts[0]) > 1e-6 {
            let along = (far - y.pts[0]).norm();
            let toward = cross(along, corner - y.pts[0]).signum();
            let sag = (1..8)
                .map(|i| cross(along, y.at(long * f64::from(i) / 8.0) - y.pts[0]) * toward)
                .fold(0.0, f64::max);
            if sag > FACE_SAG * width {
                return None;
            }
        }
        let sides = [side(x, x.cum[k], width), side(y, 0.0, width)];
        // (The corner by the way the rail leaves it; the column's
        // direction by the way its sides run over half its width.)
        let leaving = (x.at(x.cum[k] + (0.15 * width).max(1.5)) - corner).norm();
        let across = (corner - x.pts[0]).norm();
        if across.dot(leaving) > FACE_TURN.cos() || sides[0].dot(sides[1]) < 0.3 {
            return None;
        }
        let axis = (sides[0] + sides[1]).norm();
        (across.dot(axis).abs() <= FACE_SHEAR).then_some((k, long, width))
    };
    // (A column that comes to a point between two such runs alike is
    // pointed: neither is its end.)
    let (on_a, (k, long, width)) = match (face(&al.a, &al.b), face(&al.b, &al.a)) {
        (Some(a), Some(b)) if a.1 > 1.5 * b.1 => (true, a),
        (Some(a), Some(b)) if b.1 > 1.5 * a.1 => (false, b),
        (Some(a), None) => (true, a),
        (None, Some(b)) => (false, b),
        _ => return None,
    };
    // The rail the cut was on, from the cut's far corner; and the other
    // on to the cut's near one, past whatever short end the column had
    // there (a corner of it taken off).
    let (x, y) = if on_a { (&r.a, &r.b) } else { (&r.b, &r.a) };
    let on = x[k..].to_vec();
    let mut other = y.clone();
    if tip > 1e-6 {
        other.insert(0, x[0]);
    }
    let cut = if on_a { al.a.cum[k] } else { al.b.cum[k] };
    let (a, b, cut) = if on_a {
        (on, other, (cut, -tip))
    } else {
        (other, on, (-tip, cut))
    };
    let long = long.max(width);
    // The stitches square across from where the turn is over (a rung
    // there, from one rail to the nearest of the other), as they were
    // beyond it, and in proportion along each rail up to it.
    let margin = FACE_TURNING * long;
    let moved = |(sa, sb): (f64, f64)| (sa - cut.0, sb - cut.1);
    let (na, nb) = (Arc::new(&a), Arc::new(&b));
    let nearest = |arc: &Arc, p: P| -> f64 {
        let mut best = (f64::INFINITY, 0.0);
        for (i, w) in arc.pts.windows(2).enumerate() {
            let e = w[1] - w[0];
            let len2 = e.dot(e);
            let t = if len2 > 0.0 {
                ((p - w[0]).dot(e) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let d = w[0].lerp(w[1], t).dist(p);
            if d < best.0 {
                best = (d, arc.cum[i] + t * (arc.cum[i + 1] - arc.cum[i]));
            }
        }
        best.1
    };
    let steps = (na.len() / 2.0).ceil().max(1.0) as usize;
    let from = (1..steps)
        .map(|i| {
            let sa = na.len() * i as f64 / steps as f64;
            (sa, nearest(&nb, na.at(sa)))
        })
        .find(|&(sa, sb)| sa > 1.0 && sb > 1.0 && sb < nb.len() - 1.0 && (sa + sb) * 0.5 >= margin);
    let mut sync: Vec<(f64, f64)> = from.into_iter().collect();
    if let Some((fa, fb)) = from {
        sync.extend(
            r.sync
                .iter()
                .map(|&s| moved(s))
                .filter(|&(sa, sb)| sa > fa + 0.5 && sb > fb + 0.5),
        );
    }
    Some(Rails {
        a,
        b,
        closed: false,
        sync,
    })
}

/// How far columns' stitches are from square across them, on average,
/// degrees: 0 when each crosses its column at right angles. (A column
/// runs, at a stitch, the way the middles of the stitches either side of
/// it do.) Rails followed in proportion to their lengths drift out of step
/// where one turns a corner the other doesn't, and every stitch between
/// the corners leans.
pub fn lean(columns: &[Rails]) -> f64 {
    let (mut sum, mut count) = (0.0, 0usize);
    for r in columns {
        let ps = pairs(r, 8.0);
        let n = ps.len();
        if n < 3 {
            continue;
        }
        let mid = |k: usize| ps[k].0.lerp(ps[k].1, 0.5);
        for (k, &(a, b)) in ps.iter().enumerate() {
            let (before, after) = if r.closed {
                ((k + n - 1) % n, (k + 1) % n)
            } else if k == 0 || k == n - 1 {
                // (An open column's end stitches lie along its ends.)
                continue;
            } else {
                (k - 1, k + 1)
            };
            let along = mid(after) - mid(before);
            let across = b - a;
            if along.len() < 1e-6 || across.len() < 1.0 {
                continue;
            }
            let cos = (along.x * across.x + along.y * across.y) / (along.len() * across.len());
            sum += cos.abs().min(1.0).asin().to_degrees();
            count += 1;
        }
    }
    if count == 0 { 0.0 } else { sum / count as f64 }
}

/// A column with the fold taken out of its end. A ring opened at a seam
/// (a bowl, cut across where it's joined) may come as a column that
/// closes up on its own first stitch: each rail runs round to where it
/// began, and on from there along the seam to meet the other in a point
/// at the middle of it. There's nothing between them along the seam to
/// sew, and every stitch into the point lies along it, one on another: a
/// ridge of thread on a line. The column ends where each rail comes back
/// to its start instead, flat across the seam as it began.
pub fn unfolded(r: &Rails) -> Rails {
    let mut r = r.clone();
    if r.closed {
        return r;
    }
    // How many points back from a rail's end (`last`, or on from its
    // start) the rail passes the place its other end is, if it runs
    // straight from there into this end; and which way.
    let fold = |line: &[P], last: bool| -> Option<(usize, P)> {
        let n = line.len();
        let at = |k: usize| if last { line[n - 1 - k] } else { line[k] };
        let k = (1..n.saturating_sub(1)).find(|&k| at(k).dist(at(n - 1)) < 1.0)?;
        let out = at(k) - at(0);
        if out.len() < 1e-6 {
            return None;
        }
        let out = out.norm();
        (1..k)
            .all(|i| (at(i) - at(0)).dot(out.perp()).abs() < 0.5)
            .then_some((k, out))
    };
    for last in [true, false] {
        let (na, nb) = (r.a.len(), r.b.len());
        if na < 3 || nb < 3 {
            break;
        }
        let (ea, eb) = if last {
            (r.a[na - 1], r.b[nb - 1])
        } else {
            (r.a[0], r.b[0])
        };
        if ea.dist(eb) >= 1.0 {
            continue;
        }
        let (Some((ka, da)), Some((kb, db))) = (fold(&r.a, last), fold(&r.b, last)) else {
            continue;
        };
        // (From opposite sides: the two halves of the seam.)
        if da.dot(db) > -0.98 {
            continue;
        }
        if last {
            r.a.truncate(na - ka);
            r.b.truncate(nb - kb);
            let (la, lb) = (polyline_length(&r.a), polyline_length(&r.b));
            r.sync.retain(|&(sa, sb)| sa < la && sb < lb);
        } else {
            let (cut_a, cut_b) = (polyline_length(&r.a[..=ka]), polyline_length(&r.b[..=kb]));
            r.a.drain(..ka);
            r.b.drain(..kb);
            r.sync = r
                .sync
                .iter()
                .map(|&(sa, sb)| (sa - cut_a, sb - cut_b))
                .filter(|&(sa, sb)| sa > 0.0 && sb > 0.0)
                .collect();
        }
    }
    r
}

/// A column's end that's a cut further than this from square across it
/// (degrees) is a point instead: see [`pointed`]. (Up to here its last
/// stitches turn to lie along the cut, none of them further from square
/// than a stitch may lean; past it they'd have to.)
pub const POINT_SHEAR: f64 = 35.0;

/// An open column whose ends cut on a steep slant are points instead:
/// where its last stitch would lie more than `shear` degrees from square
/// across it (a stroke that ends against the side of another at a sharp
/// angle: the diagonals of a W, the arm of a k), the rail that stops
/// short runs on along the cut to the other's end. The column covers
/// what it did, but the cut is its side now, not its end: its stitches
/// stay square to it and shorten into the point, where they'd have swung
/// round to lie along the cut.
///
/// Only an end that's a cut through the shape (`cut` says, of the line
/// between the rails' ends): one that lies along the shape's own outline
/// is the stroke's end, and its stitches lie along it.
pub fn pointed(r: &Rails, shear: f64, cut: impl Fn(P, P) -> bool) -> Rails {
    let mut r = r.clone();
    if r.closed || r.a.len() < 2 || r.b.len() < 2 {
        return r;
    }
    // The way a rail runs into its end (`last`, or its start): from as far
    // back along it as the end is wide.
    let into = |line: &[P], last: bool, back: f64| -> P {
        let n = line.len();
        let at = |k: usize| if last { line[n - 1 - k] } else { line[k] };
        let (mut gone, mut k) = (0.0, 0);
        while k + 1 < n && gone + at(k).dist(at(k + 1)) < back {
            gone += at(k).dist(at(k + 1));
            k += 1;
        }
        let far = if k + 1 < n {
            let d = at(k).dist(at(k + 1));
            at(k).lerp(at(k + 1), if d > 1e-9 { (back - gone) / d } else { 0.0 })
        } else {
            at(k)
        };
        (at(0) - far).norm()
    };
    let limit = shear.to_radians().sin();
    for last in [true, false] {
        let (a, b) = if last {
            (r.a[r.a.len() - 1], r.b[r.b.len() - 1])
        } else {
            (r.a[0], r.b[0])
        };
        let width = a.dist(b);
        if width < 1.0 || !cut(a, b) {
            continue;
        }
        let out = into(&r.a, last, width) + into(&r.b, last, width);
        if out.len() < 1e-6 {
            continue;
        }
        // (How far rail b's end is ahead of rail a's, as a share of the
        // end's width: the sine of how far the end is from square.)
        let ahead = (b - a).dot(out.norm()) / width;
        if ahead.abs() <= limit {
            continue;
        }
        match (last, ahead > 0.0) {
            (true, true) => r.a.push(b),
            (true, false) => r.b.push(a),
            (false, true) => {
                // (Rail b's start is further out: it's the one that's
                // ahead, looking out of this end.)
                r.a.insert(0, b);
                r.sync.iter_mut().for_each(|s| s.0 += width);
            }
            (false, false) => {
                r.b.insert(0, a);
                r.sync.iter_mut().for_each(|s| s.1 += width);
            }
        }
    }
    r
}

/// How far apart along a rail [`square_rungs`] looks for a rung, and how
/// far either side of a point a rail's direction there is taken over
/// (units).
const SQUARE_STEP: f64 = 5.0;
const SQUARE_TANGENT: f64 = 2.0;

/// Rungs that keep a column's stitches square to it, whatever its shape:
/// each crosses from one rail to the other at the same angle to both (the
/// chord of the circle that fits between them there), so on a straight it
/// is square across, on a taper or a swelling it splits the difference,
/// and round a corner the rungs fan. From every [`SQUARE_STEP`] along each
/// rail; those that are in step along both; and none nearer an open
/// column's end than the rails are out of step there, or than the column
/// is wide (its ends lie as the shape's do: square, and the rungs run
/// right up to it; on a slant, and the stitches come round to it over as
/// far as it sets one rail ahead).
pub fn square_rungs(r: &Rails) -> Vec<(f64, f64)> {
    square_rungs_bent(r, SQUARE_BEND_WEIGHT)
}

/// [`square_rungs`], with a bend of a radian in a rail counting `bent`
/// against its direction.
fn square_rungs_bent(r: &Rails, bent: f64) -> Vec<(f64, f64)> {
    if r.a.len() < 2 || r.b.len() < 2 {
        return Vec::new();
    }
    let (a, b) = (Arc::new(&r.a), Arc::new(&r.b));
    let tangent = |line: &Arc, s: f64| {
        let l = line.len();
        let at = |s: f64| line.at(if r.closed { s.rem_euclid(l) } else { s });
        (at(s + SQUARE_TANGENT) - at(s - SQUARE_TANGENT)).norm()
    };
    // How straight a rail runs at `s`: 1 for a straight one, less the more
    // it turns over [`SQUARE_BEND`] either side.
    let straight = |line: &Arc, s: f64| -> f64 {
        let l = line.len();
        let at = |s: f64| {
            line.at(if r.closed {
                s.rem_euclid(l)
            } else {
                s.clamp(0.0, l)
            })
        };
        let (u, v) = (
            (at(s) - at(s - SQUARE_BEND)).norm(),
            (at(s + SQUARE_BEND) - at(s)).norm(),
        );
        let turn = cross(u, v).atan2(u.dot(v)).abs();
        1.0 / (1.0 + bent * turn)
    };
    // Whether the column comes to a point at its start, and at its end.
    let point = (
        r.a[0].dist(r.b[0]) < 1.0,
        r.a[r.a.len() - 1].dist(r.b[r.b.len() - 1]) < 1.0,
    );
    let mut found: Vec<(f64, f64)> = Vec::new();
    for (near, far, swap) in [(&a, &b, false), (&b, &a, true)] {
        let (ln, lf) = (near.len(), far.len());
        let n = (ln / SQUARE_STEP).floor() as usize;
        for k in 1..n {
            let s = k as f64 * ln / n as f64;
            let (p, tn) = (near.at(s), tangent(near, s));
            // Square across from this rail first: which side the other
            // rail is, and about where.
            let Some((_, across, mut t)) = [tn.perp(), -tn.perp()]
                .into_iter()
                .filter_map(|d| {
                    super::columns::ray_on_line(p, d, far.pts).map(|(len, t)| (len, d, t))
                })
                .min_by(|x, y| x.0.total_cmp(&y.0))
            else {
                continue;
            };
            // Then at the same angle to both rails.
            let mut len = f64::INFINITY;
            for _ in 0..3 {
                let along = tn * straight(near, s) + tangent(far, t) * straight(far, t);
                if along.len() < 0.5 {
                    len = f64::INFINITY;
                    break;
                }
                let d = along.norm().perp();
                let d = if d.dot(across) < 0.0 { -d } else { d };
                match super::columns::ray_on_line(p, d, far.pts) {
                    Some((l, at)) => (len, t) = (l, at),
                    None => {
                        len = f64::INFINITY;
                        break;
                    }
                }
            }
            if len > AUTO_MAX_WIDTH * 1.2 {
                continue;
            }
            // No nearer an end than the rails are out of step there: the
            // stitches come round to a slanted end over as far as the
            // slant sets one rail ahead of the other (and right up to a
            // square one), or over the column's width if that's less (an
            // end cut so steep it runs along the column for a way).
            // (Nor does that hold at an end that's a point: there's no
            // last stitch there to come round to, and left to follow the
            // rails in proportion the stitches into it lie along it, a
            // hair apart, where square across it they shorten into it.)
            let (before, after) = ((s - t).abs(), ((ln - s) - (lf - t)).abs());
            if !r.closed
                && ((!point.0 && s.min(t) < before.min(len))
                    || (!point.1 && (ln - s).min(lf - t) < after.min(len)))
            {
                continue;
            }
            found.push(if swap { (t, s) } else { (s, t) });
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

/// How far either side of a point a rail's bend is measured (units), and
/// how much a bend of a radian there counts against it, for weighting the
/// two rails' directions in [`square_rungs`]: stitches cross a stroke
/// square to the side of it that runs straight, where the other bulges.
const SQUARE_BEND: f64 = 8.0;
const SQUARE_BEND_WEIGHT: f64 = 4.0;

/// A column needs no rung nearer than this (units along either rail) to
/// one it has.
const SQUARE_GAP: f64 = 10.0;

/// A column with [`square_rungs`] wherever it has none of its own: the
/// rungs it came with are kept (whatever found the column knew where its
/// strokes fork and end), and between them, where the rails would only be
/// followed in proportion, each square rung that's in step with them.
pub fn squared(r: &Rails) -> Rails {
    let mut sync = r.sync.clone();
    for (sa, sb) in square_rungs(r) {
        let clear = r
            .sync
            .iter()
            .all(|o| (o.0 - sa).abs() >= SQUARE_GAP && (o.1 - sb).abs() >= SQUARE_GAP);
        let i = sync.partition_point(|x| x.0 < sa);
        let after = i == 0 || (sync[i - 1].0 + 0.5 < sa && sync[i - 1].1 + 0.5 < sb);
        let before = i == sync.len() || (sa + 0.5 < sync[i].0 && sb + 0.5 < sync[i].1);
        if clear && after && before {
            sync.insert(i, (sa, sb));
        }
    }
    Rails {
        a: r.a.clone(),
        b: r.b.clone(),
        closed: r.closed,
        sync,
    }
}

/// A ring (a shape and its hole, sewn round between the two outlines)
/// whose stitches lean more than this on average, degrees, is cut into
/// columns instead, if those are squarer: a triangle with a triangle cut
/// out of it, a D. (An O's lean a few degrees where its stroke thickens.)
pub const RING_LEAN: f64 = 15.0;

/// Where a column's top stitches go: matched points on both rails, about
/// `step` apart along whichever rail is the longer there, so the outside
/// of a bend is covered as closely as a straight run is (and the inside
/// more closely: see the short stitches). Each stitch leans `lean` radians
/// from square across: its end on rail b is further along the column than
/// its end on rail a by the column's width there times tan(lean). At an
/// open column's ends the stitches fan from the corner, so the lean leaves
/// no gap.
pub fn pairs_leaning(r: &Rails, step: f64, lean: f64) -> Vec<(P, P)> {
    let t = lean.tan();
    let Some(al) = Along::new(r) else {
        return Vec::new();
    };
    let total = al.total();
    let longest = al.longest();
    if t.abs() < 1e-6 {
        let n = (longest / step).ceil().max(1.0) as usize;
        let mut out: Vec<(P, P)> = (0..n)
            .map(|k| al.pair(al.at_long(k as f64 * longest / n as f64)))
            .collect();
        out.push((al.a.at(al.a.len()), al.b.at(al.b.len())));
        return out;
    }
    // How far apart along the column a stitch's two ends are, where the
    // column is `s` along.
    let shift = |s: f64| {
        let (a, b) = al.pair(s);
        a.dist(b) * t
    };
    let widest = pairs(r, (total / 24.0).max(step))
        .iter()
        .map(|(a, b)| a.dist(*b))
        .fold(0.0, f64::max);
    // (A closed column has no ends to fan at.)
    let over = if al.closed {
        0.0
    } else {
        widest * t.abs() * 0.5
    };
    let span = longest + 2.0 * over;
    let n = (span / step).ceil().max(1.0) as usize;
    let step = span / n as f64;
    (0..=n)
        .map(|k| {
            let s = al.at_long(-over + k as f64 * step);
            let d = shift(s) * 0.5;
            (al.a.at(al.arcs(s - d).0), al.b.at(al.arcs(s + d).1))
        })
        .collect()
}

/// Sew a column: underlay passes, then the top satin, as one continuous path.
pub fn column(r: &Rails, p: &SatinParams) -> Polyline {
    column_lapped(r, p, &NO_LAPS)
}

/// Where a column's stitches run on past its start and past its end: a
/// row of them there, from rail a's side to rail b's. (Under the column
/// it ends against: see `laps` in the planner.)
pub type Laps = [Option<(P, P)>; 2];
/// A column sewn from one of its ends to the other and no further.
pub const NO_LAPS: Laps = [None, None];

/// [`column`], its top stitches run on past its ends where `laps` says.
pub fn column_lapped(r: &Rails, p: &SatinParams, laps: &Laps) -> Polyline {
    let mut out: Polyline = Vec::new();
    // Where the needle is: start (false) or end (true) of the column.
    let mut at_end = false;
    let inset = |a: P, b: P| {
        let w = a.dist(b);
        let d = UNDERLAY_INSET.min(w * 0.25);
        (a + (b - a).norm() * d, b + (a - b).norm() * d)
    };

    if p.underlay & underlay::CENTER != 0 {
        let mut pass: Polyline = pairs(r, p.run_len)
            .into_iter()
            .map(|(a, b)| a.lerp(b, 0.5))
            .collect();
        if at_end {
            pass.reverse();
        }
        out.extend(pass);
        at_end = !at_end;
    }
    if p.underlay & underlay::CONTOUR != 0 {
        let ps: Vec<(P, P)> = pairs(r, p.run_len)
            .into_iter()
            .map(|(a, b)| inset(a, b))
            .collect();
        let side_a: Polyline = ps.iter().map(|q| q.0).collect();
        let side_b: Polyline = ps.iter().map(|q| q.1).collect();
        // Out along one side and back along the other: ends where it started.
        if at_end {
            out.extend(side_a.iter().rev());
            out.extend(side_b.iter());
        } else {
            out.extend(side_a.iter());
            out.extend(side_b.iter().rev());
        }
    }
    if p.underlay & underlay::ZIGZAG != 0 {
        let mut pass: Polyline = Vec::new();
        for (i, (a, b)) in pairs(r, ZIGZAG_UNDERLAY_SPACING)
            .into_iter()
            .map(|(a, b)| inset(a, b))
            .enumerate()
        {
            pass.push(if i % 2 == 0 { a } else { b });
        }
        if at_end {
            pass.reverse();
        }
        out.extend(pass);
        at_end = !at_end;
    }

    // Top stitching.
    let spacing = match p.style {
        SatinStyle::Zigzag => p.spacing * 3.0,
        _ => p.spacing,
    };
    let mut top = if p.even {
        top_even(r, spacing, p)
    } else {
        top_square(r, spacing, p)
    };
    lap_rows(&mut top, r, p, laps);
    match p.style {
        SatinStyle::Plain => {
            if let Some(max) = p.split.filter(|m| *m > 0.0) {
                top = split_long(&top, max);
            }
        }
        SatinStyle::SStitch => top = s_stitch(&top),
        SatinStyle::Zigzag => {}
    }
    if at_end {
        top.reverse();
    }
    out.extend(top);
    out
}

/// Run a column's top stitches on past its ends where `laps` says: after
/// the first stitch, out to a row across there and back; and the same
/// before the last. The column's first and last stitches are the ones it
/// had, so what's sewn before and after it is as it was (which side it's
/// left from is chosen by its last stitch), and the row lies under the
/// column it ends against.
fn lap_rows(top: &mut Polyline, r: &Rails, p: &SatinParams, laps: &Laps) {
    if r.closed || top.len() < 4 || r.a.is_empty() || r.b.is_empty() {
        return;
    }
    let pulled = |(a, b): (P, P)| pulled_out(a, b, p.pull);
    // (A stitch crosses the column each time: the row starts on the side
    // the needle isn't on.)
    if let Some(row) = laps[1] {
        let (a, b) = pulled(row);
        let n = top.len();
        let on_a = top[n - 1].dist(r.a[r.a.len() - 1]) <= top[n - 1].dist(r.b[r.b.len() - 1]);
        top.splice(n - 2..n - 2, if on_a { [b, a] } else { [a, b] });
    }
    if let Some(row) = laps[0] {
        let (a, b) = pulled(row);
        let on_a = top[0].dist(r.a[0]) <= top[0].dist(r.b[0]);
        top.splice(2..2, if on_a { [a, b] } else { [b, a] });
    }
}

/// The most a stitch is lengthened at each end to make up for the fabric
/// pulling in (pull compensation), as a share of its own length: the pull
/// is the thread's along the stitch, so a narrow column pulls in less. On
/// a satin a few millimetres wide the setting's pull is less than this; on
/// a hairline, a serif or a script letter's thin stroke half a millimetre
/// across, the full pull would sew it nearly twice its width.
const PULL_SHARE: f64 = 0.1;

/// A stitch from `a` to `b` lengthened by `pull` at each end, but by no
/// more than [`PULL_SHARE`] of its length.
fn pulled_out(a: P, b: P, pull: f64) -> (P, P) {
    let dir = (a - b).norm();
    let pull = pull.min(PULL_SHARE * a.dist(b));
    (a + dir * pull, b - dir * pull)
}

/// How far across the column (as a fraction of its width) a stitch
/// stops short of a rail where the needle points on it crowd: `near` is
/// how far apart they are there, `far` how far apart on the other rail,
/// and `every` which stitches go further than this one (every 2nd, 4th or
/// 8th). It stops where those are as far apart as stitches are on the far
/// rail: short of there they'd leave a gap, and beyond it the thread only
/// piles up. (Nothing, until the near rail's points are that many times
/// closer than the far rail's.)
fn short(near: f64, far: f64, every: f64) -> f64 {
    if far <= 1e-9 || near >= far / every {
        return 0.0;
    }
    ((far / every - near) / (far - near)).clamp(0.0, SHORT_MOST)
}

/// How far apart the needle points on each rail are about the `k`th of
/// `ps` (`stride` apart are on one rail): on average over the steps up to
/// [`SPACING_OVER`] either side, not the one step, as how crowded they are
/// is the curve's, and a column's rungs can put one step a little short
/// and the next a little long (a stitch cut short at one of those, and
/// not its neighbours, is a notch in the edge).
fn needle_gaps(ps: &[(P, P)], k: usize, stride: usize) -> (f64, f64) {
    let lo = k.saturating_sub(SPACING_OVER * stride).max(stride);
    let hi = (k + SPACING_OVER * stride).min(ps.len() - 1);
    let (mut da, mut db, mut n) = (0.0, 0.0, 0.0);
    let mut i = lo;
    while i <= hi {
        da += ps[i].0.dist(ps[i - stride].0);
        db += ps[i].1.dist(ps[i - stride].1);
        n += 1.0;
        i += stride;
    }
    if n == 0.0 {
        return (
            ps[k].0.dist(ps[k - stride].0),
            ps[k].1.dist(ps[k - stride].1),
        );
    }
    (da / n, db / n)
}

/// How many steps either side of a stitch its rails' spacing is averaged
/// over, for short stitches (see [`needle_gaps`]).
const SPACING_OVER: usize = 2;

/// Which stitches go further than the `j`th along a rail, where its
/// needle points crowd: every other stitch stops shortest (the stitches
/// either side of it go on), every other of the rest stops less short,
/// and so on; every eighth runs the whole way, to the rail itself.
fn short_every(j: usize) -> Option<f64> {
    match j % 8 {
        0 => None,
        4 => Some(8.0),
        2 | 6 => Some(4.0),
        _ => Some(2.0),
    }
}

/// The top stitches with one straight across the column and the next
/// taking the whole step along it: every needle point on rail a is square
/// across from one on rail b.
fn top_square(r: &Rails, spacing: f64, p: &SatinParams) -> Polyline {
    let ps = pairs_leaning(r, spacing, p.lean);
    let mut top: Polyline = Vec::with_capacity(ps.len() * 2);
    for (k, &(a, b)) in ps.iter().enumerate() {
        let (mut a2, mut b2) = pulled_out(a, b, p.pull);
        if let (true, true, Some(every)) = (p.short_stitches, k > 0, short_every(k)) {
            let (da, db) = needle_gaps(&ps, k, 1);
            let (fa, fb) = (short(da, db, every), short(db, da, every));
            (a2, b2) = (a2.lerp(b2, fa), b2.lerp(a2, fb));
        }
        top.push(a2);
        top.push(b2);
    }
    top
}

/// The top stitches as a true zigzag: the needle points on rail b fall
/// halfway between those on rail a, so the stitch over and the stitch
/// back each take half the step and slant the same amount, one each way.
/// (Straight across and a full step back, on a narrow column, shows as
/// one clean stitch and one slanting one, and a ragged edge.) An open
/// column begins and ends with a stitch straight across, so its ends are
/// square.
fn top_even(r: &Rails, spacing: f64, p: &SatinParams) -> Polyline {
    // Matched points every half step: rail a's needle points are the even
    // ones, rail b's the odd.
    let ps = pairs_leaning(r, spacing * 0.5, p.lean);
    let pulled = |k: usize| pulled_out(ps[k].0, ps[k].1, p.pull);
    let mut top: Polyline = Vec::with_capacity(ps.len() + 2);
    for k in 0..ps.len() {
        let (a2, b2) = pulled(k);
        let on_a = k % 2 == 0;
        let (mut q, other) = if on_a { (a2, b2) } else { (b2, a2) };
        // Needle points on a rail are pulled in where they crowd (the
        // inside of a tight curve).
        if let (true, true, Some(every)) = (p.short_stitches, k >= 2, short_every(k / 2)) {
            let (da, db) = needle_gaps(&ps, k, 2);
            let (near, far) = if on_a { (da, db) } else { (db, da) };
            q = q.lerp(other, short(near, far, every));
        }
        if k == 0 && !r.closed {
            top.push(other);
        }
        top.push(q);
        if k + 1 == ps.len() && !r.closed {
            top.push(other);
        }
    }
    top
}

/// S-stitch: each stitch across the column becomes three, the middle
/// point pushed sideways (alternating sides), so the column reads as a
/// run of shallow S curves.
fn s_stitch(pts: &[P]) -> Polyline {
    let mut out = Vec::with_capacity(pts.len() * 3);
    for (k, w) in pts.windows(2).enumerate() {
        if k == 0 {
            out.push(w[0]);
        }
        let d = w[1] - w[0];
        let len = d.len();
        if len > 6.0 {
            let n = d.norm();
            let side = P::new(-n.y, n.x) * (len * 0.18).min(8.0);
            let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
            out.push(w[0].lerp(w[1], 0.3) + side * sign);
            out.push(w[0].lerp(w[1], 0.7) - side * sign);
        }
        out.push(w[1]);
    }
    if pts.len() == 1 {
        out.push(pts[0]);
    }
    out
}

/// Auto-split: break stitches longer than `max` into shorter ones, staggering
/// the split points from stitch to stitch so they don't line up into a groove.
fn split_long(pts: &[P], max: f64) -> Polyline {
    let mut out = Vec::with_capacity(pts.len() * 2);
    for (k, w) in pts.windows(2).enumerate() {
        if k == 0 {
            out.push(w[0]);
        }
        let len = w[0].dist(w[1]);
        let n = (len / max).ceil() as usize;
        if n > 1 {
            let shift = [0.0, 0.33, -0.33][k % 3] / n as f64;
            for j in 1..n {
                out.push(w[0].lerp(w[1], j as f64 / n as f64 + shift));
            }
        }
        out.push(w[1]);
    }
    if pts.len() == 1 {
        out.push(pts[0]);
    }
    out
}

/// A column cut in two across itself, at the stitch nearest `p`: the part
/// up to there and the part from there on, each running the way the whole
/// did. `None` for a ring, or where either part would be shorter than
/// `least` along its middle.
pub fn split_at(r: &Rails, p: P, least: f64) -> Option<(Rails, Rails)> {
    if r.closed {
        return None;
    }
    let al = Along::new(r)?;
    let total = al.total();
    let steps = (total / 2.0).ceil().max(1.0) as usize;
    let (mut best, mut best_d) = (0.0, f64::INFINITY);
    for k in 0..=steps {
        let s = total * k as f64 / steps as f64;
        let (sa, sb) = al.arcs(s);
        let d = al.a.at(sa).lerp(al.b.at(sb), 0.5).dist(p);
        if d < best_d {
            (best, best_d) = (s, d);
        }
    }
    if best < least || total - best < least {
        return None;
    }
    let (sa, sb) = al.arcs(best);
    let cut = |arc: &Arc, s: f64| -> (Polyline, Polyline) {
        let at = arc.at(s);
        let i = arc.cum.partition_point(|&c| c < s);
        let mut head: Polyline = arc.pts[..i].to_vec();
        if head.last().is_none_or(|l| l.dist(at) > 1e-6) {
            head.push(at);
        }
        let mut tail: Polyline = vec![at];
        tail.extend(arc.pts[i..].iter().filter(|q| q.dist(at) > 1e-6));
        (head, tail)
    };
    let ((a0, a1), (b0, b1)) = (cut(&al.a, sa), cut(&al.b, sb));
    if a0.len() < 2 || a1.len() < 2 || b0.len() < 2 || b1.len() < 2 {
        return None;
    }
    let before = r
        .sync
        .iter()
        .copied()
        .filter(|&(a, b)| a < sa && b < sb)
        .collect();
    let after = r
        .sync
        .iter()
        .filter(|&&(a, b)| a > sa && b > sb)
        .map(|&(a, b)| (a - sa, b - sb))
        .collect();
    Some((
        Rails {
            a: a0,
            b: b0,
            closed: false,
            sync: before,
        },
        Rails {
            a: a1,
            b: b1,
            closed: false,
            sync: after,
        },
    ))
}

/// The strip of a column between fractions `t0` and `t1` of its width
/// (rail a at 0, rail b at 1), as rails of its own: the rungs are kept,
/// so the strip's zigzags line up with the whole column's.
pub fn strip(r: &Rails, t0: f64, t1: f64) -> Rails {
    // Matched points along both rails, finely.
    let ps = pairs(r, 5.0);
    let a: Polyline = ps.iter().map(|(a, b)| a.lerp(*b, t0)).collect();
    let b: Polyline = ps.iter().map(|(a, b)| a.lerp(*b, t1)).collect();
    Rails {
        a,
        b,
        closed: r.closed,
        sync: Vec::new(),
    }
}

/// Rails for a satin of `width` along a single centerline.
pub fn rails_from_centerline(line: &[P], closed: bool, width: f64) -> Rails {
    let mut base: Polyline = line.to_vec();
    if closed && base.len() > 2 {
        base.push(base[0]);
    }
    // Resample finely so offset rails follow curves smoothly.
    let c = crate::geom::resample_keep_corners(&base, 5.0);
    let n = c.len();
    if n < 2 {
        // No length (a single point, or points all in one place): no column.
        return Rails {
            a: Vec::new(),
            b: Vec::new(),
            closed,
            sync: Vec::new(),
        };
    }
    let half = width * 0.5;
    let tangent = |i: usize| -> P {
        let prev = if i > 0 {
            c[i] - c[i - 1]
        } else if closed && n > 2 {
            c[0] - c[n - 2]
        } else {
            c[1] - c[0]
        };
        let next = if i + 1 < n {
            c[i + 1] - c[i]
        } else if closed && n > 2 {
            c[1] - c[0]
        } else {
            c[n - 1] - c[n - 2]
        };
        let (tp, tn) = (prev.norm(), next.norm());
        let avg = (tp + tn).norm();
        if avg.len() < 1e-9 { tn } else { avg }
    };
    let mut a: Polyline = Vec::with_capacity(n);
    let mut b: Polyline = Vec::with_capacity(n);
    // Each point's two offsets belong together: a sync at every one keeps the
    // stitches square to the centerline round bends, where the inner rail is
    // shorter than the outer.
    let mut sync = Vec::with_capacity(n);
    let (mut la, mut lb) = (0.0, 0.0);
    for i in 0..n {
        let t = tangent(i);
        let seg = if i + 1 < n {
            (c[i + 1] - c[i]).norm()
        } else {
            (c[i] - c[i.saturating_sub(1)]).norm()
        };
        let miter = (1.0 / t.dot(seg).abs().max(0.5)).min(2.0);
        let nrm = t.perp() * (half * miter);
        let (pa, pb) = (c[i] + nrm, c[i] - nrm);
        if let (Some(&qa), Some(&qb)) = (a.last(), b.last()) {
            la += pa.dist(qa);
            lb += pb.dist(qb);
            if i + 1 < n {
                sync.push((la, lb));
            }
        }
        a.push(pa);
        b.push(pb);
    }
    Rails { a, b, closed, sync }
}

/// Subpath ends nearer than this (units) are one point: a loop drawn
/// without being closed.
const LOOP_GAP: f64 = 0.5;

/// Two-rail satin from a stroke path with several subpaths (Ink/Stitch
/// style): two of them are the rails and the rest are rungs, which say
/// where on each rail a stitch across begins and ends. `marked` names the
/// rails (indices into `lines`), for a path that says which they are;
/// otherwise they're told apart by what crosses what (see [`pick_rails`]).
/// Returns `None` if the subpaths don't look like a pair of rails.
pub fn rails_from_subpaths(
    lines: &[(Polyline, bool)],
    marked: Option<(usize, usize)>,
) -> Option<Rails> {
    if lines.len() < 2 {
        return None;
    }
    let marked = marked.filter(|&(i, j)| i != j && i < lines.len() && j < lines.len());
    let (ia, ib) = marked.unwrap_or_else(|| pick_rails(lines));
    if lines[ia].0.len() < 2 || lines[ib].0.len() < 2 {
        return None;
    }
    let (la, lb) = (polyline_length(&lines[ia].0), polyline_length(&lines[ib].0));
    // (Rails that are told which they are needn't look alike.)
    if marked.is_none() && lb.min(la) < la.max(lb) * 0.4 {
        return None;
    }
    // A loop is one whether the path closes it or just ends where it began.
    let is_loop = |(l, closed): &(Polyline, bool)| {
        *closed || (l.len() > 3 && l[0].dist(*l.last().unwrap()) < LOOP_GAP)
    };
    let closed = is_loop(&lines[ia]) && is_loop(&lines[ib]);
    // (A closed rail: its first point again at the end.)
    let close = |l: &Polyline| {
        let mut l = l.clone();
        if closed && l.len() > 2 {
            if l[0].dist(*l.last().unwrap()) < LOOP_GAP {
                l.pop();
            }
            l.push(l[0]);
        }
        l
    };
    let mut a = close(&lines[ia].0);
    let mut b = close(&lines[ib].0);
    let rungs: Vec<&Polyline> = (0..lines.len())
        .filter(|&k| k != ia && k != ib)
        .map(|k| &lines[k].0)
        .collect();
    let mut sync: Vec<(f64, f64)> = Vec::new();
    if !closed {
        let same = a[0].dist(b[0]) + a.last().unwrap().dist(*b.last().unwrap());
        let flipped = a[0].dist(*b.last().unwrap()) + a.last().unwrap().dist(b[0]);
        if flipped < same {
            b.reverse();
        }
        for rung in &rungs {
            sync.extend(rung_syncs(&a, &b, rung));
        }
    } else {
        turn_alike(&a, &mut b);
        let mut all: Vec<(f64, f64)> = Vec::new();
        for rung in &rungs {
            all.extend(rung_syncs(&a, &b, rung));
        }
        all.sort_by(|x, y| x.0.total_cmp(&y.0));
        match all.first().copied() {
            // Both rails begin at the first rung, so the rest are measured
            // from a stitch that's known to be one. (Measured from wherever
            // each rail happened to begin, a rung just before one start and
            // just after the other pairs the whole of one rail with a
            // sliver of the other.)
            Some((a0, b0)) => {
                let (la, lb) = (polyline_length(&a), polyline_length(&b));
                a = rotate_loop(&a, a0);
                b = rotate_loop(&b, b0);
                sync = all[1..]
                    .iter()
                    .map(|&(sa, sb)| ((sa - a0).rem_euclid(la), (sb - b0).rem_euclid(lb)))
                    .collect();
            }
            // No rungs: rail b begins at its point nearest rail a's start.
            None => {
                let at = nearest_arc(&b, a[0]);
                b = rotate_loop(&b, at);
            }
        }
    }
    // Rails must be close relative to their length to be one column.
    if marked.is_none() {
        let mid_a = Arc::new(&a).at(polyline_length(&a) * 0.5);
        let mid_b = Arc::new(&b).at(polyline_length(&b) * 0.5);
        if mid_a.dist(mid_b) > 150.0 {
            return None;
        }
    }
    sync.sort_by(|x, y| x.0.total_cmp(&y.0));
    // Keep only rungs that advance along both rails.
    let mut kept: Vec<(f64, f64)> = Vec::new();
    for s in sync {
        if kept.last().is_none_or(|k| s.0 > k.0 && s.1 > k.1) {
            kept.push(s);
        }
    }
    Some(Rails {
        a,
        b,
        closed,
        sync: kept,
    })
}

/// Which two of `lines` [`rails_from_subpaths`] takes for the rails (the
/// rest are rungs), if they make a column.
pub fn rails_of_subpaths(
    lines: &[(Polyline, bool)],
    marked: Option<(usize, usize)>,
) -> Option<(usize, usize)> {
    rails_from_subpaths(lines, marked)?;
    let marked = marked.filter(|&(i, j)| i != j && i < lines.len() && j < lines.len());
    Some(marked.unwrap_or_else(|| pick_rails(lines)))
}

/// Which two of a satin path's subpaths are its rails. A rung crosses both
/// rails, and the rails don't cross each other: so the rails are the pair
/// that every other subpath crosses. Where that fits more than one pair
/// (two rails and two rungs make a # either way up) the first two subpaths
/// are the rails if they're such a pair, as Ink/Stitch has it, and
/// otherwise the longest pair; where it fits none (rungs that stop short
/// of a rail), the two longest subpaths.
fn pick_rails(lines: &[(Polyline, bool)]) -> (usize, usize) {
    let n = lines.len();
    let len: Vec<f64> = lines.iter().map(|(l, _)| polyline_length(l)).collect();
    let mut by_len: Vec<usize> = (0..n).collect();
    by_len.sort_by(|&i, &j| len[j].total_cmp(&len[i]));
    let longest = (by_len[0], by_len[1]);
    if n == 2 {
        return longest;
    }
    let boxes: Vec<crate::geom::Bounds> = lines
        .iter()
        .map(|(l, _)| {
            let mut b = crate::geom::Bounds::empty();
            l.iter().for_each(|&p| b.add(p));
            b
        })
        .collect();
    let crosses = |i: usize, j: usize| -> bool {
        let (bi, bj) = (&boxes[i], &boxes[j]);
        if bi.is_empty()
            || bj.is_empty()
            || bi.min.x > bj.max.x
            || bj.min.x > bi.max.x
            || bi.min.y > bj.max.y
            || bj.min.y > bi.max.y
        {
            return false;
        }
        let segs = |k: usize| {
            let (l, closed) = &lines[k];
            let wrap = (*closed && l.len() > 2).then(|| (l[l.len() - 1], l[0]));
            l.windows(2).map(|w| (w[0], w[1])).chain(wrap)
        };
        // (Two open lines that meet at an end of each, a leaf's rails at
        // its tip, touch there and don't cross: only rungs go across.)
        let ends = |k: usize| {
            let (l, closed) = &lines[k];
            (!*closed && l.len() >= 2).then(|| [l[0], l[l.len() - 1]])
        };
        let at_ends = |x: P| {
            let near = |k: usize| ends(k).is_some_and(|e| e.iter().any(|&q| q.dist(x) < LOOP_GAP));
            near(i) && near(j)
        };
        segs(i).any(|(p, p2)| {
            segs(j).any(|(q, q2)| {
                seg_intersect(p, p2, q, q2).is_some_and(|t| !at_ends(p + (p2 - p) * t))
            })
        })
    };
    let mut cross = vec![false; n * n];
    for i in 0..n {
        for j in i + 1..n {
            let c = crosses(i, j);
            cross[i * n + j] = c;
            cross[j * n + i] = c;
        }
    }
    let rails = |i: usize, j: usize| {
        !cross[i * n + j]
            && (0..n).all(|k| k == i || k == j || (cross[k * n + i] && cross[k * n + j]))
    };
    let mut fits: Vec<(usize, usize)> = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            if rails(i, j) {
                fits.push((i, j));
            }
        }
    }
    // (The longer rail first, as the two longest are.)
    let longer_first = |(i, j): (usize, usize)| if len[j] > len[i] { (j, i) } else { (i, j) };
    match fits.len() {
        0 => longest,
        1 => longer_first(fits[0]),
        _ if fits.contains(&(0, 1)) => longer_first((0, 1)),
        _ => fits
            .into_iter()
            .max_by(|x, y| (len[x.0] + len[x.1]).total_cmp(&(len[y.0] + len[y.1])))
            .map(longer_first)
            .unwrap_or(longest),
    }
}

/// Where `rung` crosses `rail`: (arc length along the rail, the point).
fn hits(rail: &[P], rung: &[P]) -> Vec<(f64, P)> {
    let mut out = Vec::new();
    let mut s = 0.0;
    for w in rail.windows(2) {
        let d = w[0].dist(w[1]);
        for r in rung.windows(2) {
            if let Some(t) = seg_intersect(w[0], w[1], r[0], r[1]) {
                out.push((s + t * d, w[0].lerp(w[1], t)));
            }
        }
        s += d;
    }
    out
}

/// The stitches a rung marks, as arc lengths on rails `a` and `b`: where it
/// crosses one and, nearest that, the other. (A rung right across a ring
/// crosses each rail twice and marks two stitches, one each side.)
fn rung_syncs(a: &[P], b: &[P], rung: &[P]) -> Vec<(f64, f64)> {
    let (mut on_a, mut on_b) = (hits(a, rung), hits(b, rung));
    let mut out = Vec::new();
    while !on_a.is_empty() && !on_b.is_empty() {
        let (mut best, mut best_d) = ((0, 0), f64::INFINITY);
        for (i, x) in on_a.iter().enumerate() {
            for (j, y) in on_b.iter().enumerate() {
                let d = x.1.dist(y.1);
                if d < best_d {
                    (best, best_d) = ((i, j), d);
                }
            }
        }
        out.push((on_a.swap_remove(best.0).0, on_b.swap_remove(best.1).0));
    }
    out
}

/// The arc length along `line` of its point nearest `p`.
fn nearest_arc(line: &[P], p: P) -> f64 {
    let (mut best, mut best_d, mut s) = (0.0, f64::INFINITY, 0.0);
    for w in line.windows(2) {
        let d = w[0].dist(w[1]);
        let t = if d > 0.0 {
            ((p - w[0]).dot(w[1] - w[0]) / (d * d)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let dist = w[0].lerp(w[1], t).dist(p);
        if dist < best_d {
            (best, best_d) = (s + t * d, dist);
        }
        s += d;
    }
    best
}

/// A closed loop (its first point again at the end) begun instead at arc
/// length `s` along it.
fn rotate_loop(ring: &[P], s: f64) -> Polyline {
    let n = ring.len();
    if n < 3 {
        return ring.to_vec();
    }
    let arc = Arc::new(ring);
    let s = s.rem_euclid(arc.len().max(1e-9));
    let start = arc.at(s);
    // The first vertex past `s`.
    let i = arc.cum.partition_point(|&c| c <= s).clamp(1, n - 1);
    let mut out = Vec::with_capacity(n + 1);
    out.push(start);
    // On round to the end (which is the first point), then from the second
    // point up to where it began.
    out.extend(ring[i..n].iter().copied());
    out.extend(ring[1..i].iter().copied());
    out.push(start);
    out.dedup_by(|q, p| q.dist(*p) < 1e-9);
    if out.len() > 1 && out[0].dist(*out.last().unwrap()) > 1e-9 {
        out.push(start);
    }
    out
}

fn seg_intersect(p: P, p2: P, q: P, q2: P) -> Option<f64> {
    let (r, s) = (p2 - p, q2 - q);
    let d = cross(r, s);
    if d.abs() < 1e-12 {
        return None;
    }
    let t = cross(q - p, s) / d;
    let u = cross(q - p, r) / d;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(t)
}

fn signed_area(r: &[P]) -> f64 {
    let n = r.len();
    (0..n).map(|i| cross(r[i], r[(i + 1) % n])).sum::<f64>() * 0.5
}

/// Make closed rail `b` (its first point again at the end) run the same
/// way round as `a`.
fn turn_alike(a: &[P], b: &mut Polyline) {
    let open = |r: &[P]| -> Polyline {
        let mut v = r.to_vec();
        if v.len() > 1 && v[0].dist(*v.last().unwrap()) < 1e-9 {
            v.pop();
        }
        v
    };
    if signed_area(&open(a)).signum() != signed_area(&open(b)).signum() {
        b.reverse();
    }
}

/// Make closed rail `b` run the same way round as `a`, starting nearest `a[0]`.
fn align_loop(a: &[P], b: &mut Polyline) {
    let open = |r: &[P]| -> Polyline {
        let mut v = r.to_vec();
        if v.len() > 1 && v[0].dist(*v.last().unwrap()) < 1e-9 {
            v.pop();
        }
        v
    };
    let a0 = open(a);
    let mut b0 = open(b);
    if signed_area(&a0).signum() != signed_area(&b0).signum() {
        b0.reverse();
    }
    let start = (0..b0.len())
        .min_by(|&i, &j| b0[i].dist(a0[0]).total_cmp(&b0[j].dist(a0[0])))
        .unwrap_or(0);
    b0.rotate_left(start);
    b0.push(b0[0]);
    *b = b0;
}

/// Why a filled shape couldn't become a satin column.
#[derive(Clone, Debug, PartialEq)]
pub enum AutoSatinError {
    TooManyHoles,
    TooComplex,
    TooWide(f64),
}

impl std::fmt::Display for AutoSatinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AutoSatinError::TooManyHoles => write!(
                f,
                "has more than one hole, so it can't be a single satin column"
            ),
            AutoSatinError::TooComplex => {
                write!(
                    f,
                    "branches or bends too much for an automatic satin; split it into simple strokes, or use Fill"
                )
            }
            AutoSatinError::TooWide(w) => write!(
                f,
                "is {:.1} mm wide in places; satin over ~10 mm snags. Use Fill",
                w / 10.0
            ),
        }
    }
}

/// Widest stitch allowed in an automatic satin, units.
pub(crate) const AUTO_MAX_WIDTH: f64 = 80.0;

/// Group rings into components: each outer ring with the rings directly inside it.
pub fn components(rings: &[Polyline], rule: FillRule) -> Vec<Vec<Polyline>> {
    let n = rings.len();
    // A point outside a ring's box is outside the ring: most pairs are
    // settled without walking the ring (lettering has hundreds).
    let boxes: Vec<crate::geom::Bounds> = rings
        .iter()
        .map(|r| {
            let mut b = crate::geom::Bounds::empty();
            r.iter().for_each(|&p| b.add(p));
            b
        })
        .collect();
    let in_box = |j: usize, p: P| {
        let b = &boxes[j];
        p.x >= b.min.x && p.x <= b.max.x && p.y >= b.min.y && p.y <= b.max.y
    };
    let inside = |j: usize, p: P| in_box(j, p) && point_inside(&rings[j..=j], FillRule::NonZero, p);
    let depth: Vec<usize> = (0..n)
        .map(|i| {
            (0..n)
                .filter(|&j| j != i && rings[j].len() >= 3 && inside(j, rings[i][0]))
                .count()
        })
        .collect();
    let mut comps: Vec<Vec<Polyline>> = Vec::new();
    for i in 0..n {
        if depth[i].is_multiple_of(2) {
            let mut comp = vec![rings[i].clone()];
            for j in 0..n {
                if depth[j] == depth[i] + 1 && inside(i, rings[j][0]) {
                    comp.push(rings[j].clone());
                }
            }
            comps.push(comp);
        }
    }
    let _ = rule;
    comps
}

/// Rails for one filled component (outer ring + holes).
pub fn auto_rails(comp: &[Polyline]) -> Result<Rails, AutoSatinError> {
    let rails = match comp.len() {
        1 => tips_of(&comp[0], true).ok_or(AutoSatinError::TooComplex)?,
        2 => {
            let mut a = comp[0].clone();
            a.push(a[0]);
            let mut b = comp[1].clone();
            align_loop(&a, &mut b);
            Rails {
                a,
                b,
                closed: true,
                sync: Vec::new(),
            }
        }
        _ => return Err(AutoSatinError::TooManyHoles),
    };
    // Validate: stitches must stay inside the shape and not get too wide.
    let ps = pairs(&rails, 10.0);
    let mut bad = 0;
    let mut widest: f64 = 0.0;
    for &(a, b) in &ps {
        let w = a.dist(b);
        widest = widest.max(w);
        if w > 1.0 && !point_inside(comp, FillRule::EvenOdd, a.lerp(b, 0.5)) {
            bad += 1;
        }
    }
    if widest > AUTO_MAX_WIDTH {
        return Err(AutoSatinError::TooWide(widest));
    }
    if bad as f64 > ps.len() as f64 * 0.08 {
        return Err(AutoSatinError::TooComplex);
    }
    Ok(rails)
}

/// Resample a closed ring to roughly even spacing.
fn resample_ring(ring: &[P], step: f64) -> Polyline {
    let mut closed = ring.to_vec();
    closed.push(ring[0]);
    let mut out = crate::geom::resample_keep_corners(&closed, step);
    out.pop();
    out
}

/// Split a single ring at its two tips into rails running tip to tip.
pub(crate) fn split_at_tips(ring: &[P]) -> Option<Rails> {
    tips_of(ring, false)
}

/// [`split_at_tips`], of a ring that's a whole part of a shape
/// (`whole`), not a piece cut off one.
fn tips_of(ring: &[P], whole: bool) -> Option<Rails> {
    if ring.len() < 3 {
        return None;
    }
    let total = polyline_length(&[ring, &ring[..1]].concat());
    let r = resample_ring(ring, (total / 240.0).max(2.0));
    let n = r.len();
    if n < 6 {
        return None;
    }
    let mut cum = vec![0.0; n + 1];
    for i in 0..n {
        cum[i + 1] = cum[i] + r[i].dist(r[(i + 1) % n]);
    }
    let per = cum[n];
    let rails_for = |i: usize, j: usize| -> Rails {
        let a: Polyline = (i..=j).map(|k| r[k]).collect();
        let b: Polyline = (0..=(n - j + i)).map(|k| r[(i + n - k) % n]).collect();
        Rails {
            a,
            b,
            closed: false,
            sync: Vec::new(),
        }
    };
    // A convex part (a dot, a plain bar) has no strokes to end: its
    // column runs the length of the narrowest strip it lies in, and the
    // stitches across it the short way (a square dot side to side, not
    // corner to corner). Not a piece cut off a letter, which ends at its
    // cuts.
    if whole && let Some((i, j)) = narrowest_way(ring, &r) {
        return Some(rails_for(i.min(j), i.max(j)));
    }
    // Candidate tips: far apart, splitting the outline into similar halves.
    let stride = (n / 120).max(1);
    let mut cands: Vec<(f64, usize, usize)> = Vec::new();
    for i in (0..n).step_by(stride) {
        for j in (i + 1..n).step_by(stride) {
            let arc = cum[j] - cum[i];
            let bal = arc.min(per - arc) / arc.max(per - arc);
            cands.push((r[i].dist(r[j]) * bal, i, j));
        }
    }
    let best = cands.iter().map(|c| c.0).fold(0.0, f64::max);
    if best <= 0.0 {
        return None;
    }
    // Among near-best candidates, prefer the one giving the narrowest stitches
    // (e.g. a bar splits at its short ends, not its corners).
    let mean_width = |rails: &Rails| -> f64 {
        let step = polyline_length(&rails.a).max(polyline_length(&rails.b)) / 24.0;
        let ps = pairs(rails, step.max(1.0));
        ps.iter().map(|(a, b)| a.dist(*b)).sum::<f64>() / ps.len().max(1) as f64
    };
    // Each candidate's width worked out once (not twice per comparison).
    cands
        .iter()
        .filter(|c| c.0 >= best * 0.85)
        .map(|&(_, i, j)| {
            let rails = rails_for(i, j);
            (mean_width(&rails), rails)
        })
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .map(|(_, rails)| rails)
}

/// Of a convex outline (`ring`; `None` if it isn't one), the points of
/// its resampling `r` at the ends of the narrowest strip it lies in: the
/// strip along one of its edges (the narrowest always is: rotating
/// calipers find it in one pass; a dot's few edges are each tried here),
/// the ends where the strip's middle line leaves the outline.
fn narrowest_way(ring: &[P], r: &[P]) -> Option<(usize, usize)> {
    let n = ring.len();
    let edge = |i: usize| ring[(i + 1) % n] - ring[i];
    // Convex: every corner turns the same way (or not at all).
    let turns: Vec<f64> = (0..n).map(|i| cross(edge(i), edge((i + 1) % n))).collect();
    let most = turns.iter().fold(0.0f64, |m, t| m.max(t.abs()));
    if most <= 0.0
        || !(turns.iter().all(|&t| t >= -1e-9 * most) || turns.iter().all(|&t| t <= 1e-9 * most))
    {
        return None;
    }
    // The edge the outline is narrowest across: its direction, and how
    // far across.
    let (along, across) = (0..n)
        .filter(|&i| edge(i).len() > 1e-9)
        .map(|i| {
            let u = edge(i).norm();
            let wide = ring
                .iter()
                .map(|&p| cross(u, p - ring[i]).abs())
                .fold(0.0, f64::max);
            (u, wide)
        })
        .min_by(|x, y| x.1.total_cmp(&y.1))?;
    // The strip's middle line, where it leaves the outline each way, and
    // the samples nearest there.
    let normal = along.perp();
    let lo = ring
        .iter()
        .map(|&p| p.dot(normal))
        .fold(f64::INFINITY, f64::min);
    let middle = lo + across * 0.5;
    let crossings: Vec<f64> = (0..n)
        .filter_map(|i| {
            let (p, q) = (ring[i], ring[(i + 1) % n]);
            let (dp, dq) = (p.dot(normal) - middle, q.dot(normal) - middle);
            (dp * dq <= 0.0 && (dp - dq).abs() > 1e-12)
                .then(|| p.lerp(q, dp / (dp - dq)).dot(along))
        })
        .collect();
    let at = |t: f64| -> Option<usize> {
        let target = along * t + normal * middle;
        (0..r.len()).min_by(|&x, &y| r[x].dist(target).total_cmp(&r[y].dist(target)))
    };
    let end = |sign: f64| {
        let t = crossings
            .iter()
            .copied()
            .max_by(|x, y| (sign * x).total_cmp(&(sign * y)))?;
        at(t)
    };
    let (i, j) = (end(1.0)?, end(-1.0)?);
    (i != j).then_some((i, j))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(underlay: u8) -> SatinParams {
        SatinParams {
            spacing: 4.0,
            pull: 0.0,
            underlay,
            run_len: 25.0,
            short_stitches: true,
            split: None,
            style: Default::default(),
            lean: 0.0,
            even: false,
        }
    }

    #[test]
    fn a_square_dot_is_sewn_side_to_side_not_corner_to_corner() {
        // The dot of an i, a little taller than wide: its stitches run
        // straight across it, as the stem's do, not at 45° to its sides.
        let dot = vec![
            P::new(0.0, 0.0),
            P::new(18.0, 0.0),
            P::new(18.0, 21.0),
            P::new(0.0, 21.0),
        ];
        let r = auto_rails(&[dot]).expect("a column");
        // (From the middle of one end to the middle of the other: the
        // ends are squared later, see `flat_ends`.)
        let mut tips = [r.a[0], r.a[r.a.len() - 1]];
        tips.sort_by(|p, q| p.y.total_cmp(&q.y));
        assert!(tips[0].dist(P::new(9.0, 0.0)) < 2.0, "{tips:?}");
        assert!(tips[1].dist(P::new(9.0, 21.0)) < 2.0, "{tips:?}");
        // A piece cut off a letter is left to end at its cuts.
        let ring = [
            P::new(0.0, 0.0),
            P::new(18.0, 0.0),
            P::new(18.0, 21.0),
            P::new(0.0, 21.0),
        ];
        assert!(tips_of(&ring, false).is_some());
    }

    #[test]
    fn leaning_stitches_slant_across_the_column_and_fan_at_its_ends() {
        // A straight column 3 mm wide, rail a along y = 15, b along y = −15
        // (or the other way round: the lean is measured from a to b).
        let r = rails_from_centerline(&[P::new(0.0, 0.0), P::new(200.0, 0.0)], false, 30.0);
        let square = pairs(&r, 4.0);
        assert!(square.iter().all(|(a, b)| (a.x - b.x).abs() < 1e-6));
        assert_eq!(pairs_leaning(&r, 4.0, 0.0).len(), square.len());
        for deg in [8.0f64, 30.0, -30.0] {
            let lean = deg.to_radians();
            let ps = pairs_leaning(&r, 4.0, lean);
            // In the body of the column every stitch slants by the angle:
            // its b end 30·tan further along than its a end.
            let want = 30.0 * lean.tan();
            let body: Vec<&(P, P)> = ps
                .iter()
                .filter(|(a, b)| a.x.min(b.x) > 1.0 && a.x.max(b.x) < 199.0)
                .collect();
            assert!(body.len() > 30, "{deg}");
            for (a, b) in &body {
                assert!(((b.x - a.x) - want).abs() < 1e-6, "{deg}: {a:?} {b:?}");
                assert!((a.dist(*b) - 30.0 / lean.cos()).abs() < 1e-6);
            }
            // At each end they fan from the corner: both rails are sewn
            // right to their ends, and nothing goes past them.
            let (first, last) = (ps[0], *ps.last().unwrap());
            assert!(
                first.0.x.abs() < 1e-6 && first.1.x.abs() < 1e-6,
                "{deg}: {first:?}"
            );
            assert!((last.0.x - 200.0).abs() < 1e-6 && (last.1.x - 200.0).abs() < 1e-6);
            assert!(ps.iter().all(|(a, b)| a.x >= -1e-6 && b.x <= 200.0 + 1e-6));
            // Needle points no further apart along either rail than asked.
            assert!(ps.windows(2).all(|w| w[0].0.dist(w[1].0) <= 4.0 + 1e-6
                && w[0].1.dist(w[1].1) <= 4.0 + 1e-6));
        }
        // The column sews with it, and a ring's stitches lean all the way
        // round with no ends to fan at.
        let mut p = params(0);
        p.lean = 20f64.to_radians();
        let col = column(&r, &p);
        assert!(col.windows(2).any(|w| (w[0].x - w[1].x).abs() > 8.0));
        let ring = Rails {
            a: (0..=40)
                .map(|k| {
                    let t = k as f64 / 40.0 * std::f64::consts::TAU;
                    P::new(60.0 * t.cos(), 60.0 * t.sin())
                })
                .collect(),
            b: (0..=40)
                .map(|k| {
                    let t = k as f64 / 40.0 * std::f64::consts::TAU;
                    P::new(40.0 * t.cos(), 40.0 * t.sin())
                })
                .collect(),
            closed: true,
            sync: Vec::new(),
        };
        let ps = pairs_leaning(&ring, 4.0, 20f64.to_radians());
        assert!(ps.iter().all(|(a, b)| a.dist(*b) > 20.5), "all lean");
    }

    #[test]
    fn centerline_satin_zigzags_across_width() {
        let r = rails_from_centerline(&[P::new(0.0, 0.0), P::new(100.0, 0.0)], false, 30.0);
        let s = column(&r, &params(0));
        assert!(s.len() > 40);
        for q in &s {
            assert!((q.y.abs() - 15.0).abs() < 1e-6, "{q:?}");
        }
        assert!(s.windows(2).all(|w| (w[0].y > 0.0) != (w[1].y > 0.0)));
    }

    #[test]
    fn underlay_is_sewn_first_and_path_is_continuous() {
        let r = rails_from_centerline(&[P::new(0.0, 0.0), P::new(200.0, 0.0)], false, 40.0);
        let all = underlay::CENTER | underlay::CONTOUR | underlay::ZIGZAG;
        let s = column(&r, &params(all));
        // Center walk first: points on the centerline.
        assert!(s[..5].iter().all(|q| q.y.abs() < 1e-6));
        // No jumps longer than the zigzag underlay diagonal.
        assert!(s.windows(2).all(|w| w[0].dist(w[1]) < 55.0));
        // Underlay stays inside the rails.
        let top_start = s.len() - pairs(&r, 4.0).len() * 2;
        assert!(s[..top_start].iter().all(|q| q.y.abs() <= 16.0 + 1e-6));
    }

    #[test]
    fn two_rails_with_reversed_second_rail() {
        let a = vec![P::new(0.0, 0.0), P::new(100.0, 0.0)];
        let b = vec![P::new(100.0, 20.0), P::new(0.0, 20.0)];
        let rung = vec![P::new(50.0, -5.0), P::new(50.0, 25.0)];
        let r = rails_from_subpaths(&[(a, false), (b, false), (rung, false)], None).unwrap();
        assert_eq!(r.b[0], P::new(0.0, 20.0));
        assert_eq!(r.sync.len(), 1);
        assert!((r.sync[0].0 - 50.0).abs() < 1e-9 && (r.sync[0].1 - 50.0).abs() < 1e-9);
        for (pa, pb) in pairs(&r, 4.0) {
            assert!((pa.x - pb.x).abs() < 1e-6, "stitches stay perpendicular");
        }
    }

    #[test]
    fn a_stroke_cut_at_a_slant_ends_along_the_cut() {
        // A bar whose right end is cut at 45°, found as a column that
        // comes to a point at the sharp corner: the cut is its end, not
        // the last of its top side.
        let p = |x: f64, y: f64| P::new(x, y);
        let shape = vec![vec![
            p(0.0, 0.0),
            p(100.0, 0.0),
            p(120.0, 20.0),
            p(0.0, 20.0),
        ]];
        let pointed = Rails {
            a: vec![p(0.0, 0.0), p(100.0, 0.0), p(120.0, 20.0)],
            b: vec![p(0.0, 20.0), p(120.0, 20.0)],
            closed: false,
            sync: Vec::new(),
        };
        let flat = flat_ends(&pointed, &shape).expect("the cut is its end");
        assert_eq!(flat.a, vec![p(0.0, 0.0), p(100.0, 0.0)]);
        assert_eq!(flat.b, pointed.b);
        // Its last stitch lies along the cut, and the stitches a way back
        // from it are square across as they were.
        let ps = pairs(&flat, 4.0);
        assert_eq!(*ps.last().unwrap(), (p(100.0, 0.0), p(120.0, 20.0)));
        let square = ps.iter().filter(|(a, b)| (a.x - b.x).abs() < 0.5).count();
        assert!(square * 2 > ps.len(), "{square} of {} square", ps.len());
        // The same whichever end it is, and whichever rail the cut was on.
        let turned = Rails {
            a: pointed.b.iter().rev().copied().collect(),
            b: pointed.a.iter().rev().copied().collect(),
            closed: false,
            sync: Vec::new(),
        };
        let flat = flat_ends(&turned, &shape).expect("the cut is its end");
        assert_eq!(flat.b, vec![p(100.0, 0.0), p(0.0, 0.0)]);
        // A column that's flat-ended already is left alone…
        assert!(flat_ends(&flat, &shape).is_none());
        // …and so is one that comes to a point between two sides alike.
        let arrow = vec![vec![
            p(0.0, 0.0),
            p(100.0, 0.0),
            p(110.0, 10.0),
            p(100.0, 20.0),
            p(0.0, 20.0),
        ]];
        let point = Rails {
            a: vec![p(0.0, 0.0), p(100.0, 0.0), p(110.0, 10.0)],
            b: vec![p(0.0, 20.0), p(100.0, 20.0), p(110.0, 10.0)],
            closed: false,
            sync: Vec::new(),
        };
        assert!(flat_ends(&point, &arrow).is_none());
    }

    #[test]
    fn the_top_of_a_serif_is_not_a_cut() {
        // A stroke that runs down to the right from under a serif, the
        // serif's thin end the column's own: the top of the serif and the
        // stroke together is as long as a slanted cut would be, but under
        // most of it there's only the serif, its bracket curving in. The
        // stitches go on from the serif round into the stroke.
        let p = |x: f64, y: f64| P::new(x, y);
        let rails = Rails {
            a: vec![p(0.0, 0.0), p(60.0, 0.0), p(90.0, 60.0)],
            b: vec![
                p(0.0, 4.0),
                p(25.0, 4.0),
                p(32.0, 10.0),
                p(36.0, 20.0),
                p(66.0, 80.0),
            ],
            closed: false,
            sync: Vec::new(),
        };
        let mut ring = rails.a.clone();
        ring.extend(rails.b.iter().rev());
        assert!(flat_ends(&rails, &[ring]).is_none());
    }

    #[test]
    fn an_end_that_butts_into_another_column_stays_where_it_is() {
        // The same rails, their short end a cut across a bigger shape (a
        // stroke butting into another) and not the shape's own outline.
        let p = |x: f64, y: f64| P::new(x, y);
        let rails = Rails {
            a: vec![p(0.0, 0.0), p(100.0, 0.0), p(120.0, 20.0)],
            b: vec![p(0.0, 20.0), p(112.0, 20.0)],
            closed: false,
            sync: Vec::new(),
        };
        let own = vec![vec![
            p(0.0, 0.0),
            p(100.0, 0.0),
            p(120.0, 20.0),
            p(0.0, 20.0),
        ]];
        assert!(flat_ends(&rails, &own).is_some());
        let bigger = vec![vec![
            p(-50.0, -50.0),
            p(300.0, -50.0),
            p(300.0, 100.0),
            p(-50.0, 100.0),
        ]];
        assert!(flat_ends(&rails, &bigger).is_none());
    }

    #[test]
    fn a_column_is_cut_in_two_across_itself() {
        // A column 100 long and 14 wide, with a rung a third of the way.
        let r = Rails {
            a: vec![P::new(0.0, 0.0), P::new(100.0, 0.0)],
            b: vec![P::new(0.0, 14.0), P::new(100.0, 14.0)],
            closed: false,
            sync: vec![(30.0, 30.0)],
        };
        let (head, tail) = split_at(&r, P::new(60.0, 20.0), 12.0).expect("cut");
        // Square across at the stitch nearest the point, both parts
        // running the way the whole did.
        assert_eq!(head.a.first(), r.a.first());
        assert_eq!(tail.b.last(), r.b.last());
        for (end, start) in [
            (head.a.last(), tail.a.first()),
            (head.b.last(), tail.b.first()),
        ] {
            let (end, start) = (*end.unwrap(), *start.unwrap());
            assert!(
                end.dist(start) < 1e-9 && (end.x - 60.0).abs() < 1e-6,
                "{end:?}"
            );
        }
        // The rung stays with the part it's on.
        assert_eq!(head.sync, vec![(30.0, 30.0)]);
        assert!(tail.sync.is_empty());
        // Not where a part would be a stub, and not a ring.
        assert!(split_at(&r, P::new(5.0, 7.0), 12.0).is_none());
        assert!(split_at(&r, P::new(95.0, 7.0), 12.0).is_none());
        let ring = Rails { closed: true, ..r };
        assert!(split_at(&ring, P::new(60.0, 7.0), 12.0).is_none());
    }

    #[test]
    fn an_even_zigzag_shares_the_step_between_both_stitches() {
        // A straight stem 1.2 mm wide, zigzags 0.4 mm apart.
        let r = rails_from_centerline(&[P::new(0.0, 0.0), P::new(100.0, 0.0)], false, 12.0);
        let mut p = params(0);
        let along =
            |s: &Polyline| -> Vec<f64> { s.windows(2).map(|w| (w[1].x - w[0].x).abs()).collect() };
        // Square: one stitch straight across, the next the whole step.
        let square = column(&r, &p);
        let steps = along(&square);
        assert!(steps.iter().step_by(2).all(|d| *d < 1e-6), "{steps:?}");
        assert!(
            steps
                .iter()
                .skip(1)
                .step_by(2)
                .all(|d| (d - 4.0).abs() < 1e-6)
        );
        // Even: every stitch takes half the step, so each slants the same
        // (one each way), between square ends.
        p.even = true;
        let even = column(&r, &p);
        let steps = along(&even);
        let n = steps.len();
        assert!(
            steps[0] < 1e-6 && steps[n - 1] < 1e-6,
            "square ends: {steps:?}"
        );
        assert!(
            steps[1..n - 1].iter().all(|d| (d - 2.0).abs() < 1e-6),
            "{steps:?}"
        );
        // Still needle points on alternate rails, each rail's a step apart,
        // from one end of the column to the other.
        assert!(even.windows(2).all(|w| (w[0].y > 0.0) != (w[1].y > 0.0)));
        assert!(even.iter().all(|q| (q.y.abs() - 6.0).abs() < 1e-6));
        let on_a: Vec<f64> = even.iter().filter(|q| q.y > 0.0).map(|q| q.x).collect();
        assert!(
            on_a.windows(2)
                .all(|w| (w[1] - w[0] - 4.0).abs() < 1e-6 || w[1] - w[0] < 4.0)
        );
        let xs: Vec<f64> = even.iter().map(|q| q.x).collect();
        assert!(xs.iter().cloned().fold(f64::INFINITY, f64::min).abs() < 1e-6);
        assert!((xs.iter().cloned().fold(0.0, f64::max) - 100.0).abs() < 1e-6);
        // As many stitches as before, give or take the ends.
        assert!(even.len().abs_diff(square.len()) <= 2);
        // A ring has no ends to square off, and closes on itself.
        let ring = Rails {
            a: (0..=80)
                .map(|k| {
                    let t = k as f64 / 80.0 * std::f64::consts::TAU;
                    P::new(60.0 * t.cos(), 60.0 * t.sin())
                })
                .collect(),
            b: (0..=80)
                .map(|k| {
                    let t = k as f64 / 80.0 * std::f64::consts::TAU;
                    P::new(40.0 * t.cos(), 40.0 * t.sin())
                })
                .collect(),
            closed: true,
            sync: Vec::new(),
        };
        let round = column(&ring, &p);
        assert!(
            round
                .windows(2)
                .all(|w| (w[0].len() > 50.0) != (w[1].len() > 50.0))
        );
        assert!(round.windows(2).all(|w| w[0].dist(w[1]) < 21.5));
    }

    fn ellipse(rx: f64, ry: f64, from: f64, clockwise: bool) -> Polyline {
        (0..96)
            .map(|k| {
                let t = k as f64 / 96.0 * std::f64::consts::TAU;
                let t = if clockwise { from - t } else { from + t };
                P::new(rx * t.cos(), ry * t.sin())
            })
            .collect()
    }

    #[test]
    fn rungs_longer_than_the_rails_are_still_rungs() {
        // A serif: a column 3 mm wide and 1 mm long, its edges the two
        // short lines, with a rung along its length sticking out both ends.
        let top = vec![P::new(0.0, 0.0), P::new(10.0, 0.0)];
        let bottom = vec![P::new(0.0, 30.0), P::new(10.0, 30.0)];
        let rung = vec![P::new(5.0, -4.0), P::new(5.0, 34.0)];
        let across = |r: &Rails| {
            let ps = pairs(r, 2.0);
            ps.iter().map(|(a, b)| a.dist(*b)).sum::<f64>() / ps.len() as f64
        };
        // One rung: it's the one that crosses both of the others.
        for order in [[0, 1, 2], [2, 0, 1], [1, 2, 0]] {
            let all = [top.clone(), bottom.clone(), rung.clone()];
            let lines: Vec<(Polyline, bool)> =
                order.iter().map(|&k| (all[k].clone(), false)).collect();
            let r = rails_from_subpaths(&lines, None).unwrap();
            assert!(
                (across(&r) - 30.0).abs() < 1e-6,
                "{order:?}: {}",
                across(&r)
            );
            assert_eq!(r.sync.len(), 1);
        }
        // Two rungs make a # that reads either way: the first two subpaths
        // are the rails, as Ink/Stitch has it.
        let rung2 = vec![P::new(8.0, -4.0), P::new(8.0, 34.0)];
        let rung1 = vec![P::new(2.0, -4.0), P::new(2.0, 34.0)];
        let hash = |ls: [&Polyline; 4]| -> Vec<(Polyline, bool)> {
            ls.iter().map(|l| ((*l).clone(), false)).collect()
        };
        let r = rails_from_subpaths(&hash([&top, &bottom, &rung1, &rung2]), None).unwrap();
        assert!((across(&r) - 30.0).abs() < 1e-6);
        // Written rungs first, it's the longest pair (the rungs): unless
        // the path says which its rails are.
        let wrong = rails_from_subpaths(&hash([&rung1, &rung2, &top, &bottom]), None).unwrap();
        assert!((across(&wrong) - 6.0).abs() < 1e-6);
        let r = rails_from_subpaths(&hash([&rung1, &rung2, &top, &bottom]), Some((2, 3))).unwrap();
        assert!((across(&r) - 30.0).abs() < 1e-6);
        assert_eq!(r.sync.len(), 2);
        // Three rungs and more leave no doubt, in any order.
        let rung3 = vec![P::new(5.0, -4.0), P::new(5.0, 34.0)];
        let lines: Vec<(Polyline, bool)> = [&rung1, &rung2, &rung3, &top, &bottom]
            .iter()
            .map(|l| ((*l).clone(), false))
            .collect();
        let r = rails_from_subpaths(&lines, None).unwrap();
        assert!((across(&r) - 30.0).abs() < 1e-6);
        assert_eq!(r.sync.len(), 3);
        // Marked rails that aren't there are worked out instead.
        let r = rails_from_subpaths(&lines, Some((7, 9))).unwrap();
        assert!((across(&r) - 30.0).abs() < 1e-6);
    }

    #[test]
    fn a_ring_given_as_two_closed_rails_is_sewn_round() {
        // The bowl of an o: an outer and an inner outline, drawn (as a
        // font's are) opposite ways round and from different places, with
        // rungs across the stroke.
        let outer = ellipse(40.0, 50.0, 0.3, false);
        let inner = ellipse(26.0, 38.0, 4.0, true);
        let rung = |t: f64| -> Polyline {
            vec![
                P::new(22.0 * t.cos(), 33.0 * t.sin()),
                P::new(46.0 * t.cos(), 57.0 * t.sin()),
            ]
        };
        let square = |r: &Rails| {
            assert!(r.closed);
            let ps = pairs(r, 4.0);
            assert!(ps.len() > 40);
            // Every stitch crosses the stroke, none the bowl.
            for (a, b) in &ps {
                assert!(a.dist(*b) < 16.5, "{a:?} {b:?} {}", a.dist(*b));
            }
            assert!(
                lean(std::slice::from_ref(r)) < 8.0,
                "{}",
                lean(std::slice::from_ref(r))
            );
        };
        // With no rungs.
        let r = rails_from_subpaths(&[(outer.clone(), true), (inner.clone(), true)], None).unwrap();
        square(&r);
        // With rungs, wherever on the rails they fall: the first is just
        // past the outer rail's start and well before the inner's.
        for count in [1usize, 2, 4, 7] {
            let mut lines = vec![(outer.clone(), true), (inner.clone(), true)];
            for k in 0..count {
                lines.push((
                    rung(0.35 + k as f64 / count as f64 * std::f64::consts::TAU),
                    false,
                ));
            }
            let r = rails_from_subpaths(&lines, None).unwrap();
            square(&r);
            assert_eq!(r.sync.len(), count - 1, "{count} rungs");
            // Each rung is a stitch: the rails' matched points pass
            // through both its crossings.
            let al = Along::new(&r).unwrap();
            for &(sa, sb) in &r.sync {
                let s = al.at[al.syncs.iter().position(|x| *x == (sa, sb)).unwrap()];
                let (pa, pb) = al.pair(s);
                assert!((pa.y.atan2(pa.x * 1.25) - pb.y.atan2(pb.x * 1.25)).abs() < 0.5);
            }
        }
        // Wherever the rails begin and whichever way round they run, with
        // a rung at or beside the outer rail's start (where a digitizer's
        // first stitch is, and where measuring from each rail's own start
        // once paired the whole of one rail with a sliver of the other).
        for from in 0..8 {
            let inner = ellipse(26.0, 38.0, 0.3 + from as f64 * 0.8, from % 2 == 0);
            for off in [-0.2, -0.05, -0.01, 0.0, 0.01, 0.02, 0.05, 0.2] {
                for count in [1usize, 4] {
                    let mut lines = vec![(outer.clone(), true), (inner.clone(), true)];
                    for k in 0..count {
                        let t = 0.3 + off + k as f64 / count as f64 * std::f64::consts::TAU;
                        lines.push((rung(t), false));
                    }
                    let r = rails_from_subpaths(&lines, None).unwrap();
                    square(&r);
                    assert_eq!(r.sync.len(), count - 1, "{from} {off} {count}");
                }
            }
        }
        // A rung right across the bowl marks a stitch each side of it.
        let across = vec![P::new(-50.0, 2.0), P::new(50.0, 2.0)];
        let r = rails_from_subpaths(
            &[
                (outer.clone(), true),
                (inner.clone(), true),
                (across, false),
            ],
            None,
        )
        .unwrap();
        square(&r);
        assert_eq!(r.sync.len(), 1);
        // Loops that end where they began without being closed are loops.
        let shut = |l: &Polyline| {
            let mut l = l.clone();
            l.push(l[0]);
            l
        };
        let r = rails_from_subpaths(
            &[
                (shut(&outer), false),
                (shut(&inner), false),
                (rung(1.0), false),
            ],
            None,
        )
        .unwrap();
        square(&r);
        // And it sews: a zigzag round the stroke, no stitch across the bowl.
        let mut p = params(underlay::CENTER);
        p.even = true;
        let s = column(&r, &p);
        assert!(s.len() > 100);
        assert!(s.windows(2).all(|w| w[0].dist(w[1]) < 30.0));
    }

    #[test]
    fn a_narrow_columns_stitches_are_pulled_out_less() {
        // 3 units of pull a side: all of it on a stitch 4 mm long, but only
        // a tenth of a hairline's 0.5 mm, which would otherwise sew nearly
        // twice as wide.
        let (a, b) = pulled_out(P::new(0.0, 40.0), P::new(0.0, 0.0), 3.0);
        assert!((a.dist(b) - 46.0).abs() < 1e-9);
        let (a, b) = pulled_out(P::new(0.0, 5.0), P::new(0.0, 0.0), 3.0);
        assert!((a.dist(b) - 6.0).abs() < 1e-9);
    }

    #[test]
    fn short_stitches_pull_in_on_tight_inner_curve() {
        // Tight arc: inner rail radius 15, outer 45.
        let line: Polyline = (0..=40)
            .map(|i| {
                let t = i as f64 / 40.0 * std::f64::consts::PI;
                P::new(30.0 * t.cos(), 30.0 * t.sin())
            })
            .collect();
        let r = rails_from_centerline(&line, false, 30.0);
        let mut p = params(0);
        let with = column(&r, &p);
        p.short_stitches = false;
        let without = column(&r, &p);
        let inner_mean = |s: &Polyline| {
            let v: Vec<f64> = s.iter().map(|q| q.len()).filter(|&l| l < 30.0).collect();
            v.iter().sum::<f64>() / v.len() as f64
        };
        assert!(
            inner_mean(&with) > inner_mean(&without) + 1.5,
            "alternate inner stitches inset"
        );
        // The outer rail has room, so it's untouched.
        let outer_max = |s: &Polyline| s.iter().map(|q| q.len()).fold(0.0, f64::max);
        assert!((outer_max(&with) - outer_max(&without)).abs() < 1e-6);
    }

    #[test]
    fn a_column_with_no_rungs_is_sewn_square_across() {
        // A bar 3 mm wide with one end cut on a steep slant, and nothing
        // to say how its stitches cross it: the rails followed in
        // proportion, every stitch leans, the whole way along.
        let bar = Rails {
            a: vec![P::new(0.0, 0.0), P::new(200.0, 0.0)],
            b: vec![P::new(60.0, 30.0), P::new(200.0, 30.0)],
            closed: false,
            sync: Vec::new(),
        };
        let slant = |r: &Rails| {
            pairs(r, 4.0)
                .iter()
                .filter(|(a, _)| a.x > 130.0 && a.x < 195.0)
                .map(|(a, b)| (a.x - b.x).abs())
                .fold(0.0, f64::max)
        };
        assert!(slant(&bar) > 10.0, "{}", slant(&bar));
        // With square rungs they're straight across, from clear of the
        // slanted end right up to the square one; and its ends are where
        // they were.
        let square = squared(&bar);
        assert!(slant(&square) < 0.5, "{}", slant(&square));
        assert!(square.sync.len() > 10);
        let (was, now) = (pairs(&bar, 4.0), pairs(&square, 4.0));
        assert_eq!(was[0], now[0]);
        assert_eq!(was.last(), now.last());
        // None within the bar's width of the slanted end (which sets rail
        // a further ahead of rail b than that: 6 mm): the stitches come
        // round to it over that far. And up to within a stitch or two of
        // the square end.
        assert!(square.sync.iter().all(|&(_, sb)| sb >= 30.0));
        assert!(square.sync.iter().any(|&(_, sb)| sb < 40.0));
        assert!(square.sync.iter().any(|&(sa, _)| sa > 190.0));

        // A column's own rungs are kept, however they lie, and where it
        // has them it gets no others.
        let own = Rails {
            a: vec![P::new(0.0, 0.0), P::new(200.0, 0.0)],
            b: vec![P::new(0.0, 30.0), P::new(200.0, 30.0)],
            closed: false,
            sync: (1..24)
                .map(|k| (k as f64 * 8.0, k as f64 * 8.0 + 3.0))
                .collect(),
        };
        assert_eq!(squared(&own).sync, own.sync);
        // And in a stretch it has none, square ones in step with its own.
        let some = Rails {
            sync: vec![(40.0, 43.0), (160.0, 163.0)],
            ..own.clone()
        };
        let filled = squared(&some).sync;
        assert!(filled.contains(&(40.0, 43.0)) && filled.contains(&(160.0, 163.0)));
        assert!(filled.len() > 15);
        assert!(
            filled
                .windows(2)
                .all(|w| w[0].0 < w[1].0 && w[0].1 < w[1].1)
        );
    }

    #[test]
    fn a_ring_closed_up_on_its_own_first_stitch_ends_flat_across_its_seam() {
        // A square ring 3 mm deep, opened at a seam across its bottom
        // side: each rail runs round to where it began and on along the
        // seam to the middle of it, where they meet in a point.
        let round = |r: f64, seam: f64| -> Polyline {
            vec![
                P::new(0.0, seam),
                P::new(-r, r),
                P::new(-r, -r),
                P::new(r, -r),
                P::new(r, r),
                P::new(0.0, seam),
                P::new(0.0, 85.0),
            ]
        };
        let (mut a, mut b) = (round(70.0, 70.0), round(100.0, 100.0));
        // (The seam in a few steps, as rails come.)
        a.insert(6, P::new(0.0, 78.0));
        b.insert(6, P::new(0.0, 92.0));
        let ring = Rails {
            a,
            b,
            closed: false,
            sync: vec![(100.0, 140.0), (580.0, 830.0)],
        };
        // Sewn as it comes, the last stitches lie along the seam, one on
        // another: every one of them up and down the line x = 0.
        let stacked = |r: &Rails| {
            pairs(r, 4.0)
                .iter()
                .filter(|(p, q)| p.x.abs() < 0.5 && q.x.abs() < 0.5 && p.dist(*q) > 1.0)
                .count()
        };
        assert!(stacked(&ring) > 3, "{}", stacked(&ring));
        // Unfolded, each rail ends where it began: one stitch along the
        // seam at the start and one at the end, and none between.
        let flat = unfolded(&ring);
        assert_eq!(flat.a.last(), Some(&P::new(0.0, 70.0)));
        assert_eq!(flat.b.last(), Some(&P::new(0.0, 100.0)));
        assert_eq!((flat.a.len(), flat.b.len()), (6, 6));
        assert_eq!(stacked(&flat), 2);
        // (And its rungs are where they were: the one past its new end is
        // gone.)
        assert_eq!(flat.sync, [(100.0, 140.0)]);
        // A column that comes to a point any other way is left alone: a
        // wedge, and a stem whose side runs straight on into its point.
        let wedge = Rails {
            a: vec![P::new(0.0, 0.0), P::new(100.0, 0.0), P::new(200.0, 0.0)],
            b: vec![P::new(0.0, 30.0), P::new(100.0, 30.0), P::new(200.0, 0.0)],
            closed: false,
            sync: Vec::new(),
        };
        assert_eq!(unfolded(&wedge).b, wedge.b);
        let stem = Rails {
            a: vec![P::new(30.0, 0.0), P::new(30.0, 50.0), P::new(30.0, 98.0)],
            b: vec![
                P::new(0.0, 0.0),
                P::new(0.0, 100.0),
                P::new(30.0, 100.0),
                P::new(30.0, 98.0),
            ],
            closed: false,
            sync: Vec::new(),
        };
        assert_eq!(unfolded(&stem).a, stem.a);
        assert_eq!(unfolded(&stem).b, stem.b);
    }

    #[test]
    fn rungs_run_right_into_an_end_thats_a_point() {
        // A bar 3 mm wide cut off steeply (72° from square), its far rail
        // run on along the cut to a point: 1 mm of rail a faces 3.2 mm of
        // cut.
        let point = Rails {
            a: vec![P::new(0.0, 0.0), P::new(200.0, 0.0)],
            b: vec![P::new(0.0, 30.0), P::new(190.0, 30.0), P::new(200.0, 0.0)],
            closed: false,
            sync: Vec::new(),
        };
        // It has rungs right into the point. Its stitches are square to
        // it up to the cut; and into the point they cross from rail a to
        // the cut at the same angle to both, half as far from square as
        // the cut is: none lies along the cut.
        let rungs = square_rungs(&point);
        assert!(rungs.iter().any(|&(sa, _)| sa > 190.0), "{rungs:?}");
        let sq = squared(&point);
        let stitches = pairs(&sq, 2.0);
        for (a, b) in stitches.iter().filter(|(a, _)| a.x > 150.0 && a.x < 180.0) {
            assert!((a.x - b.x).abs() < 1.0, "{a:?} {b:?}");
        }
        let into: Vec<&(P, P)> = stitches
            .iter()
            .filter(|(a, b)| a.x > 191.0 && a.dist(*b) > 3.0)
            .collect();
        assert!(into.len() >= 2, "{}", into.len());
        for (a, b) in into {
            assert!((a.x - b.x).abs() <= 0.8 * (a.y - b.y).abs(), "{a:?} {b:?}");
        }
        // The same bar ending in a stitch from one rail's end to the
        // other's (rail b 1 mm short) has none within that millimetre of
        // rail b's end: its stitches come round to the end over it.
        let flat = Rails {
            b: vec![P::new(0.0, 30.0), P::new(190.0, 30.0)],
            ..point.clone()
        };
        let rungs = square_rungs(&flat);
        assert!(rungs.iter().all(|&(_, sb)| sb <= 180.5), "{rungs:?}");
        assert!(rungs.iter().any(|&(_, sb)| sb > 170.0), "{rungs:?}");
    }

    #[test]
    fn stitches_cross_square_to_the_side_that_runs_straight() {
        // A stem 3 mm wide whose right side curves 0.6 mm into it and back
        // over 4 mm (a bowl's counter running into it), the left side
        // straight.
        let stem = Rails {
            a: vec![P::new(0.0, 0.0), P::new(0.0, 200.0)],
            b: std::iter::once(P::new(30.0, 0.0))
                .chain((0..=20).map(|k| {
                    let y = 80.0 + k as f64 * 2.0;
                    let u = (y - 100.0) / 20.0;
                    P::new(30.0 - 6.0 * (1.0 - u * u), y)
                }))
                .chain(std::iter::once(P::new(30.0, 200.0)))
                .collect(),
            closed: false,
            sync: Vec::new(),
        };
        // Its stitches beside the bulge cross it squarer to the straight
        // side than at the same angle to both sides: tilted no more than
        // three quarters as far.
        let tilt = |sync: Vec<(f64, f64)>| {
            let r = Rails {
                sync,
                ..stem.clone()
            };
            pairs(&r, 2.0)
                .into_iter()
                .filter(|(a, _)| a.y > 88.0 && a.y < 112.0)
                .map(|(a, b)| (a.y - b.y).abs())
                .fold(0.0, f64::max)
        };
        let (even, weighted) = (
            tilt(square_rungs_bent(&stem, 0.0)),
            tilt(square_rungs(&stem)),
        );
        assert!(even > 2.5, "{even}");
        assert!(weighted < even * 0.75, "{weighted} against {even}");
    }

    #[test]
    fn a_column_cut_off_on_a_steep_slant_comes_to_a_point() {
        let always = |_: P, _: P| true;
        // A bar 3 mm wide cut off at 45°: rail b stops 3 mm short of
        // rail a.
        let bar = Rails {
            a: vec![P::new(0.0, 0.0), P::new(200.0, 0.0)],
            b: vec![P::new(0.0, 30.0), P::new(170.0, 30.0)],
            closed: false,
            sync: vec![(50.0, 50.0)],
        };
        // The cut is its side: rail b runs on along it to rail a's end,
        // and nothing else moves.
        let point = pointed(&bar, POINT_SHEAR, always);
        assert_eq!(point.a, bar.a);
        assert_eq!(point.b, [bar.b[0], bar.b[1], P::new(200.0, 0.0)]);
        assert_eq!(point.sync, bar.sync);
        // Its stitches are square to it up to the cut, and into the point
        // lean less than half as far as the cut does: none lies along it,
        // as its last would have, with the ones before fanned round.
        let stitches = pairs(&squared(&point), 4.0);
        assert!(
            stitches
                .iter()
                .filter(|(a, _)| a.x > 5.0 && a.x < 165.0)
                .all(|(a, b)| (a.x - b.x).abs() < 0.5)
        );
        let into: Vec<&(P, P)> = stitches
            .iter()
            .filter(|(a, b)| a.x > 172.0 && a.dist(*b) > 3.0)
            .collect();
        assert!(into.len() >= 4, "{}", into.len());
        for (a, b) in into {
            assert!((b.x + b.y - 200.0).abs() < 0.5, "on the cut: {b:?}");
            assert!((a.x - b.x).abs() <= (a.y - b.y).abs() * 0.47, "{a:?} {b:?}");
        }
        // The same at its start, with its rungs where they were: here
        // rail b starts 3 mm on from rail a, and a rung square across at
        // x = 100.
        let bar = Rails {
            a: vec![P::new(0.0, 0.0), P::new(200.0, 0.0)],
            b: vec![P::new(30.0, 30.0), P::new(200.0, 30.0)],
            closed: false,
            sync: vec![(100.0, 70.0)],
        };
        let point = pointed(&bar, POINT_SHEAR, always);
        assert_eq!(point.b, [bar.a[0], bar.b[0], bar.b[1]]);
        let (sa, sb) = point.sync[0];
        let (a, b) = (Arc::new(&point.a), Arc::new(&point.b));
        assert!(a.at(sa).dist(P::new(100.0, 0.0)) < 1e-6);
        assert!(b.at(sb).dist(P::new(100.0, 30.0)) < 1e-6);
        // An end cut square, or 20° off it, is an end still; so is one
        // that isn't a cut at all, but the shape's own outline, however
        // it slants.
        for stop in [200.0, 189.0] {
            let bar = Rails {
                a: vec![P::new(0.0, 0.0), P::new(200.0, 0.0)],
                b: vec![P::new(0.0, 30.0), P::new(stop, 30.0)],
                closed: false,
                sync: Vec::new(),
            };
            assert_eq!(pointed(&bar, POINT_SHEAR, always).b, bar.b);
        }
        let never = |_: P, _: P| false;
        assert_eq!(pointed(&bar, POINT_SHEAR, never).b, bar.b);
    }

    #[test]
    fn the_outside_of_a_bend_is_sewn_as_closely_as_a_straight_run() {
        // The same tight arc (inner radius 15, outer 45), sewn both ways
        // of stepping along it, and a straight column as wide.
        let arc: Polyline = (0..=40)
            .map(|i| {
                let t = i as f64 / 40.0 * std::f64::consts::PI;
                P::new(30.0 * t.cos(), 30.0 * t.sin())
            })
            .collect();
        let bend = rails_from_centerline(&arc, false, 30.0);
        let straight = rails_from_centerline(&[P::new(0.0, 0.0), P::new(200.0, 0.0)], false, 30.0);
        for even in [false, true] {
            let mut p = params(0);
            p.even = even;
            // The needle points on the outer rail, in the order sewn: no
            // two further apart than the spacing. (Spaced along the
            // column's middle they'd be half as far apart again.)
            let outer: Polyline = column(&bend, &p)
                .into_iter()
                .filter(|q| q.len() > 44.0)
                .collect();
            let widest = outer
                .windows(2)
                .map(|w| w[0].dist(w[1]))
                .fold(0.0, f64::max);
            assert!(
                outer.len() > 30 && widest <= p.spacing + 1e-6,
                "{even}: {widest}"
            );
            // A straight column is sewn as it always was: its length over
            // the spacing, there and back.
            let n = column(&straight, &p).len();
            assert!((100..=103).contains(&n), "{even}: {n}");
        }
        // Where a rail's needle points crowd, every other stitch stops
        // short of it: nowhere until they're under half as far apart as
        // the other rail's, and never by more than half the width.
        assert_eq!(short(2.0, 4.0, 2.0), 0.0);
        assert_eq!(short(3.0, 4.0, 2.0), 0.0);
        assert!((short(1.0, 4.0, 2.0) - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(short(0.0, 4.0, 2.0), 0.5);
        assert_eq!(short(0.0, 0.0, 2.0), 0.0);
        // Every other of the rest stops a quarter of the way across at
        // most, and only where the points are four times closer; one in
        // eight runs the whole way.
        assert_eq!(short(1.0, 4.0, 4.0), 0.0);
        assert_eq!(short(0.0, 4.0, 4.0), 0.25);
        assert_eq!(short(0.0, 4.0, 8.0), 0.125);
        assert_eq!(
            (0..9).map(short_every).collect::<Vec<_>>(),
            [
                None,
                Some(2.0),
                Some(4.0),
                Some(2.0),
                Some(8.0),
                Some(2.0),
                Some(4.0),
                Some(2.0),
                None
            ]
        );
        // Stitches fanned from one point (the inside of a sharp corner):
        // an eighth of them reach it.
        let fan = Rails {
            a: vec![P::new(0.0, 0.0), P::new(0.1, 0.0)],
            b: (0..=40)
                .map(|i| {
                    let t = i as f64 / 40.0 * std::f64::consts::FRAC_PI_2;
                    P::new(60.0 * t.cos(), 60.0 * t.sin())
                })
                .collect(),
            closed: false,
            sync: Vec::new(),
        };
        let sewn = column(&fan, &params(0));
        let at_corner = sewn.iter().filter(|q| q.len() < 1.0).count();
        let outside = sewn.iter().filter(|q| q.len() > 59.0).count();
        assert!(
            outside >= 24 && at_corner * 6 <= outside,
            "{at_corner} of {outside}"
        );
    }

    #[test]
    fn auto_satin_splits_a_bar_at_its_short_ends() {
        let bar = vec![
            P::new(0.0, 0.0),
            P::new(200.0, 0.0),
            P::new(200.0, 30.0),
            P::new(0.0, 30.0),
        ];
        let r = auto_rails(&[bar]).unwrap();
        let ps = pairs(&r, 10.0);
        let mid = &ps[ps.len() / 2];
        assert!(
            (mid.0.dist(mid.1) - 30.0).abs() < 3.0,
            "stitches cross the bar's width: {mid:?}"
        );
    }

    #[test]
    fn auto_satin_ring_uses_inner_and_outer_outline() {
        let circle = |r: f64| -> Polyline {
            (0..64)
                .map(|i| {
                    let t = i as f64 / 64.0 * std::f64::consts::TAU;
                    P::new(r * t.cos(), r * t.sin())
                })
                .collect()
        };
        let comps = components(&[circle(60.0), circle(40.0)], FillRule::EvenOdd);
        assert_eq!(comps.len(), 1);
        let r = auto_rails(&comps[0]).unwrap();
        assert!(r.closed);
        for (a, b) in pairs(&r, 10.0) {
            assert!((a.dist(b) - 20.0).abs() < 2.0);
        }
    }

    #[test]
    fn auto_satin_refuses_wide_and_branched_shapes() {
        let square = vec![
            P::new(0.0, 0.0),
            P::new(300.0, 0.0),
            P::new(300.0, 300.0),
            P::new(0.0, 300.0),
        ];
        assert!(auto_rails(&[square]).is_err());
        // A plus sign: four arms can't be one column.
        let plus = vec![
            P::new(40.0, 0.0),
            P::new(60.0, 0.0),
            P::new(60.0, 40.0),
            P::new(100.0, 40.0),
            P::new(100.0, 60.0),
            P::new(60.0, 60.0),
            P::new(60.0, 100.0),
            P::new(40.0, 100.0),
            P::new(40.0, 60.0),
            P::new(0.0, 60.0),
            P::new(0.0, 40.0),
            P::new(40.0, 40.0),
        ];
        assert!(
            auto_rails(&[plus]).is_err(),
            "a 10 mm wide cross can't be one satin"
        );
    }

    #[test]
    fn rails_that_meet_at_a_tip_are_still_the_rails() {
        // A leaf: two rails from one point, bowing apart and meeting again,
        // and two rungs. The rails touch (they don't cross), and are the
        // rails, unless the path lists the rungs first (when four lines
        // that make a # are read as Ink/Stitch reads them).
        let bow = |up: f64| -> Polyline {
            (0..=20)
                .map(|k| {
                    let t = k as f64 / 20.0;
                    P::new(t * 200.0, up * 30.0 * (std::f64::consts::PI * t).sin())
                })
                .collect()
        };
        let (top, bottom) = (bow(-1.0), bow(1.0));
        let rung = |x: f64| -> Polyline { vec![P::new(x, -40.0), P::new(x, 40.0)] };
        for order in [[0usize, 1, 2, 3], [1, 0, 3, 2], [2, 0, 3, 1]] {
            let all = [top.clone(), bottom.clone(), rung(70.0), rung(130.0)];
            let lines: Vec<(Polyline, bool)> =
                order.iter().map(|&k| (all[k].clone(), false)).collect();
            let r = rails_from_subpaths(&lines, None).expect("a column");
            assert!(
                polyline_length(&r.a) > 190.0 && polyline_length(&r.b) > 190.0,
                "{order:?}: the rungs taken for rails"
            );
            assert_eq!(r.sync.len(), 2, "{order:?}");
        }
    }
}
