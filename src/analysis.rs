//! Pixel statistics shared by the CLI and the viewer: image info, diffs, sprite sheets.

use std::collections::HashSet;

use image::{Rgba, RgbaImage};
use serde::Serialize;

#[derive(Serialize, Debug, Clone, Default)]
pub struct Stats {
    pub unique_colors: usize,
    /// Pixels with alpha < 255.
    pub transparent_pixels: u64,
    /// Pixels with 0 < alpha < 255 (anti-aliasing, soft shadows).
    pub semi_transparent_pixels: u64,
    /// Bounding box of pixels with alpha > 0: [x, y, w, h].
    pub opaque_bbox: Option<[u32; 4]>,
}

pub fn stats(img: &RgbaImage) -> Stats {
    let mut colors = HashSet::with_capacity(1024);
    let (mut transparent, mut semi) = (0u64, 0u64);
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for (x, y, p) in img.enumerate_pixels() {
        colors.insert(u32::from_ne_bytes(p.0));
        let a = p.0[3];
        if a < 255 {
            transparent += 1;
            if a > 0 {
                semi += 1;
            }
        }
        if a > 0 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    Stats {
        unique_colors: colors.len(),
        transparent_pixels: transparent,
        semi_transparent_pixels: semi,
        opaque_bbox: (x0 != u32::MAX).then(|| [x0, y0, x1 - x0 + 1, y1 - y0 + 1]),
    }
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct DiffMetrics {
    pub a_size: [u32; 2],
    pub b_size: [u32; 2],
    pub size_mismatch: bool,
    pub identical: bool,
    /// Number of pixels whose RGBA differs at all (ImageMagick's AE with fuzz 0).
    pub differing_pixels: u64,
    pub total_pixels: u64,
    pub differing_percent: f64,
    pub max_channel_diff: u8,
    /// Root-mean-square error over RGBA channels, 0-255 scale.
    pub rmse: f64,
    /// Peak signal-to-noise ratio in dB (infinite when identical, reported as null).
    pub psnr_db: Option<f64>,
    /// Bounding box of differing pixels: [x, y, w, h].
    pub diff_bbox: Option<[u32; 4]>,
}

/// Compare two images aligned at the top-left. Pixels present in only one image count as different.
pub fn diff(a: &RgbaImage, b: &RgbaImage) -> DiffMetrics {
    let (w, h) = (a.width().max(b.width()), a.height().max(b.height()));
    let clear = Rgba([0, 0, 0, 0]);
    let get = |img: &RgbaImage, x: u32, y: u32| {
        if x < img.width() && y < img.height() { *img.get_pixel(x, y) } else { clear }
    };
    let (mut n, mut maxd, mut sq) = (0u64, 0u8, 0f64);
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for y in 0..h {
        for x in 0..w {
            let inside = x < a.width() && y < a.height() && x < b.width() && y < b.height();
            let (pa, pb) = (get(a, x, y), get(b, x, y));
            let mut px_diff = !inside;
            for c in 0..4 {
                let d = pa.0[c].abs_diff(pb.0[c]);
                if d > 0 {
                    px_diff = true;
                }
                maxd = maxd.max(d);
                sq += (d as f64) * (d as f64);
            }
            if px_diff {
                n += 1;
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    let total = w as u64 * h as u64;
    let mse = sq / (total.max(1) as f64 * 4.0);
    DiffMetrics {
        a_size: [a.width(), a.height()],
        b_size: [b.width(), b.height()],
        size_mismatch: a.dimensions() != b.dimensions(),
        identical: n == 0,
        differing_pixels: n,
        total_pixels: total,
        differing_percent: if total == 0 { 0.0 } else { n as f64 * 100.0 / total as f64 },
        max_channel_diff: maxd,
        rmse: mse.sqrt(),
        psnr_db: (mse > 0.0).then(|| 10.0 * (255.0f64 * 255.0 / mse).log10()),
        diff_bbox: (x0 != u32::MAX).then(|| [x0, y0, x1 - x0 + 1, y1 - y0 + 1]),
    }
}

/// Visualize differences: unchanged pixels as a dim grayscale of A, changed pixels
/// in hot pink scaled by magnitude (never fainter than 45% so 1-value changes stay visible).
pub fn diff_image(a: &RgbaImage, b: &RgbaImage) -> RgbaImage {
    let (w, h) = (a.width().max(b.width()), a.height().max(b.height()));
    RgbaImage::from_fn(w, h, |x, y| {
        let pa = (x < a.width() && y < a.height()).then(|| a.get_pixel(x, y).0);
        let pb = (x < b.width() && y < b.height()).then(|| b.get_pixel(x, y).0);
        match (pa, pb) {
            (Some(pa), Some(pb)) => {
                let d = (0..4).map(|c| pa[c].abs_diff(pb[c])).max().unwrap_or(0);
                if d == 0 {
                    let alpha = pa[3] as f32 / 255.0;
                    let l = (0.299 * pa[0] as f32 + 0.587 * pa[1] as f32 + 0.114 * pa[2] as f32) * alpha;
                    let g = (l * 0.30 + 18.0) as u8;
                    Rgba([g, g, g, 255])
                } else {
                    let t = 0.45 + 0.55 * (d as f32 / 255.0);
                    Rgba([(255.0 * t) as u8, (40.0 * t) as u8, (110.0 * t) as u8, 255])
                }
            }
            // Only in one image: amber.
            _ => Rgba([255, 176, 32, 255]),
        }
    })
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct SheetGrid {
    pub cell: [u32; 2],
    pub offset: [u32; 2],
    pub gap: [u32; 2],
}

impl SheetGrid {
    pub fn columns(&self, w: u32) -> u32 {
        let step = self.cell[0] + self.gap[0];
        if w < self.offset[0] + self.cell[0] { 0 } else { (w - self.offset[0] + self.gap[0]) / step }
    }
    pub fn rows(&self, h: u32) -> u32 {
        let step = self.cell[1] + self.gap[1];
        if h < self.offset[1] + self.cell[1] { 0 } else { (h - self.offset[1] + self.gap[1]) / step }
    }
    /// [x, y, w, h] of cell `i` in row-major order.
    pub fn cell_rect(&self, i: u32, img_w: u32) -> [u32; 4] {
        let cols = self.columns(img_w).max(1);
        let (c, r) = (i % cols, i / cols);
        [
            self.offset[0] + c * (self.cell[0] + self.gap[0]),
            self.offset[1] + r * (self.cell[1] + self.gap[1]),
            self.cell[0],
            self.cell[1],
        ]
    }
    pub fn cell_at(&self, x: f32, y: f32, w: u32, h: u32) -> Option<u32> {
        let (cols, rows) = (self.columns(w), self.rows(h));
        let (fx, fy) = (x - self.offset[0] as f32, y - self.offset[1] as f32);
        if fx < 0.0 || fy < 0.0 {
            return None;
        }
        let (c, r) = ((fx as u32) / (self.cell[0] + self.gap[0]), (fy as u32) / (self.cell[1] + self.gap[1]));
        let (ix, iy) = (fx as u32 % (self.cell[0] + self.gap[0]), fy as u32 % (self.cell[1] + self.gap[1]));
        (c < cols && r < rows && ix < self.cell[0] && iy < self.cell[1]).then_some(r * cols + c)
    }
}

/// Parse "32x32" / "32" / "16x24".
pub fn parse_size(s: &str) -> Option<[u32; 2]> {
    let s = s.trim().to_ascii_lowercase();
    let (a, b) = s.split_once(['x', '×', '*']).unwrap_or((&s, &s));
    let (a, b) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
    (a > 0 && b > 0).then_some([a, b])
}

/// Parse "4,8" / "4x8" / "4".
pub fn parse_pair(s: &str) -> Option<[u32; 2]> {
    let s = s.trim().to_ascii_lowercase();
    let (a, b) = s.split_once([',', 'x']).unwrap_or((&s, &s));
    Some([a.trim().parse().ok()?, b.trim().parse().ok()?])
}

pub fn hex(p: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}{:02x}", p[0], p[1], p[2], p[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_counts_and_bbox() {
        let a = RgbaImage::from_pixel(8, 8, Rgba([10, 20, 30, 255]));
        let mut b = a.clone();
        b.put_pixel(2, 3, Rgba([11, 20, 30, 255]));
        b.put_pixel(5, 6, Rgba([10, 20, 30, 0]));
        let m = diff(&a, &b);
        assert_eq!(m.differing_pixels, 2);
        assert_eq!(m.diff_bbox, Some([2, 3, 4, 4]));
        assert_eq!(m.max_channel_diff, 255);
        assert!(diff(&a, &a).identical);
    }

    #[test]
    fn sheet_cells() {
        let g = SheetGrid { cell: [16, 16], offset: [1, 1], gap: [2, 2] };
        assert_eq!(g.columns(1 + 16 * 4 + 2 * 3), 4);
        assert_eq!(g.cell_rect(5, 1 + 16 * 4 + 2 * 3), [19, 19, 16, 16]);
        assert_eq!(g.cell_at(20.0, 20.0, 71, 71), Some(5));
        assert_eq!(g.cell_at(17.5, 5.0, 71, 71), None); // in the gap
        assert_eq!(parse_size("32x16"), Some([32, 16]));
        assert_eq!(parse_size("8"), Some([8, 8]));
    }
}
