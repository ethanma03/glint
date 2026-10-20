//! Overlays: the auto-hiding info bar, details panel, toasts, command palette, help, modals.

use std::path::Path;

use egui::{Align2, Color32, CornerRadius, Frame, Key, Margin, Modifiers, Rect, RichText, Stroke};

use super::commands::{COMMANDS, split_args};
use super::{ACCENT, CmpMode, GlintApp, HudMode, TEXT_DIM};
use crate::analysis::hex;

pub(super) fn card() -> Frame {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(22, 22, 26, 236))
        .stroke(Stroke::new(1.0, Color32::from_white_alpha(16)))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(14, 9))
        .shadow(egui::epaint::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(110) })
}

fn dot(ui: &mut egui::Ui) {
    ui.label(RichText::new("·").color(TEXT_DIM));
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn human_size(n: u64) -> String {
    const U: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < 3 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{n} B") } else { format!("{v:.1} {}", U[i]) }
}

/// Palette entries: (label, command line).
const PALETTE: &[(&str, &str)] = &[
    ("Fit to window", "zoom fit"),
    ("Actual size (100%)", "zoom 100"),
    ("Zoom 200%", "zoom 200"),
    ("Zoom 400%", "zoom 400"),
    ("Zoom 800%", "zoom 800"),
    ("Toggle pixel grid", "grid toggle"),
    ("Toggle smooth scaling", "smooth toggle"),
    ("Background: dark checker", "bg checker"),
    ("Background: light checker", "bg light-checker"),
    ("Background: black", "bg black"),
    ("Background: white", "bg white"),
    ("Background: magenta", "bg magenta"),
    ("Sprite sheet panel…", "sheet panel"),
    ("Sprite sheet: detect grid", "sheet auto"),
    ("Sprite sheet 8×8", "sheet 8x8"),
    ("Sprite sheet 16×16", "sheet 16x16"),
    ("Sprite sheet 32×32", "sheet 32x32"),
    ("Sprite sheet 64×64", "sheet 64x64"),
    ("Sprite sheet off", "sheet off"),
    ("Play / pause animation", "play toggle"),
    ("Next frame", "frame next"),
    ("Previous frame", "frame prev"),
    ("Animation speed 0.5×", "speed 0.5"),
    ("Animation speed 1×", "speed 1"),
    ("Animation speed 2×", "speed 2"),
    ("Copy image", "copy image"),
    ("Copy file path", "copy path"),
    ("Show in Explorer", "reveal"),
    ("Reload from disk", "reload"),
    ("Toggle details panel", "info toggle"),
    ("Info bar: auto / always / hidden", "hud toggle"),
    ("Fullscreen", "fullscreen toggle"),
    ("Compare: diff", "mode diff"),
    ("Compare: side by side", "mode side"),
    ("Compare: onion skin", "mode onion"),
    ("Compare: swipe", "mode swipe"),
    ("Compare: swap A/B", "swap"),
    ("Exit compare", "compare off"),
    ("Exposure +1 EV", "exposure +1"),
    ("Exposure −1 EV", "exposure -1"),
    ("Move to Recycle Bin…", "delete"),
    ("Keyboard shortcuts", "help"),
];

const SHORTCUTS: &[(&str, &str)] = &[
    ("← → / PgUp PgDn", "Previous / next image"),
    ("Home / End", "First / last image"),
    ("Wheel / + −", "Zoom (snaps to clean steps)"),
    ("0", "Fit to window"),
    ("1 … 9", "Zoom 100% … 900%"),
    ("Drag / double-click", "Pan / toggle fit"),
    ("Space", "Play / pause animation"),
    (", .", "Previous / next frame"),
    ("G / Shift+G", "Pixel grid / sprite-sheet panel"),
    ("B", "Cycle background"),
    ("S", "Smooth vs crisp scaling"),
    ("I / H", "Details panel / info bar"),
    ("C / Shift+C", "Copy image / copy hex under cursor"),
    ("Ctrl+Shift+C", "Copy file path"),
    ("M / Tab", "Compare mode / swap A↔B"),
    ("Alt+Wheel", "Adjust onion / swipe split"),
    ("E / Shift+E", "HDR exposure up / down"),
    ("F / F11", "Fullscreen"),
    ("Ctrl+R / F5", "Reload"),
    ("Delete", "Move to Recycle Bin"),
    ("Ctrl+P", "Command palette"),
    ("Esc", "Leave compare / sheet / fullscreen"),
];

impl GlintApp {
    pub fn draw_overlays(&mut self, ctx: &egui::Context, rect: Rect) {
        let now = self.last_time;
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let near_bottom = pointer.is_some_and(|p| p.y > rect.bottom() - 120.0 && rect.contains(p));
        let recently = now - self.last_activity < 2.2;
        let want = match self.settings.hud {
            HudMode::Always => true,
            HudMode::Hidden => false,
            HudMode::Auto => recently || near_bottom || self.doc.is_none() && self.compare.is_none(),
        };
        if want && self.settings.hud == HudMode::Auto && !near_bottom {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64((2.25 - (now - self.last_activity)).max(0.05)));
        }
        let alpha = ctx.animate_bool_with_time(egui::Id::unique("hud-vis"), want, 0.22);

        if alpha > 0.0 && (self.doc.is_some() || self.compare.is_some()) {
            egui::Area::new(egui::Id::unique("hud"))
                .anchor(Align2::CENTER_BOTTOM, [0.0, -14.0])
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    ui.multiply_opacity(alpha);
                    card().show(ui, |ui| self.hud_contents(ui));
                });
        }
        if self.settings.info && self.compare.is_none() {
            self.info_panel(ctx);
        }
        self.sheet_preview(ctx, rect);
        self.sheet_panel(ctx);
        if let Some((msg, t)) = self.toast.clone() {
            let a = (1.0 - ((now - t - 1.4) / 0.4).clamp(0.0, 1.0)) as f32;
            egui::Area::new(egui::Id::unique("toast"))
                .anchor(Align2::CENTER_TOP, [0.0, 16.0])
                .order(egui::Order::Tooltip)
                .interactable(false)
                .show(ctx, |ui| {
                    ui.multiply_opacity(a);
                    card().show(ui, |ui| ui.label(msg));
                });
        }
        let loading = self.is_busy();
        if loading {
            egui::Area::new(egui::Id::unique("spinner"))
                .anchor(Align2::RIGHT_TOP, [-16.0, 16.0])
                .interactable(false)
                .show(ctx, |ui| {
                    ui.add(egui::Spinner::new().size(18.0).color(ACCENT));
                });
        }
    }

    fn hud_contents(&mut self, ui: &mut egui::Ui) {
        let mut run: Option<String> = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 7.0;
            if let Some(c) = &self.compare {
                let m = &c.metrics;
                ui.label(RichText::new(format!("{}  ⟷  {}", file_name(&c.a.path), file_name(&c.b.path))).strong());
                dot(ui);
                if m.identical {
                    ui.label(RichText::new("identical").color(Color32::from_rgb(120, 220, 140)));
                } else {
                    ui.label(
                        RichText::new(format!("{} px differ ({:.3}%)", m.differing_pixels, m.differing_percent))
                            .color(Color32::from_rgb(255, 110, 150)),
                    );
                    dot(ui);
                    ui.label(format!("max Δ {}", m.max_channel_diff));
                    if let Some(p) = m.psnr_db {
                        dot(ui);
                        ui.label(format!("PSNR {p:.1} dB"));
                    }
                }
                if m.size_mismatch {
                    dot(ui);
                    ui.label(
                        RichText::new(format!("size {}×{} vs {}×{}", m.a_size[0], m.a_size[1], m.b_size[0], m.b_size[1]))
                            .color(ACCENT),
                    );
                }
            } else if let Some(doc) = &self.doc {
                let d = &doc.dec;
                ui.label(RichText::new(file_name(&d.path)).strong());
                dot(ui);
                ui.label(format!("{} × {}", d.width(), d.height()));
                dot(ui);
                ui.label(RichText::new(&d.format).color(TEXT_DIM));
                if self.files.len() > 1 {
                    dot(ui);
                    ui.label(RichText::new(format!("{} / {}", self.index + 1, self.files.len())).color(TEXT_DIM));
                }
            }
            dot(ui);
            ui.label(RichText::new(format!("{:.0}%", self.view.zoom * 100.0)).monospace());
            if let Some(px) = self.hover_px {
                let size = self.content_size().unwrap_or_default();
                if px.x >= 0.0 && px.y >= 0.0 && px.x < size.x && px.y < size.y {
                    dot(ui);
                    ui.label(RichText::new(format!("{}, {}", px.x as u32, px.y as u32)).monospace().color(TEXT_DIM));
                    let rgba = if let Some(c) = &self.compare {
                        let get = |img: &image::RgbaImage| {
                            ((px.x as u32) < img.width() && (px.y as u32) < img.height())
                                .then(|| img.get_pixel(px.x as u32, px.y as u32).0)
                        };
                        let (a, b) = (get(c.la.frame(0)), get(c.lb.frame(0)));
                        for (n, v) in [("A", a), ("B", b)] {
                            if let Some(v) = v {
                                swatch(ui, v);
                                ui.label(RichText::new(format!("{n} {}", hex(v))).monospace());
                            }
                        }
                        None
                    } else {
                        self.hover_rgba()
                    };
                    if let Some(v) = rgba {
                        swatch(ui, v);
                        ui.label(RichText::new(hex(v)).monospace());
                    }
                }
            }
            if let Some(s) = &self.sheet {
                if let (Some(px), Some(doc)) = (self.hover_px, &self.doc) {
                    if let Some(i) = s.grid.cell_at(px.x, px.y, doc.dec.width(), doc.dec.height()) {
                        dot(ui);
                        ui.label(RichText::new(format!("cell {i}")).color(super::SHEET));
                    }
                }
            }
        });

        // Second row: compare controls or animation transport.
        if let Some(c) = &mut self.compare {
            ui.horizontal(|ui| {
                for m in CmpMode::ALL {
                    if ui.selectable_label(c.mode == m, m.label()).clicked() {
                        run = Some(format!("mode {}", format!("{m:?}").to_lowercase()));
                    }
                }
                if matches!(c.mode, CmpMode::Onion | CmpMode::Swipe) {
                    ui.add(egui::Slider::new(&mut c.split, 0.0..=1.0).show_value(false));
                }
                if ui.button("⇄ Swap").clicked() {
                    run = Some("swap".into());
                }
            });
        } else if let Some(doc) = &self.doc {
            let n = doc.dec.frames.len();
            if n > 1 {
                ui.horizontal(|ui| {
                    if ui.button("⏮").clicked() {
                        run = Some("frame prev".into());
                    }
                    let play = if self.anim.playing { "⏸" } else { "▶" };
                    if doc.dec.is_animated() && ui.button(play).clicked() {
                        run = Some("play toggle".into());
                    }
                    if ui.button("⏭").clicked() {
                        run = Some("frame next".into());
                    }
                    let mut f = self.anim.frame;
                    let resp = ui.add(egui::Slider::new(&mut f, 0..=n - 1).show_value(false));
                    if resp.changed() {
                        self.anim.frame = f;
                        self.anim.playing = false;
                    }
                    let delay = doc.dec.frames[self.anim.frame.min(n - 1)].delay_ms;
                    let unit = if doc.dec.is_animated() { "frame" } else { "page" };
                    let mut txt = format!("{unit} {} / {n}", self.anim.frame + 1);
                    if delay > 0 {
                        txt += &format!("  ·  {delay} ms");
                    }
                    if (self.anim.speed - 1.0).abs() > 1e-3 {
                        txt += &format!("  ·  {}×", self.anim.speed);
                    }
                    ui.label(RichText::new(txt).monospace().color(TEXT_DIM));
                    for t in &doc.dec.tags {
                        let active = self.anim.range == Some((t.from as usize, t.to as usize));
                        if ui.selectable_label(active, &t.name).clicked() {
                            run = Some(if active { "tag off".into() } else { format!("tag \"{}\"", t.name) });
                        }
                    }
                });
            }
        }
        if let Some(cmd) = run {
            let ctx = ui.ctx().clone();
            if let Err(e) = self.exec(&ctx, &split_args(&cmd), None) {
                self.toast(e);
            }
        }
    }

    pub fn draw_error(&self, ctx: &egui::Context, rect: Rect) {
        let Some((path, msg)) = &self.error else { return };
        egui::Area::new(egui::Id::unique("error"))
            .pivot(Align2::CENTER_CENTER)
            .fixed_pos(rect.center())
            .show(ctx, |ui| {
                card().inner_margin(Margin::same(22)).show(ui, |ui| {
                    ui.set_max_width(460.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("⚠").size(30.0).color(ACCENT));
                        ui.label(RichText::new(format!("Can't open {}", file_name(path))).size(16.0).strong());
                        ui.add_space(4.0);
                        ui.label(RichText::new(msg).color(TEXT_DIM));
                        ui.add_space(6.0);
                        ui.label(RichText::new(path.display().to_string()).small().color(TEXT_DIM.gamma_multiply(0.7)));
                    });
                });
            });
    }

    fn info_panel(&mut self, ctx: &egui::Context) {
        let Some(doc) = &mut self.doc else { return };
        if !doc.stats_started {
            doc.stats_started = true;
            let (img, slot, ctx2) = (doc.layer.frame(0).clone(), doc.stats.clone(), ctx.clone());
            std::thread::spawn(move || {
                let s = crate::analysis::stats(&img);
                *slot.lock().unwrap() = Some(s);
                ctx2.request_repaint();
            });
        }
        let d = doc.dec.clone();
        let stats = doc.stats.lock().unwrap().clone();
        egui::Area::new(egui::Id::unique("info"))
            .anchor(Align2::RIGHT_TOP, [-14.0, 14.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                card().inner_margin(Margin::same(14)).show(ui, |ui| {
                    ui.set_width(300.0);
                    ui.label(RichText::new(file_name(&d.path)).strong().size(14.5));
                    if let Some(dir) = d.path.parent() {
                        ui.label(RichText::new(dir.display().to_string()).small().color(TEXT_DIM));
                    }
                    ui.add_space(6.0);
                    egui::Grid::new("info-grid").num_columns(2).spacing([14.0, 4.0]).show(ui, |ui| {
                        let mut row = |k: &str, v: String| {
                            ui.label(RichText::new(k).color(TEXT_DIM));
                            ui.label(v);
                            ui.end_row();
                        };
                        row("Format", d.format.clone());
                        row("Decoder", d.decoder.to_string());
                        row("Pixels", d.color.clone());
                        row("Size", format!("{} × {}", d.width(), d.height()));
                        if d.frames.len() > 1 {
                            let total: u32 = d.frames.iter().map(|f| f.delay_ms).sum();
                            let extra = if total > 0 { format!(" · {:.2}s", total as f64 / 1000.0) } else { String::new() };
                            row("Frames", format!("{}{extra}", d.frames.len()));
                        }
                        row("File", human_size(d.file_size));
                        match &stats {
                            Some(s) => {
                                row("Colors", s.unique_colors.to_string());
                                row(
                                    "Alpha",
                                    if s.transparent_pixels == 0 {
                                        "opaque".into()
                                    } else {
                                        format!("{} transparent, {} partial", s.transparent_pixels, s.semi_transparent_pixels)
                                    },
                                );
                                if let Some([x, y, w, h]) = s.opaque_bbox {
                                    if s.transparent_pixels > 0 {
                                        row("Content", format!("{w}×{h} at {x},{y}"));
                                    }
                                }
                            }
                            None => row("Colors", "…".into()),
                        }
                    });
                    for n in &d.notes {
                        ui.add_space(4.0);
                        ui.label(RichText::new(n).small().color(TEXT_DIM));
                    }
                });
            });
    }

    fn sheet_preview(&mut self, ctx: &egui::Context, rect: Rect) {
        let (Some(sheet), Some(doc)) = (&self.sheet, &mut self.doc) else { return };
        if sheet.play.is_none() {
            return;
        }
        let w = doc.dec.width();
        let [x, y, cw, ch] = sheet.grid.cell_rect(self.anim.sheet_cell, w);
        let img = doc.layer.frame(0).clone();
        let smooth = self.settings.smooth;
        let tiles = doc.layer.tiles(ctx, 0, smooth);
        if tiles.len() != 1 {
            return;
        }
        let tex = tiles[0].tex.id();
        let (iw, ih) = (img.width() as f32, img.height() as f32);
        let uv = Rect::from_min_max(
            egui::pos2(x as f32 / iw, y as f32 / ih),
            egui::pos2((x + cw) as f32 / iw, (y + ch) as f32 / ih),
        );
        let ppp = ctx.pixels_per_point();
        let longest = cw.max(ch) as f32;
        let s = if longest * 2.0 <= 200.0 * ppp { ((200.0 * ppp) / longest).floor() } else { (200.0 * ppp) / longest };
        let size = egui::vec2(cw as f32, ch as f32) * s / ppp;
        let cell = self.anim.sheet_cell;
        let _ = rect;
        egui::Area::new(egui::Id::unique("sheet-preview"))
            .anchor(Align2::RIGHT_BOTTOM, [-14.0, -84.0])
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                card().inner_margin(Margin::same(10)).show(ui, |ui| {
                    let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    ui.painter().rect_filled(r, 4.0, Color32::from_gray(0x22));
                    ui.painter().image(tex, r, uv, Color32::WHITE);
                    ui.label(RichText::new(format!("cell {cell}")).small().color(TEXT_DIM));
                });
            });
    }

    pub fn context_menu(&mut self, resp: &egui::Response) {
        let mut cmd: Option<&str> = None;
        resp.context_menu(|ui| {
            ui.set_min_width(190.0);
            let has_px = self.hover_rgba().is_some();
            let mut items: Vec<(&str, &'static str)> = vec![("Copy image", "copy image"), ("Copy path", "copy path")];
            if has_px {
                items.push(("Copy color under cursor", "copy hex"));
            }
            items.push(("Show in Explorer", "reveal"));
            menu_items(ui, &items, &mut cmd);
            ui.separator();
            menu_items(ui, &[("Fit to window", "zoom fit"), ("Actual size", "zoom 100"), ("Toggle pixel grid", "grid toggle")], &mut cmd);
            // Inline swatches instead of a submenu: no hover gap to cross.
            ui.add_space(2.0);
            ui.label(RichText::new("Background").small().color(TEXT_DIM));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let cur = self.settings.bg;
                let g = Color32::from_gray;
                for (bg, fill, tip, c) in [
                    (super::Bg::DarkChecker, [g(0x2b), g(0x22)], "Dark checker", "bg checker"),
                    (super::Bg::LightChecker, [g(0xee), g(0xbe)], "Light checker", "bg light-checker"),
                    (super::Bg::Black, [Color32::BLACK; 2], "Black", "bg black"),
                    (super::Bg::Gray, [g(0x80); 2], "Gray", "bg gray"),
                    (super::Bg::White, [Color32::WHITE; 2], "White", "bg white"),
                    (super::Bg::Magenta, [Color32::from_rgb(255, 0, 255); 2], "Magenta", "bg magenta"),
                ] {
                    if super::sheet_panel::bg_swatch(ui, fill, cur == bg, tip) {
                        cmd = Some(c);
                        ui.close();
                    }
                }
            });
            ui.add_space(2.0);
            let mut sheet_items: Vec<(&str, &'static str)> = vec![("Sprite sheet…", "sheet panel")];
            if self.sheet.is_some() {
                sheet_items.push(("Hide sprite sheet", "sheet off"));
            }
            menu_items(ui, &sheet_items, &mut cmd);
            menu_items(ui, &[("Details", "info toggle")], &mut cmd);
            ui.separator();
            menu_items(ui, &[("Command palette…", "palette"), ("Move to Recycle Bin…", "delete")], &mut cmd);
        });
        if let Some(c) = cmd {
            let ctx = resp.ctx.clone();
            if let Err(e) = self.exec(&ctx, &split_args(c), None) {
                self.toast(e);
            }
        }
    }

    pub fn palette_ui(&mut self, ctx: &egui::Context) {
        if !self.palette.open {
            return;
        }
        let q = self.palette.text.to_lowercase();
        let matches: Vec<(&str, &str)> = PALETTE
            .iter()
            .copied()
            .filter(|(l, c)| q.is_empty() || fuzzy(&l.to_lowercase(), &q) || c.starts_with(q.as_str()))
            .take(10)
            .collect();
        let (down, up, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::Enter),
            )
        });
        if down {
            self.palette.selected = (self.palette.selected + 1).min(matches.len().saturating_sub(1));
        }
        if up {
            self.palette.selected = self.palette.selected.saturating_sub(1);
        }
        let mut run: Option<String> = None;
        let modal = egui::Modal::new(egui::Id::unique("palette"))
            .backdrop_color(Color32::from_black_alpha(90))
            .frame(card().inner_margin(Margin::same(10)))
            .show(ctx, |ui| {
                ui.set_width(520.0);
                let te = egui::TextEdit::singleline(&mut self.palette.text)
                    .hint_text("Type a command…   e.g.  zoom 400,  sheet 32x32 play 0-7,  bg #202030")
                    .desired_width(f32::INFINITY)
                    .frame(egui::Frame::NONE)
                    .font(egui::FontId::proportional(16.0));
                let r = ui.add(te);
                if self.palette.just_opened {
                    r.request_focus();
                    self.palette.just_opened = false;
                }
                if r.changed() {
                    self.palette.selected = 0;
                }
                ui.separator();
                for (i, (label, cmd)) in matches.iter().enumerate() {
                    let sel = i == self.palette.selected;
                    let resp = ui.add_sized(
                        [ui.available_width(), 26.0],
                        egui::Button::selectable(sel, RichText::new(*label))
                            .right_text(RichText::new(*cmd).monospace().color(TEXT_DIM)),
                    );
                    if resp.clicked() {
                        run = Some(cmd.to_string());
                    }
                }
                if matches.is_empty() && !q.is_empty() {
                    ui.label(RichText::new("Enter runs this as a command (glint ctl syntax)").small().color(TEXT_DIM));
                }
            });
        if enter {
            let typed = self.palette.text.trim().to_string();
            let first_word_is_cmd = typed
                .split_whitespace()
                .next()
                .is_some_and(|w| COMMANDS.iter().any(|(u, _)| u.split(['|', ' ']).any(|t| t == w)));
            run = if first_word_is_cmd && (matches.is_empty() || typed.contains(' ')) {
                Some(typed)
            } else {
                matches.get(self.palette.selected).map(|(_, c)| c.to_string()).or(Some(typed))
            };
        }
        if modal.should_close() {
            self.palette.open = false;
        }
        if let Some(cmd) = run.filter(|c| !c.is_empty()) {
            self.palette.open = false;
            if let Err(e) = self.exec(ctx, &split_args(&cmd), None) {
                self.toast(e);
            }
        }
    }

    pub fn modals(&mut self, ctx: &egui::Context) {
        if self.help {
            let m = egui::Modal::new(egui::Id::unique("help")).frame(card().inner_margin(Margin::same(20))).show(ctx, |ui| {
                ui.label(RichText::new("Keyboard shortcuts").size(17.0).strong());
                ui.add_space(8.0);
                egui::Grid::new("keys").num_columns(2).spacing([24.0, 5.0]).show(ui, |ui| {
                    for (k, d) in SHORTCUTS {
                        ui.label(RichText::new(*k).monospace().color(ACCENT));
                        ui.label(*d);
                        ui.end_row();
                    }
                });
                ui.add_space(8.0);
                ui.label(RichText::new("Everything is scriptable:  glint ctl help").small().color(TEXT_DIM));
            });
            if m.should_close() {
                self.help = false;
            }
        }
        if let Some(path) = self.confirm_delete.clone() {
            let mut close = false;
            let mut confirm = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
            let m = egui::Modal::new(egui::Id::unique("delete")).frame(card().inner_margin(Margin::same(20))).show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(RichText::new("Move to Recycle Bin?").size(16.0).strong());
                ui.label(RichText::new(file_name(&path)).color(TEXT_DIM));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("  Delete  ").color(Color32::from_rgb(255, 120, 120))).clicked() {
                        confirm = true;
                    }
                    if ui.button("  Cancel  ").clicked() {
                        close = true;
                    }
                });
            });
            if m.should_close() {
                close = true;
            }
            if confirm {
                match trash::delete(&path) {
                    Ok(()) => {
                        self.toast(format!("Moved {} to the Recycle Bin", file_name(&path)));
                        self.loader.invalidate(&path);
                        if let Some(i) = self.files.iter().position(|p| p == &path) {
                            self.files.remove(i);
                        }
                        if self.files.is_empty() {
                            self.doc = None;
                            self.current = None;
                        } else {
                            let i = self.index.min(self.files.len() - 1);
                            self.goto_index(i);
                        }
                    }
                    Err(e) => self.toast(format!("Couldn't delete: {e}")),
                }
                close = true;
            }
            if close {
                self.confirm_delete = None;
            }
        }
    }
}

fn menu_items(ui: &mut egui::Ui, items: &[(&str, &'static str)], cmd: &mut Option<&'static str>) {
    for (label, c) in items {
        if ui.button(*label).clicked() {
            *cmd = Some(c);
            ui.close();
        }
    }
}

fn swatch(ui: &mut egui::Ui, v: [u8; 4]) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter().rect_filled(r, 3.0, Color32::from_gray(0x40));
    ui.painter().rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(v[0], v[1], v[2], v[3]));
    ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, Color32::from_white_alpha(40)), egui::StrokeKind::Outside);
}

/// Subsequence match: "tgg" matches "toggle grid".
fn fuzzy(hay: &str, needle: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().filter(|c| !c.is_whitespace()).all(|c| it.any(|h| h == c))
}
