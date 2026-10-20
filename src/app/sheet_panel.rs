//! Floating sprite-sheet panel: cell size or columns×rows, offset, gap, detect, cell animation.

use egui::{Align2, Color32, DragValue, Margin, RichText};

use super::hud::card;
use super::{ACCENT, GlintApp, SHEET, Sheet, TEXT_DIM};
use crate::analysis::{SheetGrid, detect_grid};

pub struct SheetUi {
    pub open: bool,
    /// Size the grid by columns×rows instead of cell pixels.
    pub by_counts: bool,
    pub cell: [u32; 2],
    pub counts: [u32; 2],
    pub offset: [u32; 2],
    pub gap: [u32; 2],
    pub animate: bool,
    pub range: [u32; 2],
    pub fps: f32,
    pub status: Option<(String, bool)>,
}

impl Default for SheetUi {
    fn default() -> Self {
        Self {
            open: false,
            by_counts: false,
            cell: [32, 32],
            counts: [4, 4],
            offset: [0, 0],
            gap: [0, 0],
            animate: false,
            range: [0, 7],
            fps: 10.0,
            status: None,
        }
    }
}

impl SheetUi {
    /// Mirror an applied sheet (from a command or detection) into the controls.
    pub fn sync(&mut self, s: &Sheet) {
        self.cell = s.grid.cell;
        self.offset = s.grid.offset;
        self.gap = s.grid.gap;
        self.animate = s.play.is_some();
        if let Some(r) = s.play {
            self.range = r;
        }
        self.fps = s.fps;
    }
}

const PRESETS: &[u32] = &[8, 16, 24, 32, 48, 64];

impl GlintApp {
    pub fn open_sheet_panel(&mut self) {
        if self.sheet_ui.open {
            self.sheet_ui.open = false;
            return;
        }
        self.sheet_ui.open = true;
        self.sheet_ui.status = None;
        if self.sheet.is_none() {
            // Start from a detected grid when possible; it's right more often than a preset.
            let detected = self.doc.as_ref().and_then(|d| detect_grid(d.layer.frame(0)));
            match detected {
                Some(g) => {
                    self.set_sheet(g, None, self.sheet_ui.fps);
                    self.sheet_ui.by_counts = false;
                    self.sheet_ui.status = Some(("Grid detected from sprite gaps".into(), false));
                }
                None => self.apply_sheet_ui(),
            }
        }
    }

    /// Rebuild the overlay from the panel's values.
    pub(super) fn apply_sheet_ui(&mut self) {
        let Some((w, h)) = self.doc.as_ref().map(|d| (d.dec.width(), d.dec.height())) else { return };
        let u = &self.sheet_ui;
        let grid = if u.by_counts {
            match SheetGrid::from_counts(w, h, u.counts, u.offset, u.gap) {
                Some((g, _)) => g,
                None => return,
            }
        } else {
            SheetGrid { cell: u.cell, offset: u.offset, gap: u.gap }
        };
        let play = u.animate.then_some([u.range[0].min(u.range[1]), u.range[0].max(u.range[1])]);
        let fps = u.fps;
        let keep_cell = self.anim.sheet_cell;
        let by_counts = u.by_counts;
        let counts = u.counts;
        self.set_sheet(grid, play, fps);
        // set_sheet syncs cell size back; keep what the user is editing in counts mode.
        self.sheet_ui.by_counts = by_counts;
        self.sheet_ui.counts = counts;
        if play.is_some_and(|[a, b]| (a..=b).contains(&keep_cell)) {
            self.anim.sheet_cell = keep_cell;
        }
    }

    pub fn sheet_panel(&mut self, ctx: &egui::Context) {
        if !self.sheet_ui.open {
            return;
        }
        let dims = self.doc.as_ref().map(|d| (d.dec.width(), d.dec.height()));
        let mut changed = false;
        let mut detect = false;
        let mut off = false;
        let mut close = false;
        egui::Area::new(egui::Id::unique("sheet-panel"))
            .anchor(Align2::LEFT_TOP, [14.0, 14.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                card().inner_margin(Margin::same(14)).show(ui, |ui| {
                    ui.set_width(286.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Sprite sheet").strong().size(14.5));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("✕").on_hover_text("Close panel (overlay stays)").clicked() {
                                close = true;
                            }
                        });
                    });
                    let Some((w, h)) = dims else {
                        ui.label(RichText::new("Open an image first.").color(TEXT_DIM));
                        return;
                    };
                    let u = &mut self.sheet_ui;
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        changed |= ui.selectable_value(&mut u.by_counts, false, "Cell size").changed();
                        changed |= ui.selectable_value(&mut u.by_counts, true, "Columns × rows").changed();
                    });
                    ui.add_space(2.0);
                    egui::Grid::new("sheet-grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        let num = |ui: &mut egui::Ui, v: &mut u32, max: u32| {
                            ui.add(DragValue::new(v).range(0..=max).speed(0.1)).changed()
                        };
                        if u.by_counts {
                            ui.label(RichText::new("Grid").color(TEXT_DIM));
                            ui.horizontal(|ui| {
                                changed |= ui.add(DragValue::new(&mut u.counts[0]).range(1..=w).speed(0.05)).changed();
                                ui.label("×");
                                changed |= ui.add(DragValue::new(&mut u.counts[1]).range(1..=h).speed(0.05)).changed();
                                ui.label(RichText::new("cells").color(TEXT_DIM));
                            });
                        } else {
                            ui.label(RichText::new("Cell").color(TEXT_DIM));
                            ui.horizontal(|ui| {
                                changed |= ui.add(DragValue::new(&mut u.cell[0]).range(1..=w).speed(0.1)).changed();
                                ui.label("×");
                                changed |= ui.add(DragValue::new(&mut u.cell[1]).range(1..=h).speed(0.1)).changed();
                                ui.label(RichText::new("px").color(TEXT_DIM));
                            });
                            ui.end_row();
                            ui.label("");
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                for &p in PRESETS {
                                    if ui.selectable_label(u.cell == [p, p], p.to_string()).clicked() {
                                        u.cell = [p, p];
                                        changed = true;
                                    }
                                }
                            });
                        }
                        ui.end_row();
                        ui.label(RichText::new("Offset").color(TEXT_DIM));
                        ui.horizontal(|ui| {
                            changed |= num(ui, &mut u.offset[0], w);
                            changed |= num(ui, &mut u.offset[1], h);
                        });
                        ui.end_row();
                        ui.label(RichText::new("Gap").color(TEXT_DIM));
                        ui.horizontal(|ui| {
                            changed |= num(ui, &mut u.gap[0], w);
                            changed |= num(ui, &mut u.gap[1], h);
                        });
                        ui.end_row();
                    });

                    // Summary of the applied grid, warning when it doesn't tile the image exactly.
                    if let Some(s) = &self.sheet {
                        let g = s.grid;
                        let (cols, rows) = (g.columns(w), g.rows(h));
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(format!("{cols} × {rows} = {} cells of {}×{} px", cols * rows, g.cell[0], g.cell[1]))
                                .color(SHEET),
                        );
                        let used_w = g.offset[0] + cols * g.cell[0] + cols.saturating_sub(1) * g.gap[0];
                        let used_h = g.offset[1] + rows * g.cell[1] + rows.saturating_sub(1) * g.gap[1];
                        let (lx, ly) = (w.saturating_sub(used_w), h.saturating_sub(used_h));
                        if (lx > g.offset[0] || ly > g.offset[1]) && (lx > 0 || ly > 0) {
                            ui.label(
                                RichText::new(format!("{lx} px unused at right, {ly} px at bottom")).small().color(ACCENT),
                            );
                        }
                    }
                    if let Some((msg, warn)) = &self.sheet_ui.status {
                        ui.label(RichText::new(msg).small().color(if *warn { ACCENT } else { TEXT_DIM }));
                    }

                    ui.add_space(6.0);
                    let u = &mut self.sheet_ui;
                    ui.horizontal(|ui| {
                        changed |= ui.checkbox(&mut u.animate, "Animate cells").changed();
                        if u.animate {
                            changed |= ui.add(DragValue::new(&mut u.range[0]).range(0..=9999).speed(0.05)).changed();
                            ui.label("–");
                            changed |= ui.add(DragValue::new(&mut u.range[1]).range(0..=9999).speed(0.05)).changed();
                            changed |= ui
                                .add(DragValue::new(&mut u.fps).range(0.5..=60.0).speed(0.1).suffix(" fps"))
                                .changed();
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("✨ Detect").on_hover_text("Find the grid from the gaps between sprites").clicked() {
                            detect = true;
                        }
                        if ui.button("Hide overlay").clicked() {
                            off = true;
                        }
                    });
                    ui.label(RichText::new("Hover a cell to see its index · Shift+G toggles this panel").small().color(TEXT_DIM));
                });
            });

        if close {
            self.sheet_ui.open = false;
        }
        if off {
            self.sheet = None;
            self.sheet_ui.open = false;
            return;
        }
        if detect {
            let found = self.doc.as_ref().and_then(|d| detect_grid(d.layer.frame(0)));
            match found {
                Some(g) => {
                    let play = self.sheet.as_ref().and_then(|s| s.play);
                    self.set_sheet(g, play, self.sheet_ui.fps);
                    self.sheet_ui.by_counts = false;
                    self.sheet_ui.status = Some((format!("Detected {}×{} px cells", g.cell[0], g.cell[1]), false));
                }
                None => {
                    self.sheet_ui.status =
                        Some(("No regular grid found (sprites may touch). Set the size by hand.".into(), true))
                }
            }
        } else if changed {
            self.sheet_ui.status = None;
            self.apply_sheet_ui();
        }
    }
}

/// Clickable color swatch used for background choices in menus.
pub fn bg_swatch(ui: &mut egui::Ui, fill: [Color32; 2], selected: bool, tip: &str) -> bool {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
    let p = ui.painter();
    p.rect_filled(r, 5.0, fill[0]);
    if fill[0] != fill[1] {
        // Two-tone checker preview.
        let h = r.width() / 2.0;
        for (dx, dy) in [(0.0, 0.0), (h, h)] {
            let q = egui::Rect::from_min_size(r.min + egui::vec2(dx, dy), egui::vec2(h, h));
            p.rect_filled(q, 0.0, fill[1]);
        }
    }
    let stroke = if selected {
        egui::Stroke::new(2.0, ACCENT)
    } else if resp.hovered() {
        egui::Stroke::new(1.5, Color32::from_white_alpha(140))
    } else {
        egui::Stroke::new(1.0, Color32::from_white_alpha(40))
    };
    p.rect_stroke(r, 5.0, stroke, egui::StrokeKind::Outside);
    resp.on_hover_text(tip).clicked()
}
