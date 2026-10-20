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

impl SheetGrid {
    /// A grid with `counts` = [columns, rows] cells spread over a `w`×`h` image.
    /// Returns the grid and the pixels left over on [right, bottom] when it doesn't divide evenly.
    pub fn from_counts(w: u32, h: u32, counts: [u32; 2], offset: [u32; 2], gap: [u32; 2]) -> Option<(Self, [u32; 2])> {
        let axis = |len: u32, n: u32, off: u32, gap: u32| {
            let n = n.max(1);
            let avail = len.checked_sub(off + gap * (n - 1))?;
            let cell = avail / n;
            (cell > 0).then_some((cell, avail - cell * n))
        };
        let (cw, rx) = axis(w, counts[0], offset[0], gap[0])?;
        let (ch, ry) = axis(h, counts[1], offset[1], gap[1])?;
        Some((Self { cell: [cw, ch], offset, gap }, [rx, ry]))
    }
}

/// Guess a sprite sheet's grid from the empty gutters between sprites.
///
/// "Empty" means fully transparent, or — for opaque sheets — the color of the
/// top-left pixel (magenta/solid backgrounds). Works per axis: find runs of
/// content, then pick the layout with the most cells where no sprite crosses a
/// cell boundary and every cell row/column has content. Returns None when no
/// regular grid fits (e.g. packed sheets whose sprites touch).
pub fn detect_grid(img: &RgbaImage) -> Option<SheetGrid> {
    let (w, h) = img.dimensions();
    if w < 2 || h < 2 {
        return None;
    }
    let has_alpha = img.pixels().any(|p| p.0[3] < 255);
    let bg = img.get_pixel(0, 0).0;
    let empty = |p: &Rgba<u8>| if has_alpha { p.0[3] == 0 } else { p.0 == bg };
    let mut col_has = vec![false; w as usize];
    let mut row_has = vec![false; h as usize];
    for (x, y, p) in img.enumerate_pixels() {
        if !empty(p) {
            col_has[x as usize] = true;
            row_has[y as usize] = true;
        }
    }
    let (cw, ox, gx, nx) = detect_axis(&col_has)?;
    let (ch, oy, gy, ny) = detect_axis(&row_has)?;
    if nx * ny < 2 {
        return None;
    }
    Some(SheetGrid { cell: [cw, ch], offset: [ox, oy], gap: [gx, gy] })
}

/// Returns (cell, offset, gap, count) for one axis.
fn detect_axis(content: &[bool]) -> Option<(u32, u32, u32, u32)> {
    let len = content.len() as u32;
    let mut runs = Vec::new();
    let mut start = None;
    for (i, &c) in content.iter().chain(std::iter::once(&false)).enumerate() {
        match (c, start) {
            (true, None) => start = Some(i as u32),
            (false, Some(s)) => {
                runs.push((s, i as u32 - 1));
                start = None;
            }
            _ => {}
        }
    }
    if runs.is_empty() {
        return None;
    }
    if runs.len() == 1 {
        // A single row/column of sprites: the whole axis is one cell.
        return Some((len, 0, 0, 1));
    }
    // Ranking: more cells first, then sprites best centered in their cells
    // (several offset/gap combos can contain inset sprites), then the simplest layout.
    let mut best: Option<((u32, i64, i64), (u32, u32, u32, u32))> = None;
    for gap in 0..=8u32 {
        for off in 0..=8u32 {
            for trail in [0, off] {
                for n in (2..=runs.len() as u32).rev() {
                    let Some(avail) = len.checked_sub(off + trail + gap * (n - 1)) else { continue };
                    if avail % n != 0 || avail / n < 2 {
                        continue;
                    }
                    let cell = avail / n;
                    let pitch = cell + gap;
                    let mut used = vec![false; n as usize];
                    let mut imbalance = 0i64;
                    let fits = runs.iter().all(|&(a, b)| {
                        if a < off {
                            return false;
                        }
                        let k = (a - off) / pitch;
                        let cell_start = off + k * pitch;
                        let ok = k < n && b < cell_start + cell;
                        if ok {
                            used[k as usize] = true;
                            let (left, right) = ((a - cell_start) as i64, (cell_start + cell - 1 - b) as i64);
                            imbalance += (left - right).abs();
                        }
                        ok
                    });
                    if fits && used.iter().all(|&u| u) {
                        let key = (n, -imbalance, -((off + gap) as i64));
                        if best.as_ref().is_none_or(|(k, _)| key > *k) {
                            best = Some((key, (cell, off, gap, n)));
                        }
                        break; // largest n for this (gap, off) found
                    }
                }
            }
        }
    }
    best.map(|(_, v)| v)
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

    #[test]
    fn grid_from_counts() {
        let (g, rest) = SheetGrid::from_counts(25, 25, [5, 5], [0, 0], [0, 0]).unwrap();
        assert_eq!((g.cell, rest), ([5, 5], [0, 0]));
        let (g, rest) = SheetGrid::from_counts(100, 50, [3, 2], [1, 1], [2, 2]).unwrap();
        assert_eq!((g.cell, rest), ([31, 23], [2, 1]));
    }

    /// Sheet of `cols`×`rows` cells of `cell` px, with a blob inset by `inset` in each cell.
    fn synth(cols: u32, rows: u32, cell: [u32; 2], off: u32, gap: u32, inset: u32) -> RgbaImage {
        let w = off * 2 + cols * cell[0] + (cols - 1) * gap;
        let h = off * 2 + rows * cell[1] + (rows - 1) * gap;
        let mut img = RgbaImage::new(w, h);
        for r in 0..rows {
            for c in 0..cols {
                let (x0, y0) = (off + c * (cell[0] + gap), off + r * (cell[1] + gap));
                for y in y0 + inset..y0 + cell[1] - inset {
                    for x in x0 + inset..x0 + cell[0] - inset {
                        img.put_pixel(x, y, Rgba([200, 50, 50, 255]));
                    }
                }
            }
        }
        img
    }

    #[test]
    fn detect_simple_and_odd_sizes() {
        let g = detect_grid(&synth(4, 2, [32, 32], 0, 0, 3)).unwrap();
        assert_eq!((g.cell, g.offset, g.gap), ([32, 32], [0, 0], [0, 0]));
        // Non-power-of-two, non-square cells.
        let g = detect_grid(&synth(5, 5, [5, 7], 0, 0, 1)).unwrap();
        assert_eq!(g.cell, [5, 7]);
        // Exporter-style padding and spacing.
        let g = detect_grid(&synth(6, 3, [24, 24], 2, 1, 2)).unwrap();
        assert_eq!((g.cell, g.offset, g.gap), ([24, 24], [2, 2], [1, 1]));
    }

    #[test]
    fn detect_opaque_background() {
        let mut img = synth(3, 2, [16, 16], 0, 0, 2);
        for p in img.pixels_mut() {
            if p.0[3] == 0 {
                p.0 = [255, 0, 255, 255];
            }
        }
        assert_eq!(detect_grid(&img).unwrap().cell, [16, 16]);
    }
}
