//! CPU frames plus their lazily uploaded GPU textures. Images larger than the
//! GPU's max texture size are split into tiles.

use std::sync::Arc;

use egui::{TextureFilter, TextureHandle, TextureOptions, TextureWrapMode};
use image::RgbaImage;

pub struct Tile {
    pub tex: TextureHandle,
    /// [x, y, w, h] in image pixels.
    pub rect: [u32; 4],
}

pub struct Layer {
    pub frames: Vec<Arc<RgbaImage>>,
    tex: Vec<Option<Vec<Tile>>>,
    smooth: bool,
}

impl Layer {
    pub fn new(frames: Vec<Arc<RgbaImage>>) -> Self {
        let n = frames.len();
        Self { frames, tex: (0..n).map(|_| None).collect(), smooth: false }
    }

    pub fn frame(&self, i: usize) -> &Arc<RgbaImage> {
        &self.frames[i.min(self.frames.len() - 1)]
    }

    pub fn replace_frame(&mut self, i: usize, img: Arc<RgbaImage>) {
        self.frames[i] = img;
        self.tex[i] = None;
    }

    pub fn tiles(&mut self, ctx: &egui::Context, i: usize, smooth: bool) -> &[Tile] {
        if smooth != self.smooth {
            self.smooth = smooth;
            self.tex.iter_mut().for_each(|t| *t = None);
        }
        let i = i.min(self.frames.len() - 1);
        if self.tex[i].is_none() {
            self.tex[i] = Some(upload(ctx, &self.frames[i], smooth, i));
        }
        self.tex[i].as_deref().unwrap()
    }
}

fn upload(ctx: &egui::Context, img: &RgbaImage, smooth: bool, frame: usize) -> Vec<Tile> {
    let opts = TextureOptions {
        // Crisp pixels when magnified (sprites, UI screenshots) unless smoothing is on;
        // mipmapped linear when shrinking so large screenshots don't shimmer.
        magnification: if smooth { TextureFilter::Linear } else { TextureFilter::Nearest },
        minification: TextureFilter::Linear,
        wrap_mode: TextureWrapMode::ClampToEdge,
        mipmap_mode: Some(TextureFilter::Linear),
    };
    let max = ctx.input(|i| i.max_texture_side).max(256) as u32;
    let (w, h) = img.dimensions();
    let mut tiles = Vec::new();
    for ty in (0..h).step_by(max as usize) {
        for tx in (0..w).step_by(max as usize) {
            let (tw, th) = ((w - tx).min(max), (h - ty).min(max));
            let color = if tw == w && th == h {
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw())
            } else {
                let sub = image::imageops::crop_imm(img, tx, ty, tw, th).to_image();
                egui::ColorImage::from_rgba_unmultiplied([tw as usize, th as usize], sub.as_raw())
            };
            let tex = ctx.load_texture(format!("glint-{frame}-{tx}-{ty}"), color, opts);
            tiles.push(Tile { tex, rect: [tx, ty, tw, th] });
        }
    }
    tiles
}
