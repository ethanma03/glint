//! Canvas rendering: background, image tiles, pixel grid, sprite-sheet overlay, compare modes.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, TextureOptions, Vec2};

use super::layer::Layer;
use super::{ACCENT, Bg, CmpMode, GlintApp, SHEET, TEXT, TEXT_DIM, p2};

impl GlintApp {
    fn checker_tex(&mut self, ctx: &egui::Context) -> egui::TextureId {
        self.checker
            .get_or_insert_with(|| {
                let (a, b) = (Color32::from_gray(0xff), Color32::from_gray(0xcc));
                let img = egui::ColorImage::new([2, 2], vec![a, b, b, a]);
                ctx.load_texture("glint-checker", img, TextureOptions::NEAREST_REPEAT)
            })
            .id()
    }

    /// Fill the area under the image so transparency is visible.
    fn paint_bg(&mut self, ctx: &egui::Context, painter: &Painter, r: Rect) {
        const CHECK: f32 = 8.0;
        match self.settings.bg {
            Bg::DarkChecker | Bg::LightChecker => {
                let id = self.checker_tex(ctx);
                let uv = Rect::from_min_max(p2(r.min.to_vec2() / (CHECK * 2.0)), p2(r.max.to_vec2() / (CHECK * 2.0)));
                // One light checker texture, tinted down for the dark variant (255/204 -> 2b/22).
                let tint = if self.settings.bg == Bg::LightChecker { Color32::from_gray(0xee) } else { Color32::from_gray(0x2b) };
                painter.image(id, r, uv, tint);
            }
            Bg::Black => {
                painter.rect_filled(r, 0.0, Color32::BLACK);
            }
            Bg::Gray => {
                painter.rect_filled(r, 0.0, Color32::from_gray(0x80));
            }
            Bg::White => {
                painter.rect_filled(r, 0.0, Color32::WHITE);
            }
            Bg::Magenta => {
                painter.rect_filled(r, 0.0, Color32::from_rgb(255, 0, 255));
            }
            Bg::Custom(c) => {
                painter.rect_filled(r, 0.0, Color32::from_rgb(c[0], c[1], c[2]));
            }
        }
    }

    fn paint_layer(&mut self, ctx: &egui::Context, painter: &Painter, which: Which, frame: usize, origin: Pos2, tint: Color32) {
        let scale = self.view.scale();
        let smooth = self.settings.smooth;
        let Some(layer) = self.layer_mut(which) else { return };
        for t in layer.tiles(ctx, frame, smooth) {
            let [x, y, w, h] = t.rect;
            let r = Rect::from_min_size(
                origin + egui::vec2(x as f32, y as f32) * scale,
                egui::vec2(w as f32, h as f32) * scale,
            );
            painter.image(t.tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
        }
    }

    fn layer_mut(&mut self, which: Which) -> Option<&mut Layer> {
        match which {
            Which::Doc => self.doc.as_mut().map(|d| &mut d.layer),
            Which::A => self.compare.as_mut().map(|c| &mut c.la),
            Which::B => self.compare.as_mut().map(|c| &mut c.lb),
            Which::Diff => self.compare.as_mut().map(|c| &mut c.ldiff),
        }
    }

    pub fn draw_canvas(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let ctx = ui.ctx().clone();
        let painter = ui.painter_at(rect);
        let rects = self.layout(rect);

        if let Some(cmp) = &self.compare {
            let mode = cmp.mode;
            let split = cmp.split;
            let (wa, ha) = (cmp.a.width() as f32, cmp.a.height() as f32);
            let (wb, hb) = (cmp.b.width() as f32, cmp.b.height() as f32);
            let scale = self.view.scale();
            match mode {
                CmpMode::Diff => {
                    let o = self.view.origin(rect);
                    let size = egui::vec2(wa.max(wb), ha.max(hb)) * scale;
                    painter.rect_filled(Rect::from_min_size(o, size), 0.0, Color32::BLACK);
                    self.paint_layer(&ctx, &painter, Which::Diff, 0, o, Color32::WHITE);
                    self.draw_grid(&painter, rect, o, egui::vec2(wa.max(wb), ha.max(hb)));
                }
                CmpMode::Onion => {
                    let o = self.view.origin(rect);
                    let bg = Rect::from_min_size(o, egui::vec2(wa.max(wb), ha.max(hb)) * scale).intersect(rect);
                    self.paint_bg(&ctx, &painter, bg);
                    let alpha = (split * 255.0) as u8;
                    self.paint_layer(&ctx, &painter, Which::A, 0, o, Color32::from_white_alpha(255 - alpha));
                    self.paint_layer(&ctx, &painter, Which::B, 0, o, Color32::from_white_alpha(alpha));
                    self.draw_grid(&painter, rect, o, egui::vec2(wa.max(wb), ha.max(hb)));
                }
                CmpMode::Swipe => {
                    let o = self.view.origin(rect);
                    let split_x = rect.left() + split * rect.width();
                    let left = Rect::from_min_max(rect.min, Pos2::new(split_x, rect.max.y));
                    let right = Rect::from_min_max(Pos2::new(split_x, rect.min.y), rect.max);
                    let pa = painter.with_clip_rect(left);
                    let pb = painter.with_clip_rect(right);
                    let ra = Rect::from_min_size(o, egui::vec2(wa, ha) * scale).intersect(left);
                    let rb = Rect::from_min_size(o, egui::vec2(wb, hb) * scale).intersect(right);
                    self.paint_bg(&ctx, &pa, ra);
                    self.paint_bg(&ctx, &pb, rb);
                    self.paint_layer(&ctx, &pa, Which::A, 0, o, Color32::WHITE);
                    self.paint_layer(&ctx, &pb, Which::B, 0, o, Color32::WHITE);
                    self.draw_grid(&painter, rect, o, egui::vec2(wa.max(wb), ha.max(hb)));
                    // Divider with a grab handle.
                    painter.line_segment(
                        [Pos2::new(split_x, rect.top()), Pos2::new(split_x, rect.bottom())],
                        Stroke::new(1.5, ACCENT),
                    );
                    let knob = Rect::from_center_size(Pos2::new(split_x, rect.center().y), egui::vec2(10.0, 44.0));
                    painter.rect_filled(knob, 5.0, ACCENT);
                    label(&painter, Pos2::new(split_x - 10.0, rect.top() + 12.0), Align2::RIGHT_TOP, "A");
                    label(&painter, Pos2::new(split_x + 10.0, rect.top() + 12.0), Align2::LEFT_TOP, "B");
                }
                CmpMode::Side => {
                    for (i, (sub, which, (w, h))) in
                        [(rects[0], Which::A, (wa, ha)), (rects[1], Which::B, (wb, hb))].into_iter().enumerate()
                    {
                        let p = painter.with_clip_rect(sub);
                        let o = self.view.origin(sub);
                        let r = Rect::from_min_size(o, egui::vec2(w, h) * scale).intersect(sub);
                        self.paint_bg(&ctx, &p, r);
                        self.paint_layer(&ctx, &p, which, 0, o, Color32::WHITE);
                        self.draw_grid(&p, sub, o, egui::vec2(w, h));
                        self.draw_hover_pixel(&p, o, egui::vec2(w, h));
                        let c = self.compare.as_ref().unwrap();
                        let name = if i == 0 { &c.a.path } else { &c.b.path };
                        let name = name.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        label(&p, sub.left_top() + egui::vec2(12.0, 12.0), Align2::LEFT_TOP, &format!("{} · {name}", ["A", "B"][i]));
                    }
                    painter.rect_filled(
                        Rect::from_min_max(Pos2::new(rects[0].right(), rect.top()), Pos2::new(rects[1].left(), rect.bottom())),
                        0.0,
                        Color32::from_gray(0x0c),
                    );
                    return;
                }
            }
            let o = self.view.origin(rect);
            self.draw_hover_pixel(&painter, o, egui::vec2(wa.max(wb), ha.max(hb)));
            return;
        }

        let Some((img, doc_path)) = self.doc.as_ref().map(|d| (d.layer.frame(self.anim.frame).clone(), d.dec.path.clone())) else {
            if self.error.is_none() && !self.current.as_ref().is_some_and(|p| self.loader.is_loading(p)) {
                self.draw_empty(&painter, rect);
            }
            return;
        };
        let frame = self.anim.frame;
        let size = egui::vec2(img.width() as f32, img.height() as f32);
        let o = self.view.origin(rect);
        let r = Rect::from_min_size(o, size * self.view.scale());
        // Subtle drop shadow lifts the image off the canvas.
        painter.add(egui::Shape::from(egui::epaint::Shadow {
            offset: [0, 6],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(90),
        }
        .as_shape(r, 0.0)));
        self.paint_bg(&ctx, &painter, r.intersect(rect));
        let loading = self.current.as_ref().is_some_and(|p| p != &doc_path);
        let tint = if loading { Color32::from_white_alpha(120) } else { Color32::WHITE };
        self.paint_layer(&ctx, &painter, Which::Doc, frame, o, tint);
        self.draw_grid(&painter, rect, o, size);
        self.draw_sheet(&painter, rect, o, size);
        self.draw_hover_pixel(&painter, o, size);
    }

    fn draw_grid(&self, painter: &Painter, rect: Rect, o: Pos2, size: Vec2) {
        let scale = self.view.scale();
        let phys = self.view.zoom;
        if !self.settings.grid || phys < 6.0 {
            return;
        }
        let light = self.settings.bg.is_light();
        // Fade the grid in between 6x and 10x so it never flickers on.
        let fade = ((phys - 6.0) / 4.0).clamp(0.0, 1.0);
        let base = if light { Color32::from_black_alpha((38.0 * fade) as u8) } else { Color32::from_white_alpha((26.0 * fade) as u8) };
        let major = if light { Color32::from_black_alpha((80.0 * fade) as u8) } else { Color32::from_white_alpha((58.0 * fade) as u8) };
        let w = 1.0 / self.view.ppp;
        let vis = rect.intersect(Rect::from_min_size(o, size * scale));
        if vis.is_negative() {
            return;
        }
        let x0 = ((vis.left() - o.x) / scale).floor().max(0.0) as i64;
        let x1 = ((vis.right() - o.x) / scale).ceil().min(size.x) as i64;
        let y0 = ((vis.top() - o.y) / scale).floor().max(0.0) as i64;
        let y1 = ((vis.bottom() - o.y) / scale).ceil().min(size.y) as i64;
        for x in x0..=x1 {
            let sx = o.x + x as f32 * scale;
            let c = if x % 8 == 0 { major } else { base };
            painter.line_segment([Pos2::new(sx, vis.top()), Pos2::new(sx, vis.bottom())], Stroke::new(w, c));
        }
        for y in y0..=y1 {
            let sy = o.y + y as f32 * scale;
            let c = if y % 8 == 0 { major } else { base };
            painter.line_segment([Pos2::new(vis.left(), sy), Pos2::new(vis.right(), sy)], Stroke::new(w, c));
        }
    }

    fn draw_hover_pixel(&self, painter: &Painter, o: Pos2, size: Vec2) {
        let Some(px) = self.hover_px else { return };
        if self.view.zoom < 8.0 || px.x < 0.0 || px.y < 0.0 || px.x >= size.x || px.y >= size.y {
            return;
        }
        let scale = self.view.scale();
        let r = Rect::from_min_size(o + egui::vec2(px.x.floor(), px.y.floor()) * scale, Vec2::splat(scale));
        painter.rect_stroke(r, 0.0, Stroke::new(1.5, ACCENT), egui::StrokeKind::Outside);
    }

    fn draw_sheet(&self, painter: &Painter, rect: Rect, o: Pos2, size: Vec2) {
        let Some(sheet) = &self.sheet else { return };
        let g = sheet.grid;
        let (w, h) = (size.x as u32, size.y as u32);
        let (cols, rows) = (g.columns(w), g.rows(h));
        if cols == 0 || rows == 0 {
            return;
        }
        let scale = self.view.scale();
        let cell_px = egui::vec2(g.cell[0] as f32, g.cell[1] as f32) * scale;
        let stroke = Stroke::new(1.0, SHEET.gamma_multiply(0.7));
        let hovered = self.hover_px.and_then(|p| g.cell_at(p.x, p.y, w, h));
        let show_idx = cell_px.x >= 26.0 && cell_px.y >= 18.0;
        let font = FontId::proportional((cell_px.y * 0.22).clamp(9.0, 13.0));
        for i in 0..cols * rows {
            let [x, y, cw, ch] = g.cell_rect(i, w);
            let r = Rect::from_min_size(o + egui::vec2(x as f32, y as f32) * scale, egui::vec2(cw as f32, ch as f32) * scale);
            if !r.intersects(rect) {
                continue;
            }
            if hovered == Some(i) {
                painter.rect_filled(r, 0.0, SHEET.gamma_multiply(0.14));
            }
            painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
            if show_idx {
                painter.text(r.left_top() + egui::vec2(3.0, 2.0), Align2::LEFT_TOP, i.to_string(), font.clone(), SHEET);
            }
        }
    }

    fn draw_empty(&self, painter: &Painter, rect: Rect) {
        let c = rect.center();
        painter.text(c - egui::vec2(0.0, 28.0), Align2::CENTER_CENTER, "Glint", FontId::proportional(44.0), TEXT.gamma_multiply(0.85));
        painter.text(
            c + egui::vec2(0.0, 18.0),
            Align2::CENTER_CENTER,
            "Drop an image here, or run  glint <file>",
            FontId::proportional(14.0),
            TEXT_DIM,
        );
        painter.text(
            c + egui::vec2(0.0, 42.0),
            Align2::CENTER_CENTER,
            "Ctrl+P  commands     F1  shortcuts",
            FontId::proportional(12.5),
            TEXT_DIM.gamma_multiply(0.7),
        );
    }
}

#[derive(Clone, Copy)]
enum Which {
    Doc,
    A,
    B,
    Diff,
}

fn label(painter: &Painter, pos: Pos2, align: Align2, text: &str) {
    let galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(12.5), TEXT);
    let r = align.anchor_size(pos, galley.size()).expand2(egui::vec2(8.0, 4.0));
    painter.rect_filled(r, 6.0, Color32::from_black_alpha(170));
    painter.galley(r.min + egui::vec2(8.0, 4.0), galley, TEXT);
}
