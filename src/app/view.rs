//! Zoom / pan state and canvas mouse input.
//!
//! `zoom` is measured in *physical* pixels per image pixel, so 100% means one
//! screen pixel per image pixel regardless of Windows display scaling, and the
//! image origin is snapped to the physical pixel grid to keep nearest-neighbour
//! sampling perfectly crisp.

use egui::{Event, MouseWheelUnit, PointerButton, Pos2, Rect, Response, Vec2};

use super::{CmpMode, GlintApp};

const STEPS: &[f32] = &[
    0.02, 0.03, 0.05, 0.0625, 0.083, 0.1, 0.125, 0.167, 0.2, 0.25, 0.333, 0.5, 0.667, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0,
    5.0, 6.0, 8.0, 10.0, 12.0, 16.0, 20.0, 24.0, 32.0, 40.0, 48.0, 64.0, 96.0, 128.0, 192.0, 256.0,
];
pub const MIN_ZOOM: f32 = 0.01;
pub const MAX_ZOOM: f32 = 256.0;

pub struct View {
    pub zoom: f32,
    pub target: f32,
    /// Image-space point shown at the center of the view rect.
    pub center: Vec2,
    pub fit: bool,
    /// Jump straight to the next fit instead of animating.
    pub snap_next: bool,
    /// (offset from view-rect center, image point) kept fixed while zoom animates.
    anchor: Option<(Vec2, Vec2)>,
    wheel_accum: f32,
    pub ppp: f32,
    /// Primary view rect from the last frame (half the window in side-by-side).
    pub rect: Rect,
    dragging_split: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            target: 1.0,
            center: Vec2::ZERO,
            fit: true,
            snap_next: true,
            anchor: None,
            wheel_accum: 0.0,
            ppp: 1.0,
            rect: Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0)),
            dragging_split: false,
        }
    }
}

impl View {
    /// Points per image pixel.
    pub fn scale(&self) -> f32 {
        self.zoom / self.ppp
    }

    /// Screen position of image pixel (0,0) inside `r`, snapped to physical pixels.
    pub fn origin(&self, r: Rect) -> Pos2 {
        let o = r.center() - self.center * self.scale();
        Pos2::new((o.x * self.ppp).round() / self.ppp, (o.y * self.ppp).round() / self.ppp)
    }

    pub fn to_image(&self, r: Rect, p: Pos2) -> Vec2 {
        (p - self.origin(r)) / self.scale()
    }

    pub fn fit_zoom(content: Vec2, r: Rect, ppp: f32) -> f32 {
        let avail = r.size() * ppp;
        let z = (avail.x / content.x.max(1.0)).min(avail.y / content.y.max(1.0));
        if z >= 2.0 {
            // Small sprites: largest integer scale that fits, so pixels stay square.
            z.floor().min(64.0)
        } else if z >= 1.0 {
            1.0
        } else {
            z.max(MIN_ZOOM)
        }
    }

    /// Zoom to `z`, keeping the image point under `anchor` (screen) fixed.
    pub fn zoom_to(&mut self, z: f32, anchor: Pos2, r: Rect, animate: bool) {
        let z = z.clamp(MIN_ZOOM, MAX_ZOOM);
        let img = self.to_image(r, anchor);
        self.fit = false;
        self.target = z;
        self.anchor = Some((anchor - r.center(), img));
        if !animate {
            self.zoom = z;
            self.apply_anchor();
        }
    }

    pub fn step_zoom(&self, dir: i32) -> f32 {
        let z = self.target;
        if dir > 0 {
            STEPS.iter().copied().find(|&s| s > z * 1.001).unwrap_or(MAX_ZOOM)
        } else {
            STEPS.iter().rev().copied().find(|&s| s < z * 0.999).unwrap_or(MIN_ZOOM)
        }
    }

    fn apply_anchor(&mut self) {
        if let Some((off, img)) = self.anchor {
            self.center = img - off / self.scale();
        }
    }

    /// Advance the zoom animation / refit. Returns true while animating.
    pub fn update(&mut self, dt: f32, content: Vec2, r: Rect) -> bool {
        if self.fit {
            let z = Self::fit_zoom(content, r, self.ppp);
            self.target = z;
            self.center = content / 2.0;
            self.anchor = None;
            if self.snap_next {
                self.zoom = z;
            }
        }
        self.snap_next = false;
        if (self.zoom - self.target).abs() < 1e-6 {
            self.zoom = self.target;
            return false;
        }
        let (lz, lt) = (self.zoom.ln(), self.target.ln());
        let k = 1.0 - (-dt * 22.0).exp();
        let mut nz = lz + (lt - lz) * k;
        if (lt - nz).abs() < 0.003 {
            nz = lt;
        }
        self.zoom = nz.exp();
        if self.fit {
            self.center = content / 2.0;
        } else {
            self.apply_anchor();
        }
        true
    }

    pub fn pan_by(&mut self, screen_delta: Vec2) {
        self.fit = false;
        self.anchor = None;
        self.target = self.zoom;
        self.center -= screen_delta / self.scale();
    }

    pub fn center_on(&mut self, img: Vec2) {
        self.fit = false;
        self.anchor = None;
        self.target = self.zoom;
        self.center = img;
    }
}

impl GlintApp {
    /// One rect normally; two in side-by-side compare.
    pub fn layout(&self, rect: Rect) -> Vec<Rect> {
        match &self.compare {
            Some(c) if c.mode == CmpMode::Side => {
                let gap = 6.0;
                let w = (rect.width() - gap) / 2.0;
                vec![
                    Rect::from_min_size(rect.min, egui::vec2(w, rect.height())),
                    Rect::from_min_size(rect.min + egui::vec2(w + gap, 0.0), egui::vec2(w, rect.height())),
                ]
            }
            _ => vec![rect],
        }
    }

    pub fn handle_canvas_input(&mut self, ctx: &egui::Context, resp: &Response, rect: Rect) {
        self.view.ppp = ctx.pixels_per_point();
        let rects = self.layout(rect);
        self.view.rect = rects[0];
        let Some(content) = self.content_size() else {
            self.hover_px = None;
            return;
        };

        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let sub = pointer.and_then(|p| rects.iter().copied().find(|r| r.contains(p))).unwrap_or(rects[0]);
        self.hover_px = pointer.filter(|_| resp.hovered() || resp.dragged()).map(|p| self.view.to_image(sub, p));

        if resp.hovered() {
            let events = ctx.input(|i| i.events.clone());
            for ev in events {
                match ev {
                    Event::MouseWheel { unit, delta, modifiers, .. } => {
                        let notches = match unit {
                            MouseWheelUnit::Line => delta.y,
                            MouseWheelUnit::Point => delta.y / 50.0,
                            MouseWheelUnit::Page => delta.y * 4.0,
                        };
                        if modifiers.alt {
                            if let Some(c) = &mut self.compare {
                                c.split = (c.split + notches * 0.05).clamp(0.0, 1.0);
                            }
                            continue;
                        }
                        self.view.wheel_accum += notches;
                        let anchor = pointer.unwrap_or(sub.center());
                        while self.view.wheel_accum >= 1.0 {
                            self.view.wheel_accum -= 1.0;
                            let z = self.view.step_zoom(1);
                            self.view.zoom_to(z, anchor, sub, true);
                        }
                        while self.view.wheel_accum <= -1.0 {
                            self.view.wheel_accum += 1.0;
                            let z = self.view.step_zoom(-1);
                            self.view.zoom_to(z, anchor, sub, true);
                        }
                    }
                    Event::Zoom(f) => {
                        let anchor = pointer.unwrap_or(sub.center());
                        let z = self.view.zoom * f;
                        self.view.zoom_to(z, anchor, sub, false);
                    }
                    _ => {}
                }
            }
        }

        // Swipe compare: dragging near the divider moves it instead of panning.
        let swipe = self.compare.as_ref().is_some_and(|c| c.mode == CmpMode::Swipe);
        if resp.drag_started_by(PointerButton::Primary) && swipe {
            let split_x = rect.left() + self.compare.as_ref().unwrap().split * rect.width();
            self.view.dragging_split = pointer.is_some_and(|p| (p.x - split_x).abs() < 14.0);
        }
        if resp.drag_stopped() {
            self.view.dragging_split = false;
        }
        if resp.dragged() {
            if self.view.dragging_split {
                if let (Some(c), Some(p)) = (&mut self.compare, pointer) {
                    c.split = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                }
            } else {
                self.view.pan_by(resp.drag_delta());
            }
        }

        if resp.double_clicked() {
            if self.view.fit || (self.view.zoom - View::fit_zoom(content, sub, self.view.ppp)).abs() < 1e-3 {
                let fz = View::fit_zoom(content, sub, self.view.ppp);
                // From fit, jump to 100% — or for sprites already ≥100%, double the zoom.
                let z = if fz >= 1.0 { (fz * 2.0).min(MAX_ZOOM) } else { 1.0 };
                self.view.zoom_to(z, pointer.unwrap_or(sub.center()), sub, true);
            } else {
                self.view.fit = true;
            }
        }

        let dt = ctx.input(|i| i.stable_dt).min(0.05);
        self.view_animating = self.view.update(dt, content, sub);
        if self.view_animating {
            ctx.request_repaint();
        }
        let cursor = if self.view.dragging_split || (swipe && pointer.is_some_and(|p| {
            let split_x = rect.left() + self.compare.as_ref().unwrap().split * rect.width();
            (p.x - split_x).abs() < 14.0
        })) {
            egui::CursorIcon::ResizeHorizontal
        } else if resp.dragged() {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::Default
        };
        if resp.hovered() || resp.dragged() {
            ctx.set_cursor_icon(cursor);
        }
    }
}
