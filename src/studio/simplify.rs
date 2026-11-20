//! Object › Simplify Design…: fewer threads, fewer small details and fewer
//! trims, with what the design costs to sew before and after beside the
//! controls. It previews on the canvas as the controls move (like Object ›
//! Path › Simplify…); OK keeps it as one undo step, Cancel puts the design
//! back.

use crate::app::{SPM, Studio};
use crate::theme::{pal_ui, space};
use crate::widgets::fmt_int;
use eframe::egui::{self, Key};
use zigzag::design::Design;
use zigzag::document::{Document, SimplifyDesign, SimplifyReport, Unite};

/// What a design costs to sew.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Figures {
    pub threads: usize,
    /// Times the thread is changed (one fewer than its color blocks).
    pub changes: usize,
    pub stitches: usize,
    pub trims: usize,
    pub minutes: usize,
}

impl Figures {
    pub fn of(d: &Design) -> Figures {
        let mut threads: Vec<[u8; 3]> = d.blocks.iter().map(|b| b.color).collect();
        threads.sort_unstable();
        threads.dedup();
        let stitches = d.stitch_count();
        Figures {
            threads: threads.len(),
            changes: d.blocks.len().saturating_sub(1),
            stitches,
            trims: d.trim_count(),
            minutes: (stitches as f64 / SPM).ceil() as usize,
        }
    }
}

/// The Simplify Design window.
#[derive(Clone)]
pub struct DesignDialog {
    pub opt: SimplifyDesign,
    /// The design as it was (Cancel puts it back).
    pub(crate) base: Document,
    /// What the canvas shows now (`None`: nothing applied yet).
    shown: Option<SimplifyDesign>,
    before: Figures,
    /// How many threads the design came with.
    threads: usize,
    report: SimplifyReport,
    /// What was selected, to select again afterwards.
    selected: Vec<usize>,
}

impl DesignDialog {
    /// What it has done so far.
    #[cfg(test)]
    pub fn report(&self) -> &SimplifyReport {
        &self.report
    }
}

impl Studio {
    /// Open Object › Simplify Design….
    pub fn open_design_dialog(&mut self) {
        if self.doc.shapes.is_empty() || self.previewing() {
            return;
        }
        if self.dirty_needs_plan() {
            self.replan();
        }
        let selected = self
            .sel_indices()
            .iter()
            .filter_map(|&i| self.doc.shapes.get(i).map(|s| s.uid))
            .collect();
        self.layout.design_dialog = Some(DesignDialog {
            // Opens having done the one thing that costs the design
            // nothing: each thread together, where the layering allows.
            opt: SimplifyDesign {
                group: true,
                ..SimplifyDesign::default()
            },
            base: self.doc.clone(),
            shown: None,
            before: Figures::of(&self.design),
            threads: self.doc.threads().len(),
            report: SimplifyReport::default(),
            selected,
        });
    }

    /// The Simplify Design window, previewing on the canvas as it goes.
    pub fn design_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.layout.design_dialog.take() else {
            return;
        };
        let (mut ok, mut cancel, mut reset) = (false, false, false);
        let now = Figures::of(&self.design);
        use crate::dialog;
        dialog::show(
            ctx,
            egui::Id::unique("simplify-design"),
            dialog::Place::TopLeft,
            |ui| {
                dialog::heading(ui, "Tasarımı Sadeleştir");
                ui.spacing_mut().slider_width = 150.0;
                crate::theme::form(ui, "simplify-design-form", |fm| {
                    fm.row("İplikler", |ui, lid| {
                        let mut n = d.opt.threads.unwrap_or(d.threads).min(d.threads);
                        let r = ui
                            .add_enabled(
                                d.threads > 1,
                                egui::Slider::new(&mut n, 1..=d.threads.max(1)).show_value(false),
                            )
                            .labelled_by(lid)
                            .on_hover_text(
                                "Bu sayıda kalana kadar en benzer iplikleri birleştir. Her çift, daha çok dikilen ipliğe dönüşür; böylece gerçek iplikler olarak kalırlar.",
                            );
                        r.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::Role::Slider, true, "Tutulacak iplik sayısı")
                        });
                        if r.changed() {
                            d.opt.threads = (n < d.threads).then_some(n);
                        }
                        ui.label(if n < d.threads {
                            format!("{n} / {}", d.threads)
                        } else {
                            format!("tümü ({})", d.threads)
                        });
                    });
                    fm.row("Ayrıntılar", |ui, lid| {
                        let mm = &mut d.opt.min_size_mm;
                        let r = ui
                            .add(
                                egui::Slider::new(mm, 0.0..=5.0)
                                    .show_value(false)
                                    .step_by(0.1)
                                    .clamping(egui::SliderClamping::Never),
                            )
                            .labelled_by(lid)
                            .on_hover_text(
                                "Bundan küçük katmanları çıkar: iyi dikilemeyecek kadar küçük ve her biri bir duruşa mal olan ayrıntılar. Yazılara dokunulmaz.",
                            );
                        r.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::Role::Slider,
                                true,
                                "Tutulacak en küçük ayrıntı",
                            )
                        });
                        ui.add(crate::units::drag_mm(
                            mm,
                            0.0..=SimplifyDesign::MAX_SIZE_MM,
                            0.1,
                        ));
                    });
                    fm.row("Kesimler", |ui, lid| {
                        let least = d.base.settings.connect_mm;
                        let most = SimplifyDesign::MAX_CONNECT_MM.max(least);
                        let mm = &mut d.opt.connect_mm;
                        *mm = mm.max(least);
                        let r = ui
                            .add(
                                egui::Slider::new(mm, least..=most)
                                    .show_value(false)
                                    .step_by(0.1),
                            )
                            .labelled_by(lid)
                            .on_hover_text(
                                "Bu uzunluğa kadar olan boşluklarda ipliği kesmek yerine üzerinden dik (Belge panelindeki Bağla). Bu dikişler kumaşta görünür.",
                            );
                        r.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::Role::Slider,
                                true,
                                "Üzerinden dikilecek en uzun boşluk",
                            )
                        });
                        ui.add(crate::units::drag_mm(mm, least..=most, 0.1));
                    });
                    fm.row("Sıra", |ui, _| {
                        ui.checkbox(&mut d.opt.group, "Her ipliği bir arada dik")
                            .on_hover_text(
                                "Katmanları, her iplik katman düzeninin izin verdiği en az seferde dikilecek şekilde yeniden sırala (Renkleri Grupla)",
                            );
                    });
                    fm.row("Birleştir", |ui, _| {
                        let choice = d.opt.unite;
                        ui.scope(|ui| {
                            crate::theme::segmented(
                                ui,
                                "simplify-design-unite",
                                &mut d.opt.unite,
                                &[
                                    (Unite::Off, "Kapalı"),
                                    (Unite::Similar, "Benzer Katmanlar"),
                                    (Unite::All, "Değenlerin Tümü"),
                                ],
                            );
                        })
                        .response
                        .on_hover_text(match choice {
                            Unite::Off => "Her katman ayrı kalır.",
                            Unite::Similar => {
                                "Aynı iplikte olup birbirine değen ve aynı şekilde dikilen katmanlar (dolguyla dolgu, satenle saten) tek şekil olur. Dolgu üzerindeki saten kalır."
                            }
                            Unite::All => {
                                "Aynı iplikte olup birbirine değen katmanlar, nasıl dikilirse dikilsin tek şekil olur: en az dikiş, ama aynı iplikteki ayrıntılar kaybolur."
                            }
                        });
                    });
                });
                ui.add_space(space::S);
                ui.separator();
                ui.add_space(space::XS);
                figures_table(ui, &d.before, &now);
                ui.add_space(space::S);
                ui.separator();
                dialog::buttons(ui, |ui| {
                    ok = dialog::primary(ui, "Tamam", true).clicked()
                        || ui.input(|i| i.key_pressed(Key::Enter));
                    cancel = dialog::secondary(ui, "İptal").clicked()
                        || ui.input(|i| i.key_pressed(Key::Escape));
                    reset = dialog::apart(ui, |ui| {
                        dialog::secondary(ui, "Sıfırla")
                            .on_hover_text("Tasarımın önceki hâline dön")
                            .clicked()
                    });
                });
            },
        );
        if reset {
            d.opt = SimplifyDesign {
                connect_mm: d.base.settings.connect_mm,
                ..SimplifyDesign::default()
            };
        }
        if d.shown != Some(d.opt) {
            let mut doc = d.base.clone();
            match doc.simplify_design(&d.opt) {
                Ok(report) => {
                    d.report = report;
                    self.doc = doc;
                }
                Err(e) => {
                    self.doc = d.base.clone();
                    d.report = SimplifyReport::default();
                    self.status = format!("Sadeleştirilemedi: {e:#}");
                }
            }
            self.dirty = true;
            d.shown = Some(d.opt);
        }
        if ok {
            let changed = d.report.changed();
            let after = std::mem::replace(&mut self.doc, d.base.clone());
            if changed {
                let _ = self.structural(|doc| {
                    *doc = after;
                    Ok(())
                });
                self.replan();
                self.status = simplified(&d.before, &Figures::of(&self.design), &d.report);
            }
            self.restore_selection(&d.selected);
            self.dirty = true;
        } else if cancel {
            self.doc = d.base;
            self.restore_selection(&d.selected);
            self.dirty = true;
        } else {
            self.layout.design_dialog = Some(d);
        }
    }
}

/// What simplifying came to, in a line.
fn simplified(before: &Figures, after: &Figures, report: &SimplifyReport) -> String {
    let mut parts = Vec::new();
    let mut say = |what: &str, a: usize, b: usize| {
        if a != b {
            parts.push(format!("{what} {} → {}", fmt_int(a), fmt_int(b)));
        }
    };
    say("iplik", before.threads, after.threads);
    say("iplik değişimi", before.changes, after.changes);
    say("kesim", before.trims, after.trims);
    say("dikiş", before.stitches, after.stitches);
    let layers = |n: usize| if n == 1 { "katman" } else { "katman" };
    if report.removed > 0 {
        parts.push(format!(
            "{} küçük {} çıkarıldı",
            report.removed,
            layers(report.removed)
        ));
    }
    if report.united > 0 {
        parts.push(format!(
            "{} {} yanındakiyle birleştirildi",
            report.united,
            layers(report.united)
        ));
    }
    if parts.is_empty() {
        "Sadeleştirildi: dikimi öncekiyle aynı.".into()
    } else {
        format!("Sadeleştirildi: {}.", parts.join(", "))
    }
}

/// Before and after, a row for each thing a design costs to sew.
fn figures_table(ui: &mut egui::Ui, before: &Figures, now: &Figures) {
    let p = pal_ui(ui);
    egui::Grid::new("simplify-design-figures")
        .num_columns(4)
        .spacing([space::M, space::XS])
        .min_col_width(crate::theme::LABEL_W)
        .show(ui, |ui| {
            let head = |ui: &mut egui::Ui, t: &str| {
                ui.label(egui::RichText::new(t).size(11.0).color(p.tertiary));
            };
            head(ui, "");
            head(ui, "Şu an");
            head(ui, "Sadeleştirilmiş");
            ui.end_row();
            let rows: [(&str, usize, usize, &str); 5] = [
                ("İplikler", before.threads, now.threads, ""),
                ("İplik değişimleri", before.changes, now.changes, ""),
                ("Kesimler", before.trims, now.trims, ""),
                ("Dikişler", before.stitches, now.stitches, ""),
                ("Dikiş süresi", before.minutes, now.minutes, " dk"),
            ];
            for (what, a, b, unit) in rows {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(what).color(p.secondary));
                });
                ui.label(format!("{}{unit}", fmt_int(a)));
                let text = egui::RichText::new(format!("{}{unit}", fmt_int(b)));
                // Fewer is the point: lit when it is, plain when it isn't.
                ui.label(if b < a {
                    text.color(p.accent).strong()
                } else {
                    text
                })
                .widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::Role::Label,
                        true,
                        format!("{what}: {} to {}{unit}", fmt_int(a), fmt_int(b)),
                    )
                });
                ui.end_row();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Action;
    use eframe::egui::Vec2;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use std::path::PathBuf;

    /// The badge, with its ring in a blue of its own and a speck added:
    /// four threads, one of them barely different from another.
    fn studio() -> Harness<'static, Studio> {
        let badge = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/badge.svg");
        let mut h = Harness::builder()
            .with_size(Vec2::new(1400.0, 1500.0))
            .build_ui_state(
                |ui, s: &mut Studio| s.frame(ui),
                Studio::with_file(Some(badge), None),
            );
        h.run_steps(4);
        let ring = h.state().names.iter().position(|n| n == "ring").unwrap();
        h.state_mut()
            .structural(|d| {
                d.shapes[ring].color = [50, 76, 96];
                let speck = [
                    zigzag::geom::P::new(-200.0, -200.0),
                    zigzag::geom::P::new(-195.0, -200.0),
                    zigzag::geom::P::new(-195.0, -195.0),
                ];
                d.add_drawing(&speck, true, [0, 150, 0], zigzag::params::Kind::Fill)?;
                Ok(())
            })
            .unwrap();
        h.run_steps(4);
        h
    }

    #[test]
    fn simplify_design_previews_then_keeps_or_puts_back() {
        let mut h = studio();
        let before = Figures::of(&h.state().design);
        assert_eq!(before.threads, 5);
        let shapes = h.state().doc.shapes.len();
        h.state_mut().run_action(Action::SimplifyDesign);
        h.run_steps(3);
        assert!(h.state().previewing());
        assert!(h.query_by_label("Simplify Design").is_some());
        // Down to three threads, without the speck: on the canvas at once.
        {
            let d = h.state_mut().layout.design_dialog.as_mut().unwrap();
            d.opt.threads = Some(3);
            d.opt.min_size_mm = 1.0;
        }
        h.run_steps(4);
        let d = h.state().layout.design_dialog.clone().unwrap();
        assert_eq!(d.report().threads, (5, 3));
        assert_eq!(d.report().removed, 1);
        assert_eq!(h.state().doc.shapes.len(), shapes - 1);
        assert!(h.query_by_label("Threads: 5 to 3").is_some());
        let undo = h.state().undo.len();
        // Cancel: the design as it was.
        h.get_by_label("Cancel").click();
        h.run_steps(4);
        assert!(h.state().layout.design_dialog.is_none());
        assert_eq!(h.state().doc.shapes.len(), shapes);
        assert_eq!(Figures::of(&h.state().design), before);
        assert_eq!(h.state().undo.len(), undo);
        // Again, and OK: kept, as one step to undo.
        h.state_mut().run_action(Action::SimplifyDesign);
        h.run_steps(3);
        {
            let d = h.state_mut().layout.design_dialog.as_mut().unwrap();
            d.opt.threads = Some(3);
            d.opt.min_size_mm = 1.0;
        }
        h.run_steps(4);
        h.get_by_label("OK").click();
        h.run_steps(4);
        assert!(h.state().layout.design_dialog.is_none());
        let after = Figures::of(&h.state().design);
        assert_eq!(after.threads, 3);
        assert!(after.changes < before.changes, "{after:?} {before:?}");
        assert_eq!(h.state().undo.len(), undo + 1);
        let said = h.state().status.clone();
        assert!(said.starts_with("Simplified: threads 5 to 3"), "{said}");
        assert!(said.ends_with("1 small layer taken out."), "{said}");
        h.state_mut().run_action(Action::Undo);
        h.run_steps(4);
        assert_eq!(h.state().doc.shapes.len(), shapes);
        assert_eq!(Figures::of(&h.state().design).threads, 5);
    }
}
