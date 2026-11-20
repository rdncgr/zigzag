//! Stitch previews: an SVG drawing of the plan (thread-colored stitches, dashed
//! jumps, hoop outline) and a PNG rendering of it. Units are millimetres.

use crate::design::Design;
use anyhow::{Context, Result};
use std::fmt::Write;

#[derive(Clone, Debug)]
pub struct PreviewOptions {
    /// Hoop (width, height) in mm to outline, centered on the origin.
    pub hoop_mm: Option<(f64, f64)>,
    /// Fabric color behind the stitches.
    pub background: [u8; 3],
    pub show_jumps: bool,
    /// Drawn thread thickness, mm.
    pub thread_mm: f64,
    /// Marks drawn over the stitches: the notes someone pinned on
    /// the canvas for an agent to read.
    pub marks: Vec<Mark>,
}

/// A mark on a preview: a pin at `at`, or a box from `at` to `to` with
/// the pin at its corner (design units). `number` is the note's, for
/// whoever reads the marks; it isn't drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub number: u32,
    pub at: crate::geom::P,
    pub to: Option<crate::geom::P>,
}

/// The color marks are drawn in (the studio's blue, as on its canvas).
pub const MARK_RGB: [u8; 3] = [0, 122, 255];

impl Default for PreviewOptions {
    fn default() -> Self {
        PreviewOptions {
            hoop_mm: Some((100.0, 100.0)),
            background: [244, 241, 234],
            show_jumps: true,
            thread_mm: 0.32,
            marks: Vec::new(),
        }
    }
}

pub fn write_svg(d: &Design, opt: &PreviewOptions) -> String {
    let b = d.bounds();
    let pad = 30.0;
    let (mut x0, mut y0, mut x1, mut y1) = if b.is_empty() {
        (-pad, -pad, pad, pad)
    } else {
        (b.min.x - pad, b.min.y - pad, b.max.x + pad, b.max.y + pad)
    };
    for m in &opt.marks {
        for p in std::iter::once(m.at).chain(m.to) {
            x0 = x0.min(p.x - pad);
            y0 = y0.min(p.y - pad);
            x1 = x1.max(p.x + pad);
            y1 = y1.max(p.y + pad);
        }
    }
    if let Some((hw, hh)) = opt.hoop_mm {
        x0 = x0.min(-hw * 5.0 - pad);
        y0 = y0.min(-hh * 5.0 - pad);
        x1 = x1.max(hw * 5.0 + pad);
        y1 = y1.max(hh * 5.0 + pad);
    }
    // The marks are sized to the picture; then room for each pin's head,
    // which stands above its point and a little to each side.
    let k = ((x1 - x0).max(y1 - y0) / 1000.0).clamp(0.4, 4.0);
    for m in &opt.marks {
        x0 = x0.min(m.at.x - PIN_HALF * k * 10.0 - pad);
        x1 = x1.max(m.at.x + PIN_HALF * k * 10.0 + pad);
        y0 = y0.min(m.at.y - PIN_TALL * k * 10.0 - pad);
    }
    let (w, h) = ((x1 - x0) / 10.0, (y1 - y0) / 10.0);
    let mut s = String::with_capacity(d.stitch_count() * 14 + 1024);
    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.1}mm" height="{h:.1}mm" viewBox="{:.1} {:.1} {:.1} {:.1}">"#,
        x0 / 10.0,
        y0 / 10.0,
        w,
        h
    );
    let [r, g, bl] = opt.background;
    let _ = write!(
        s,
        r##"<rect x="{:.1}" y="{:.1}" width="{w:.1}" height="{h:.1}" fill="#{r:02x}{g:02x}{bl:02x}"/>"##,
        x0 / 10.0,
        y0 / 10.0
    );
    if let Some((hw, hh)) = opt.hoop_mm {
        let _ = write!(
            s,
            r##"<rect x="{:.1}" y="{:.1}" width="{hw:.1}" height="{hh:.1}" rx="4" fill="none" stroke="#9a9488" stroke-width="0.3" stroke-dasharray="2 1"/>"##,
            -hw / 2.0,
            -hh / 2.0
        );
    }
    let mut last: Option<crate::geom::P> = None;
    for blk in &d.blocks {
        let color = format!(
            "#{:02x}{:02x}{:02x}",
            blk.color[0], blk.color[1], blk.color[2]
        );
        for run in &blk.runs {
            if opt.show_jumps
                && let (Some(a), Some(&b)) = (last, run.pts.first())
            {
                let _ = write!(
                    s,
                    r##"<line x1="{:.2}" y1="{:.2}" x2="{:.2}" y2="{:.2}" stroke="#888" stroke-width="0.08" stroke-dasharray="0.4 0.4"/>"##,
                    a.x / 10.0,
                    a.y / 10.0,
                    b.x / 10.0,
                    b.y / 10.0
                );
            }
            let _ = write!(
                s,
                r#"<polyline fill="none" stroke-linejoin="round" stroke-linecap="round" stroke-width="{:.2}" stroke="{color}" points=""#,
                opt.thread_mm
            );
            for p in &run.pts {
                let _ = write!(s, "{:.2},{:.2} ", p.x / 10.0, p.y / 10.0);
            }
            s.push_str(r#""/>"#);
            last = run.pts.last().copied();
        }
    }
    write_marks(&mut s, &opt.marks, k);
    s.push_str("</svg>\n");
    s
}

/// How far a pin's head stands above its point, and reaches to each side
/// (mm, in a 100 mm picture).
const PIN_TALL: f64 = 6.6;
const PIN_HALF: f64 = 2.4;

/// Draw `marks` (see [`Mark`]) at `k` times their size for a 100 mm picture.
fn write_marks(s: &mut String, marks: &[Mark], k: f64) {
    let [r, g, b] = MARK_RGB;
    let c = format!("#{r:02x}{g:02x}{b:02x}");
    for m in marks {
        let (x, y) = (m.at.x / 10.0, m.at.y / 10.0);
        if let Some(to) = m.to {
            let (x1, y1) = (to.x / 10.0, to.y / 10.0);
            let _ = write!(
                s,
                r#"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="{c}" fill-opacity="0.08" stroke="{c}" stroke-width="{:.2}" stroke-dasharray="{:.2} {:.2}"/>"#,
                x.min(x1),
                y.min(y1),
                (x1 - x).abs(),
                (y1 - y).abs(),
                0.35 * k,
                1.2 * k,
                0.8 * k
            );
        }
        // A pin: its point on the spot, its head above. (No number in
        // it: which note is which is told by where each one is.)
        let (hr, hy) = (PIN_HALF * k, y - (PIN_TALL - PIN_HALF) * k);
        let _ = write!(
            s,
            r#"<path d="M{x:.2},{y:.2} L{:.2},{:.2} A{hr:.2},{hr:.2} 0 1 1 {:.2},{:.2} Z" fill="{c}" stroke="white" stroke-width="{:.2}"/>"#,
            x - hr * 0.7,
            hy + hr * 0.7,
            x + hr * 0.7,
            hy + hr * 0.7,
            0.3 * k
        );
    }
}

/// Render the preview to a PNG `width_px` wide.
pub fn render_png(d: &Design, opt: &PreviewOptions, width_px: u32) -> Result<Vec<u8>> {
    let svg = write_svg(d, opt);
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).context("preview SVG")?;
    let size = tree.size();
    let scale = width_px.clamp(64, 4096) as f32 / size.width();
    let (w, h) = (
        (size.width() * scale).ceil() as u32,
        (size.height() * scale).ceil() as u32,
    );
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).context("allocating preview")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().context("encoding PNG")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::P;

    #[test]
    fn a_pins_head_is_inside_the_picture_however_big_it_is_drawn() {
        // A big hoop: the marks are drawn big, and one at the very top
        // needs that much room above it.
        for hoop in [(100.0, 100.0), (360.0, 200.0)] {
            let opt = PreviewOptions {
                hoop_mm: Some(hoop),
                marks: vec![Mark {
                    number: 1,
                    at: P::new(0.0, -hoop.1 * 5.0),
                    to: None,
                }],
                ..Default::default()
            };
            let svg = write_svg(&Design::default(), &opt);
            let view: Vec<f64> = svg
                .split("viewBox=\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
                .unwrap()
                .split(' ')
                .map(|v| v.parse().unwrap())
                .collect();
            let k = (view[2].max(view[3]) / 100.0).min(4.0);
            let top = -hoop.1 / 2.0 - PIN_TALL * k.min(4.0);
            // (The picture grew for the head after its size was settled,
            // so the head is no bigger than this.)
            assert!(view[1] <= top + 0.5 * k, "{hoop:?}: {view:?} {top}");
            assert!(!svg.contains("<text"));
        }
    }
}
