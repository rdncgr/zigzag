//! Small custom widgets and formatting helpers.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use zigzag::params::{Kind, Params};
use zigzag::svg::{Geometry, Shape};

pub fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Fill => "Dolgu",
        Kind::Satin => "Saten",
        Kind::Run => "Düz Dikiş",
        Kind::Skip => "Kapalı",
        Kind::Contour => "Kontur",
        Kind::Blanket => "Battaniye",
        Kind::Applique => "Aplike",
        Kind::Manual => "Elle",
        Kind::Meander => "Kıvrımlı",
        Kind::Ripple => "Dalga",
        Kind::Cross => "Kanaviçe",
        Kind::Tartan => "Tartan",
    }
}

pub fn kind_help(k: Kind, on_filled_shape: bool) -> &'static str {
    match (k, on_filled_shape) {
        (Kind::Fill, _) => {
            "Tatami dolgu: bir alanı kaplayan dikiş sıraları. ~7 mm'den geniş şekiller için en iyisi. (F)"
        }
        (Kind::Satin, true) => {
            "Şekil boyunca saten sütun (dar şekiller, harfler, halkalar) ya da dış hattı boyunca saten kenarlık. (S)"
        }
        (Kind::Satin, false) => {
            "Saten sütun: 1–7 mm genişliğinde düzgün, parlak zikzak. Yazı ve kenarlıklar için ideal. (S)"
        }
        (Kind::Run, true) => "Dış hat boyunca düz dikiş: ince bir çizgi. (R)",
        (Kind::Run, false) => "Düz dikiş: ayrıntılar ve ince dış hatlar için ince bir çizgi. (R)",
        (Kind::Skip, _) => "Bu nesneyi dikme. (X)",
        (Kind::Contour, _) => {
            "Kontur dolgu: dalgalar gibi dış hattı içe doğru izleyen sıralar ya da merkezden dışa spiral. Organik şekiller için iyi. (C)"
        }
        (Kind::Blanket, true) => {
            "Battaniye (E-dikiş) kenarı: şeklin içine dişleri olan bir çizgi. Klasik aplike ve arma kenarı. (B)"
        }
        (Kind::Blanket, false) => "Battaniye (E-dikiş): bir yana dişleri olan bir çizgi. (B)",
        (Kind::Applique, _) => {
            "Aplike: yerleşim çizgisi diker, kumaşı koymanız için durur, sabitler, kesmeniz için durur, sonra kenarı satenle kaplar."
        }
        (Kind::Manual, _) => {
            "Elle: yolun her noktası, tam çizildiği gibi bir iğne noktasıdır. İçe aktarılan makine dosyaları ve elle dijitalleştirme için. (M)"
        }
        (Kind::Meander, _) => {
            "Kıvrımlı: kapitone tarzı, şeklin içinde gezinen tek bir çizgi. Hafif ve yumuşak; ince kumaşta ve geniş alanlarda iyi."
        }
        (Kind::Ripple, _) => {
            "Dalga: sudaki dalgalar gibi dış hattı izleyen açık düz dikiş sıraları ya da merkezden tek bir spiral."
        }
        (Kind::Cross, _) => "Kanaviçe: el kanaviçesi gibi X'lerden oluşan bir ızgara. Düz ve bloklu.",
        (Kind::Tartan, _) => {
            "Tartan: birkaç iplikle dokuma şeritler. Şeritleri aşağıda ayarlayın; her iplik kendi rengiyle dikilir."
        }
    }
}

/// Every stitch type, in the order the Stitch panel shows them.
pub const ALL_KINDS: [Kind; 12] = [
    Kind::Fill,
    Kind::Satin,
    Kind::Run,
    Kind::Contour,
    Kind::Blanket,
    Kind::Applique,
    Kind::Meander,
    Kind::Ripple,
    Kind::Cross,
    Kind::Tartan,
    Kind::Manual,
    Kind::Skip,
];

/// Why a stitch type an object can't take is dimmed in the Stitch panel
/// (only a line can't take one: the types that cover an area).
pub fn kind_unavailable(k: Kind) -> String {
    format!(
        "{}\n{} bir alanı kaplar, bu yüzden dolgulu bir şekil gerekir. Bu nesne bir çizgi.",
        kind_blurb(k),
        kind_name(k)
    )
}

pub fn allowed_kinds(s: &Shape) -> Vec<Kind> {
    match s.geometry {
        Geometry::Fill { .. } => ALL_KINDS.to_vec(),
        Geometry::Stroke { .. } => vec![
            Kind::Satin,
            Kind::Run,
            Kind::Blanket,
            Kind::Manual,
            Kind::Skip,
        ],
    }
}

pub fn normalize_angle(mut deg: f64) -> f64 {
    // Rows have no direction: 30° and 210° are the same fill.
    while deg > 90.0 {
        deg -= 180.0;
    }
    while deg <= -90.0 {
        deg += 180.0;
    }
    deg
}

pub fn fmt_int(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn underlay_box(ui: &mut egui::Ui, p: &mut Params, label: &str) {
    let mut on = p.underlay_on();
    if ui.checkbox(&mut on, label).changed() {
        p.underlay = Some(on);
    }
}

/// Stitch-type tile: a painted preview of the stitch over its name.
/// A stitch type as a compact icon button (the Stitch panel's row): the
/// same look whether or not anything is selected. Its tooltip names it.
pub fn kind_button(ui: &mut egui::Ui, k: Kind, selected: bool, enabled: bool) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(30.0), sense);
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, enabled, selected, kind_name(k))
    });
    let p = crate::theme::pal_ui(ui);
    let painter = ui.painter();
    if selected {
        painter.rect_filled(
            rect,
            6.0,
            p.accent.gamma_multiply(if p.dark { 0.28 } else { 0.14 }),
        );
        painter.rect_stroke(
            rect,
            6.0,
            Stroke::new(1.2, p.accent),
            egui::StrokeKind::Inside,
        );
    } else if resp.hovered() {
        painter.rect_filled(rect, 6.0, p.hover);
    }
    let fg = if selected {
        p.accent
    } else if resp.hovered() && enabled {
        p.text
    } else {
        p.secondary
    };
    draw_kind_icon(painter, rect.shrink(6.0), k, fg);
    resp
}

/// The larger stitch-type tile for a selected object: the same icon, with
/// its name under it. `w` × `h` points. Dimmed, and not to be clicked,
/// when the object can't take that type (`enabled` false).
pub fn kind_tile(
    ui: &mut egui::Ui,
    k: Kind,
    selected: bool,
    enabled: bool,
    w: f32,
    h: f32,
) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), sense);
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, enabled, selected, kind_name(k))
    });
    let p = crate::theme::pal_ui(ui);
    let painter = ui.painter();
    if !enabled {
        painter.rect_stroke(
            rect,
            6.0,
            Stroke::new(1.0, p.separator.gamma_multiply(0.6)),
            egui::StrokeKind::Inside,
        );
        let icon = Rect::from_center_size(
            Pos2::new(rect.center().x, rect.top() + crate::theme::space::S + 9.0),
            Vec2::splat(18.0),
        );
        draw_kind_icon(painter, icon, k, p.tertiary);
        painter.text(
            Pos2::new(rect.center().x, rect.bottom() - crate::theme::space::S),
            egui::Align2::CENTER_BOTTOM,
            kind_name(k),
            egui::FontId::proportional(11.0),
            p.tertiary,
        );
        return resp;
    }
    let (bg, border) = if selected {
        (
            p.accent.gamma_multiply(if p.dark { 0.28 } else { 0.14 }),
            Stroke::new(1.2, p.accent),
        )
    } else if resp.hovered() {
        (p.hover, Stroke::new(1.0, p.control_border))
    } else {
        (Color32::TRANSPARENT, Stroke::new(1.0, p.separator))
    };
    painter.rect_filled(rect, 6.0, bg);
    painter.rect_stroke(rect, 6.0, border, egui::StrokeKind::Inside);
    let fg = if selected {
        p.accent
    } else if resp.hovered() {
        p.text
    } else {
        p.secondary
    };
    // Icon in the upper part, name along the bottom.
    let icon = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.top() + crate::theme::space::S + 9.0),
        Vec2::splat(18.0),
    );
    draw_kind_icon(painter, icon, k, fg);
    painter.text(
        Pos2::new(rect.center().x, rect.bottom() - crate::theme::space::S),
        egui::Align2::CENTER_BOTTOM,
        kind_name(k),
        egui::FontId::proportional(11.0),
        if selected { p.accent } else { p.text },
    );
    resp
}

pub fn draw_kind_icon(p: &egui::Painter, r: Rect, k: Kind, c: Color32) {
    // (Whole pixels wide, like the other icons' lines.)
    let ppp = p.pixels_per_point();
    let s = Stroke::new((1.3 * ppp).round().max(1.0) / ppp, c);
    let at = |x: f32, y: f32| Pos2::new(r.left() + x * r.width(), r.top() + y * r.height());
    match k {
        Kind::Fill => {
            // Staggered tatami rows.
            for row in 0..5 {
                let y = 0.1 + row as f32 * 0.2;
                let off = (row % 3) as f32 * 0.12;
                let mut x = 0.0;
                let mut pts = vec![at(0.0, y)];
                while x < 1.0 {
                    x = (x + 0.36 + if pts.len() == 1 { off - 0.12 } else { 0.0 }).min(1.0);
                    pts.push(at(x, y));
                }
                for w in pts.windows(2) {
                    p.line_segment([w[0] + Vec2::new(1.0, 0.0), w[1] - Vec2::new(1.0, 0.0)], s);
                }
            }
        }
        Kind::Satin => {
            let n = 9;
            let pts: Vec<Pos2> = (0..n)
                .map(|i| {
                    at(
                        i as f32 / (n - 1) as f32,
                        if i % 2 == 0 { 0.15 } else { 0.85 },
                    )
                })
                .collect();
            p.add(egui::Shape::line(pts, s));
        }
        Kind::Run => {
            let pts: Vec<Pos2> = (0..=4)
                .map(|i| at(i as f32 / 4.0, 0.5 + 0.25 * ((i as f32) * 1.3).sin()))
                .collect();
            for w in pts.windows(2) {
                p.line_segment([w[0].lerp(w[1], 0.12), w[1].lerp(w[0], 0.12)], s);
            }
            for q in pts {
                p.circle_filled(q, 1.5, c);
            }
        }
        Kind::Skip => {
            let cen = r.center();
            let rad = r.height().min(r.width()) * 0.4;
            p.circle_stroke(cen, rad, s);
            let d = Vec2::splat(rad * std::f32::consts::FRAC_1_SQRT_2);
            p.line_segment([cen - d, cen + d], s);
        }
        Kind::Contour => {
            for k in 0..3 {
                let inset = k as f32 * r.height().min(r.width()) * 0.16;
                p.rect_stroke(
                    r.shrink(inset + 1.0),
                    8.0 - k as f32 * 2.5,
                    s,
                    egui::StrokeKind::Inside,
                );
            }
        }
        Kind::Blanket => {
            p.line_segment([at(0.0, 0.8), at(1.0, 0.8)], s);
            for i in 0..5 {
                let x = 0.1 + i as f32 * 0.2;
                p.line_segment([at(x, 0.8), at(x, 0.2)], s);
            }
        }
        Kind::Applique => {
            let inner = r.shrink(5.0);
            let dash = Stroke::new(1.0, c.gamma_multiply(0.7));
            p.add(egui::Shape::dashed_line(
                &[
                    inner.left_top(),
                    inner.right_top(),
                    inner.right_bottom(),
                    inner.left_bottom(),
                    inner.left_top(),
                ],
                dash,
                3.0,
                2.0,
            ));
            p.rect_stroke(
                r.shrink(1.0),
                3.0,
                Stroke::new(3.0, c),
                egui::StrokeKind::Inside,
            );
        }
        Kind::Meander => {
            // A wandering line.
            let pts = [
                at(0.05, 0.85),
                at(0.1, 0.3),
                at(0.3, 0.25),
                at(0.35, 0.7),
                at(0.5, 0.75),
                at(0.55, 0.2),
                at(0.75, 0.15),
                at(0.8, 0.6),
                at(0.95, 0.65),
            ];
            let mut smooth = vec![pts[0]];
            for w in pts.windows(2) {
                smooth.push(w[0].lerp(w[1], 0.3));
                smooth.push(w[0].lerp(w[1], 0.7));
            }
            smooth.push(pts[pts.len() - 1]);
            p.add(egui::Shape::line(smooth, s));
        }
        Kind::Ripple => {
            // Three nested rounded shapes, far apart.
            for k in 0..3 {
                let inset = k as f32 * r.height().min(r.width()) * 0.22;
                p.rect_stroke(
                    r.shrink(inset + 1.0),
                    10.0 - k as f32 * 3.0,
                    Stroke::new(1.0, c),
                    egui::StrokeKind::Inside,
                );
            }
        }
        Kind::Cross => {
            for (x, y) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                let d = 0.17;
                p.line_segment([at(x - d, y - d), at(x + d, y + d)], s);
                p.line_segment([at(x - d, y + d), at(x + d, y - d)], s);
            }
        }
        Kind::Tartan => {
            let thin = Stroke::new(1.0, c.gamma_multiply(0.6));
            for v in [0.25, 0.7] {
                p.line_segment(
                    [at(v, 0.05), at(v, 0.95)],
                    Stroke::new(3.0, c.gamma_multiply(0.5)),
                );
                p.line_segment(
                    [at(0.05, v), at(0.95, v)],
                    Stroke::new(3.0, c.gamma_multiply(0.5)),
                );
            }
            for v in [0.45, 0.9] {
                p.line_segment([at(v, 0.05), at(v, 0.95)], thin);
                p.line_segment([at(0.05, v), at(0.95, v)], thin);
            }
        }
        Kind::Manual => {
            let pts = [
                at(0.05, 0.75),
                at(0.3, 0.2),
                at(0.55, 0.65),
                at(0.8, 0.15),
                at(0.95, 0.5),
            ];
            p.add(egui::Shape::line(
                pts.to_vec(),
                Stroke::new(1.0, c.gamma_multiply(0.8)),
            ));
            for q in pts {
                p.circle_filled(q, 2.2, c);
            }
        }
    }
}

/// What each stitch type is for, in a sentence.
pub fn kind_blurb(k: Kind) -> &'static str {
    match k {
        Kind::Fill => "~7 mm'den geniş bir alanı kaplayan dikiş sıraları.",
        Kind::Satin => "1–7 mm genişliğinde parlak sütunlar: kenarlıklar, harfler, halkalar.",
        Kind::Run => "Ayrıntılar için bir dikiş çizgisi; üçlü düz dikiş daha belirgindir.",
        Kind::Contour => "Dış hattı içe doğru izleyen sıralar.",
        Kind::Blanket => "Aplike ve arma kenarları için dişli bir çizgi.",
        Kind::Applique => "Dikiş yerine kumaş; yerleştirmek ve kesmek için duraklamalarla.",
        Kind::Manual => "Her nokta, tam çizildiği gibi bir iğne noktasıdır.",
        Kind::Meander => "Kapitone tarzı gezinen tek çizgi: hafif ve yumuşak.",
        Kind::Ripple => "Dış hattı izleyen açık sıralar ya da tek bir spiral.",
        Kind::Cross => "El kanaviçesi gibi X'lerden bir ızgara.",
        Kind::Tartan => "Birkaç iplikle dokuma şeritler.",
        Kind::Skip => "Dikilmez.",
    }
}

/// The stitch types at a glance: icons with tooltips, or (with `info`) a
/// list with a line about each.
pub fn stitch_type_guide(ui: &mut egui::Ui) {
    let kinds = [
        Kind::Fill,
        Kind::Satin,
        Kind::Run,
        Kind::Contour,
        Kind::Blanket,
        Kind::Applique,
        Kind::Meander,
        Kind::Ripple,
        Kind::Cross,
        Kind::Tartan,
    ];
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(crate::theme::space::XS);
        for k in kinds {
            kind_button(ui, k, false, false).on_hover_ui(|ui| {
                ui.label(egui::RichText::new(kind_name(k)).strong());
                ui.label(kind_blurb(k));
            });
        }
    });
}

/// A small color field: saturation/brightness square, hue strip and a hex
/// field. For the occasional custom color; threads come from palettes.
pub fn compact_color_picker(ui: &mut egui::Ui, c: &mut [u8; 3]) -> bool {
    use egui::ecolor::Hsva;
    use egui::epaint::Mesh;
    let hue_id = ui.make_persistent_id("compact-picker-hue");
    let mut hsva = Hsva::from(Color32::from_rgb(c[0], c[1], c[2]));
    // Grey and black have no hue: keep the last one so the field doesn't jump.
    if (hsva.s < 1e-3 || hsva.v < 1e-3)
        && let Some(h) = ui.data(|d| d.get_temp::<f32>(hue_id))
    {
        hsva.h = h;
    }
    let mut changed = false;
    let w = ui.available_width();
    let rgb = |h: f32, s: f32, v: f32| Color32::from(Hsva::new(h, s, v, 1.0));

    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 72.0), Sense::click_and_drag());
    let mut mesh = Mesh::default();
    const N: usize = 12;
    for yi in 0..=N {
        for xi in 0..=N {
            let (s, v) = (xi as f32 / N as f32, 1.0 - yi as f32 / N as f32);
            let pos = Pos2::new(r.left() + s * r.width(), r.top() + (1.0 - v) * r.height());
            mesh.colored_vertex(pos, rgb(hsva.h, s, v));
        }
    }
    for yi in 0..N {
        for xi in 0..N {
            let i = (yi * (N + 1) + xi) as u32;
            let j = i + (N + 1) as u32;
            mesh.add_triangle(i, i + 1, j);
            mesh.add_triangle(i + 1, j + 1, j);
        }
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    if let Some(pos) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked())
    {
        hsva.s = ((pos.x - r.left()) / r.width()).clamp(0.0, 1.0);
        hsva.v = 1.0 - ((pos.y - r.top()) / r.height()).clamp(0.0, 1.0);
        changed = true;
    }
    let m = Pos2::new(
        r.left() + hsva.s * r.width(),
        r.top() + (1.0 - hsva.v) * r.height(),
    );
    ui.painter()
        .circle_stroke(m, 4.0, Stroke::new(1.5, Color32::WHITE));
    ui.painter()
        .circle_stroke(m, 5.0, Stroke::new(1.0, Color32::from_black_alpha(120)));
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Slider, true, "Doygunluk ve parlaklık")
    });

    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 12.0), Sense::click_and_drag());
    let mut mesh = Mesh::default();
    const H: usize = 36;
    for k in 0..=H {
        let h = k as f32 / H as f32;
        let x = r.left() + h * r.width();
        mesh.colored_vertex(Pos2::new(x, r.top()), rgb(h, 1.0, 1.0));
        mesh.colored_vertex(Pos2::new(x, r.bottom()), rgb(h, 1.0, 1.0));
    }
    for k in 0..H as u32 {
        let i = 2 * k;
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i + 1, i + 3, i + 2);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    if let Some(pos) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked())
    {
        hsva.h = ((pos.x - r.left()) / r.width()).clamp(0.0, 0.999);
        changed = true;
    }
    let x = r.left() + hsva.h * r.width();
    ui.painter().rect_stroke(
        Rect::from_center_size(Pos2::new(x, r.center().y), Vec2::new(5.0, 14.0)),
        2.0,
        Stroke::new(1.5, Color32::WHITE),
        egui::StrokeKind::Middle,
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, "Ton"));

    if changed {
        ui.data_mut(|d| d.insert_temp(hue_id, hsva.h));
        let out = Color32::from(hsva);
        *c = [out.r(), out.g(), out.b()];
    }

    // Hex, for typing an exact color.
    let text_id = ui.make_persistent_id("compact-picker-hex");
    let mut text = ui
        .data(|d| d.get_temp::<String>(text_id))
        .unwrap_or_else(|| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]));
    let r = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = crate::theme::space::S;
        ui.label(
            egui::RichText::new("Hex")
                .size(12.0)
                .color(crate::theme::pal_ui(ui).secondary),
        );
        ui.add(egui::TextEdit::singleline(&mut text).desired_width(80.0))
    });
    let r = r.inner;
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(text_id, text.clone()));
    } else {
        if r.lost_focus()
            && let Some(v) = crate::palettes::parse_hex(&text)
            && v != *c
        {
            *c = v;
            changed = true;
        }
        ui.data_mut(|d| d.remove::<String>(text_id));
    }
    changed
}
