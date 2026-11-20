//! Window chrome: the unified title bar / toolbar, the tool strip, and the
//! status bar with the stitch player.

use crate::app::{SPM, Studio, Tool};
use crate::commands::{Action, TOOLS};
use crate::icons::Icon;
use crate::theme::{caption, icon_button, pal_ui};
use crate::widgets::fmt_int;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};

/// The issues popover's width.
const ISSUES_W: f32 = 340.0;
/// The least height of its list, under its header.
const ISSUES_MIN_H: f32 = 96.0;

/// The toolbar's height. On the Mac the window's close, minimise and zoom
/// buttons are centred in it too (`lights`).
pub const TITLEBAR_H: f32 = 48.0;
/// Room at the toolbar's ends, inside it.
pub const TITLEBAR_PAD: i8 = 10;

impl Studio {
    pub fn titlebar(&mut self, ui: &mut egui::Ui) {
        let p = pal_ui(ui);
        let ctx = ui.ctx().clone();
        // The whole bar, its padding at the ends too.
        let rect = ui
            .max_rect()
            .expand2(Vec2::new(f32::from(TITLEBAR_PAD), 0.0));
        // Empty toolbar space moves and zooms the window, as on macOS.
        let bg = ui.interact(
            rect,
            ui.make_persistent_id("titlebar-bg"),
            Sense::click_and_drag(),
        );
        if cfg!(target_os = "macos") {
            if bg.drag_started_by(egui::PointerButton::Primary) {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if bg.double_clicked() {
                let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
            }
        }
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if cfg!(target_os = "macos") {
                // Past the window's own buttons, which share this strip.
                ui.add_space((crate::lights::room() - ui.cursor().min.x).max(0.0));
            } else {
                self.menu_bar(ui);
                ui.add_space(4.0);
            }
            if icon_button(ui, Icon::LeftPanel, "Katmanları Göster/Gizle", false, 28.0).clicked() {
                self.run_action(Action::ToggleLeftDock);
            }
            // (No document name here: the tabs show it and its edited dot,
            // and the window's own title, for the Window menu and Mission
            // Control, is kept by `update_title`.)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, Icon::RightPanel, "Panelleri Göster/Gizle", false, 28.0).clicked() {
                    self.run_action(Action::ToggleRightDock);
                }
                ui.add_space(6.0);
                // (Only for a machine ZigZag sends to.)
                if self.sends_direct() {
                    self.send_button(ui);
                }
                let share = icon_button(ui, Icon::Share, "Dışa Aktar", false, 28.0);
                egui::Popup::menu(&share).show(|ui| {
                    ui.set_min_width(260.0);
                    caption(ui, "Makineniz için dışa aktarın");
                    // The document's machine, and the file it reads, first.
                    if let Some((name, ext)) = self
                        .machine()
                        .and_then(|p| Some((p.name.clone(), p.format?)))
                    {
                        if ui
                            .add_enabled(
                                !self.design.blocks.is_empty(),
                                egui::Button::new(format!("{name} ({})…", ext.to_uppercase())),
                            )
                            .clicked()
                        {
                            self.run_action(Action::Export(ext));
                        }
                        ui.separator();
                    }
                    for (ext, label) in zigzag::formats::MACHINE_FORMATS {
                        if ui
                            .add_enabled(!self.design.blocks.is_empty(), egui::Button::new(label))
                            .clicked()
                        {
                            self.run_action(Action::Export(ext));
                        }
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            !self.design.blocks.is_empty(),
                            egui::Button::new("Üretim Föyü (HTML)…"),
                        )
                        .clicked()
                    {
                        self.run_action(Action::Export("html"));
                    }
                });
                let view = icon_button(ui, Icon::ViewOptions, "Görünüm Seçenekleri", false, 28.0);
                egui::Popup::from_toggle_button_response(&view)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        ui.set_min_width(200.0);
                        ui.checkbox(&mut self.view_tools.grid, "Izgara");
                        ui.checkbox(&mut self.view_tools.points, "İğne noktaları");
                        ui.checkbox(&mut self.show_jumps, "Atlama dikişleri");
                        ui.horizontal(|ui| {
                            ui.color_edit_button_srgb(&mut self.doc.settings.cloth);
                            ui.label("Kumaş rengi");
                        });
                        ui.separator();
                        if ui.button("Sığacak Şekilde Yakınlaştır").clicked() {
                            self.request_fit();
                        }
                    });
                // (To the left of the rest, when there's a newer ZigZag.)
                ui.add_space(crate::theme::space::XS);
                self.update_button(ui);
            });
        });
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0, p.separator),
        );
    }

    /// The toolbar's Send: the Skitch window (connecting to the Skitch).
    fn send_button(&mut self, ui: &mut egui::Ui) {
        if icon_button(ui, Icon::Send, "Skitch'e Gönder", self.machine.open, 28.0)
            .on_hover_text("Skitch'inizde dikin")
            .clicked()
        {
            self.send_action();
        }
    }

    /// The vertical tool strip, Illustrator-style.
    pub fn tool_strip(&mut self, ui: &mut egui::Ui) {
        let p = pal_ui(ui);
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.add_space(8.0);
            for (t, label, key) in TOOLS {
                // Groups: the Pen's (Pen, Add / Delete Anchor Point, Anchor
                // Point) and the shapes', each in one slot with a flyout.
                match t {
                    Tool::Direct => {
                        self.group_button(ui, crate::flyout::Group::Direct);
                        continue;
                    }
                    Tool::Measure => {
                        self.group_button(ui, crate::flyout::Group::Sample);
                        continue;
                    }
                    Tool::Pen => {
                        self.group_button(ui, crate::flyout::Group::Pen);
                        continue;
                    }
                    Tool::Shape => {
                        self.group_button(ui, crate::flyout::Group::Shape);
                        // Rotate comes after the drawing tools, as in
                        // Illustrator. (Not one of `TOOLS`: R alone is
                        // the Run stitch, so its key is Shift+R.)
                        let r = icon_button(
                            ui,
                            Icon::RotateTool,
                            "Döndür",
                            self.tool == Tool::Rotate,
                            30.0,
                        )
                        .on_hover_text("Döndür (Shift+R)");
                        if r.clicked() {
                            self.set_tool(Tool::Rotate);
                        }
                        // A double-click on the tool: its dialog, as in
                        // VectorCraft and Illustrator.
                        if r.double_clicked() {
                            self.open_rotate_dialog(None);
                        }
                        self.group_button(ui, crate::flyout::Group::Build);
                        continue;
                    }
                    Tool::Brush => {
                        self.group_button(ui, crate::flyout::Group::Brush);
                        continue;
                    }
                    _ => {}
                }
                let icon = match t {
                    Tool::Select => Icon::Select,
                    Tool::Hand => Icon::Hand,
                    Tool::Text => Icon::Text,
                    Tool::Angle => Icon::Angle,
                    _ => continue,
                };
                let tip = if t == Tool::Text {
                    format!("Yazı ({}): metin yerleştirmek için tıklayın, düzenlemek için metne tıklayın, metni üzerine koymak için seçili bir yola tıklayın", key.name())
                } else {
                    format!("{label} ({})", key.name())
                };
                let r = icon_button(ui, icon, label, self.tool == t, 30.0).on_hover_text(tip);
                if r.clicked() {
                    self.set_tool(t);
                }
            }
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(Vec2::new(22.0, 1.0), Sense::hover());
            ui.painter()
                .hline(r.x_range(), r.center().y, Stroke::new(1.0, p.separator));
            ui.add_space(6.0);
            if icon_button(ui, Icon::ZoomFit, "Sığacak Şekilde Yakınlaştır", false, 30.0).clicked() {
                self.request_fit();
            }
            // The selection's thread, like Illustrator's fill swatch: click
            // it for the color picker.
            ui.add_space(10.0);
            use crate::threads::SelThread;
            let thread = self.selection_thread();
            let sense = if thread == SelThread::None {
                Sense::hover()
            } else {
                Sense::click()
            };
            let (r, resp) = ui.allocate_exact_size(Vec2::splat(24.0), sense);
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::Role::Button,
                    thread != SelThread::None,
                    if thread == SelThread::Mixed {
                        "İplik Rengi: birkaç"
                    } else {
                        "İplik Rengi"
                    },
                )
            });
            match thread {
                SelThread::One(c) => {
                    ui.painter().rect_filled(r, 5.0, crate::canvas::color32(c));
                    ui.painter().rect_stroke(
                        r,
                        5.0,
                        Stroke::new(1.0, Color32::from_black_alpha(70)),
                        egui::StrokeKind::Inside,
                    );
                }
                // Layers in different threads: no one color to show.
                SelThread::Mixed => crate::threads::paint_mixed(ui.painter(), r, 5.0, &p),
                SelThread::None => {
                    // No selection: an empty swatch with a slash, like "None".
                    ui.painter().rect_stroke(
                        r,
                        5.0,
                        Stroke::new(1.0, p.tertiary),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().line_segment(
                        [
                            r.left_bottom() + Vec2::new(4.0, -4.0),
                            r.right_top() + Vec2::new(-4.0, 4.0),
                        ],
                        Stroke::new(1.0, p.error),
                    );
                }
            }
            let resp = resp.on_hover_text(match thread {
                SelThread::One(c) => format!(
                    "İplik: {} · değiştirmek için tıklayın",
                    crate::threads::nearest(self.view_tools.brand, c).1
                ),
                SelThread::Mixed => {
                    "Birkaç iplik · tüm seçili katmanlar için birini seçmek üzere tıklayın".into()
                }
                SelThread::None => "İplik rengini ayarlamak için katman seçin".into(),
            });
            if resp.clicked() {
                // (Open for an outline, it becomes the selection's.)
                if self.picker.is_some() && self.picker_for.is_none() {
                    self.close_picker();
                } else {
                    self.open_picker(r.right_top() + Vec2::new(10.0, -8.0), None);
                }
            }
            // A floating panel rather than a popup, so the palette menus
            // inside it can open their own popups.
            if let Some(at) = self.picker {
                let ctx = ui.ctx().clone();
                let area = egui::Area::new(ui.make_persistent_id("thread-picker"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(at)
                    .show(&ctx, |ui| {
                        egui::Frame::popup(ui.style())
                            .inner_margin(crate::theme::pad())
                            .show(ui, |ui| self.thread_picker(ui));
                    });
                // A click anywhere else closes it (but not one in its menus).
                let outside = ctx.input(|i| {
                    i.pointer.any_pressed()
                        && i.pointer
                            .interact_pos()
                            .is_some_and(|p| !area.response.rect.contains(p) && !r.contains(p))
                });
                if outside && !egui::Popup::is_any_open(&ctx) {
                    self.close_picker();
                }
            }
            // At the bottom, apart from the tools: the agent's panel.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(10.0);
                let on = self.agent_showing();
                let r = icon_button(ui, Icon::Sparkle, "Ajan", on, 30.0).on_hover_text(format!(
                    "{} (Cmd+K){}",
                    self.agent.kind.name(),
                    if self.agent.running() {
                        " · çalışıyor"
                    } else {
                        ""
                    }
                ));
                if r.clicked() {
                    self.toggle_agent();
                }
            });
        });
    }

    pub fn status_bar(&mut self, ui: &mut egui::Ui) {
        let p = pal_ui(ui);
        let total = self.design.stitch_count();
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let icon = if self.playing {
                Icon::Pause
            } else {
                Icon::Play
            };
            ui.add_enabled_ui(total > 0, |ui| {
                if icon_button(
                    ui,
                    icon,
                    if self.playing { "Duraklat" } else { "Oynat" },
                    false,
                    22.0,
                )
                .on_hover_text("Dikiş animasyonunu oynat (Boşluk)")
                .clicked()
                {
                    self.toggle_play();
                }
            });
            let mut pos = self.player.unwrap_or(total);
            ui.spacing_mut().slider_width = 220.0;
            let slider = ui
                .add_enabled(
                    total > 0,
                    egui::Slider::new(&mut pos, 0..=total.max(1)).show_value(false),
                )
                .on_hover_text("Dikiş sırasında ileri geri gezin");
            slider.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Slider, total > 0, "Dikiş konumu")
            });
            if slider.changed() {
                self.playing = false;
                self.player = if pos >= total { None } else { Some(pos) };
            }
            ui.label(
                egui::RichText::new(format!("{} / {}", fmt_int(pos.min(total)), fmt_int(total)))
                    .size(11.0)
                    .color(p.secondary)
                    .monospace(),
            );
            egui::ComboBox::from_id_salt("speed")
                .selected_text(
                    egui::RichText::new(format!("{}×", (self.speed / 400.0).max(0.25))).size(11.0),
                )
                .width(52.0)
                .show_ui(ui, |ui| {
                    for (v, l) in [
                        (100.0, "0.25×"),
                        (400.0, "1×"),
                        (1600.0, "4×"),
                        (6400.0, "16×"),
                    ] {
                        ui.selectable_value(&mut self.speed, v, l);
                    }
                })
                .response
                .on_hover_text("Oynatma hızı");
            ui.add_space(8.0);
            use crate::canvas::StitchView;
            crate::theme::segmented(
                ui,
                "stitch-view",
                &mut self.view_tools.stitch_view,
                &[
                    (StitchView::Thread, "İplik"),
                    (StitchView::Lines, "Dikiş Çizgileri"),
                ],
            );
            let b = self.design_bounds();
            let stats = format!(
                "{} · {} renk · {} kesim · ~{:.0} dk",
                crate::units::fmt_size(b.width(), b.height()),
                self.design.blocks.len(),
                self.design.trim_count(),
                (total as f64 / SPM).ceil()
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.issues_button(ui);
                self.machine_chip(ui);
                self.mcp_chip(ui);
                ui.add_space(4.0);
                self.units_picker(ui);
                if self.planner.pending {
                    // A big design re-planning on the worker: the stitches
                    // shown are the last plan's until it's done.
                    ui.label(
                        egui::RichText::new("Dikişler güncelleniyor…")
                            .size(11.0)
                            .color(p.secondary),
                    );
                    ui.add(egui::Spinner::new().size(10.0));
                }
                ui.label(egui::RichText::new(stats).size(11.0).color(p.secondary))
                    .on_hover_text(format!(
                        "Dakikada {SPM} dikiş varsayımıyla tahmin. {:.1} ms'de planlandı.",
                        self.plan_ms
                    ));
                ui.add_space(12.0);
                self.status_message(ui);
            });
        });
    }

    /// The unit lengths are shown in (also View › Units).
    fn units_picker(&mut self, ui: &mut egui::Ui) {
        let mut u = self.dock.units;
        egui::ComboBox::from_id_salt("units")
            .selected_text(egui::RichText::new(u.suffix()).size(11.0))
            .width(44.0)
            .show_ui(ui, |ui| {
                for v in crate::units::Unit::ALL {
                    ui.selectable_value(&mut u, v, format!("{} ({})", v.label(), v.suffix()));
                }
            })
            .response
            .on_hover_text("Boyutlar, konumlar, ızgara ve ölçüm için birimler");
        if u != self.dock.units {
            self.set_units(u);
        }
    }

    /// The design's issues at the right of the status bar: a warning sign
    /// and how many (colored by the worst), opening the list above it.
    fn issues_button(&mut self, ui: &mut egui::Ui) {
        use zigzag::checks::Level;
        let p = pal_ui(ui);
        let n = self.issues.len();
        let worst = self.issues.iter().map(|i| match i.level {
            Level::Error => 2,
            Level::Warning => 1,
            Level::Info => 0,
        });
        let (color, icon) = match worst.max() {
            Some(2) => (p.error, Icon::Warning),
            Some(1) => (p.warning, Icon::Warning),
            Some(_) => (p.secondary, Icon::Info),
            // (The machine's and the agent's gray beside it: a fainter
            // one makes its thin diagonals look lighter than they are.)
            None => (p.secondary, Icon::Check),
        };
        let text = if n == 0 { String::new() } else { n.to_string() };
        let galley = ui
            .painter()
            .layout_no_wrap(text, egui::FontId::proportional(11.5), color);
        let w = 26.0 + if n == 0 { 0.0 } else { galley.size().x + 4.0 };
        let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 22.0), egui::Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Sorunlar"));
        let popup_id = egui::Id::unique("issues-popover");
        let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
        if resp.hovered() || open {
            ui.painter().rect_filled(r, 5.0, p.hover);
        }
        crate::icons::paint(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(r.left() + 13.0, r.center().y),
                Vec2::splat(crate::theme::STATUS_ICON),
            ),
            icon,
            color,
        );
        if n > 0 {
            ui.painter().galley(
                Pos2::new(r.left() + 24.0, r.center().y - galley.size().y / 2.0),
                galley,
                color,
            );
        }
        let resp = resp.on_hover_text(match n {
            0 => "Sorun bulunamadı".to_string(),
            1 => "1 sorun".to_string(),
            n => format!("{n} sorun"),
        });
        if std::mem::take(&mut self.open_issues) && !open {
            egui::Popup::open_id(ui.ctx(), popup_id);
        }
        let busy = self.agent.running();
        let mut fix = false;
        // As tall as there's room for above the status bar.
        let room = (resp.rect.top() - ui.ctx().content_rect().top() - 120.0).clamp(120.0, 440.0);
        egui::Popup::from_toggle_button_response(&resp)
            .id(popup_id)
            .align(egui::RectAlign::TOP_END)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            // (Its parts pad themselves, so the dividers go edge to edge.)
            .frame(egui::Frame::popup(ui.style()).inner_margin(egui::Margin::ZERO))
            .width(ISSUES_W)
            .show(|ui| {
                ui.set_width(ISSUES_W);
                ui.spacing_mut().item_spacing.y = 0.0;
                // The agent can fix them: one is installed, and the
                // studio's MCP server (which it works through) is running.
                // (Asked here, with the list showing: finding the agent's
                // program runs the login shell, the first time.)
                let can_fix = n > 0
                    && self.mcp.is_some()
                    && self.agent.installed(self.agent.kind, ui.ctx()) == Some(true);
                // The title, staying put while the list scrolls under it: a
                // bar like a panel's header, as tall and with its text.
                let radius = ui.style().visuals.menu_corner_radius;
                let (bar, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), crate::dock::HEADER_H),
                    Sense::hover(),
                );
                ui.painter().rect_filled(
                    bar,
                    egui::CornerRadius {
                        nw: radius.nw,
                        ne: radius.ne,
                        sw: 0,
                        se: 0,
                    },
                    p.header,
                );
                let pad = f32::from(crate::dock::ISSUE_PAD.left);
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(bar.shrink2(Vec2::new(pad, 0.0)))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Sorunlar").size(12.0).color(p.text));
                            if n > 0 {
                                ui.label(
                                    egui::RichText::new(n.to_string())
                                        .size(12.0)
                                        .color(p.secondary),
                                );
                            }
                            if can_fix {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let b = ui
                                            .add_enabled(
                                                !busy,
                                                egui::Button::new(
                                                    egui::RichText::new("Düzelt")
                                                        .size(12.0)
                                                        .color(Color32::WHITE),
                                                )
                                                .small()
                                                .fill(p.accent)
                                                .stroke(Stroke::NONE),
                                            )
                                            .on_hover_text(format!(
                                                "Bunları tasarımda düzeltmesini {} ajanından iste",
                                                self.agent.kind.name()
                                            ))
                                            .on_disabled_hover_text("Ajan çalışıyor");
                                        fix = b.clicked();
                                    },
                                );
                            }
                        });
                    },
                );
                crate::dock::divider(ui);
                // (Room for a couple of issues at least: with none, or one,
                // it isn't a sliver over the status bar.)
                ui.scope(|ui| {
                    ui.set_min_height(ISSUES_MIN_H);
                    self.issues_body(ui, room);
                });
            });
        if fix {
            egui::Popup::close_id(ui.ctx(), popup_id);
            self.fix_issues_with_agent(ui.ctx());
        }
    }

    fn status_message(&mut self, ui: &mut egui::Ui) {
        let p = pal_ui(ui);
        let w = ui.available_width();
        ui.add_sized(
            [w, 18.0],
            egui::Label::new(
                egui::RichText::new(&self.status)
                    .size(11.0)
                    .color(p.secondary),
            )
            .truncate()
            .halign(egui::Align::LEFT),
        );
    }

    /// A floating HUD capsule on the canvas (tool hints), Mac-style.
    pub fn hud(&self, painter: &egui::Painter, canvas: Rect, msg: &str) {
        let p = crate::theme::pal(painter.ctx());
        let galley = painter.layout(
            msg.into(),
            egui::FontId::proportional(12.0),
            Color32::WHITE,
            canvas.width() - 120.0,
        );
        let size = galley.size() + Vec2::new(28.0, 12.0);
        let pos = Pos2::new(
            canvas.center().x - size.x / 2.0,
            canvas.bottom() - self.hud_lift - size.y - 18.0,
        );
        let bg = Rect::from_min_size(pos, size);
        painter.rect_filled(bg, size.y.min(30.0) / 2.0, p.hud);
        painter.galley(bg.min + Vec2::new(14.0, 6.0), galley, Color32::WHITE);
    }
}
