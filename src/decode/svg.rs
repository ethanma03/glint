//! SVG / SVGZ via resvg. Rasterized large enough to stay crisp when zoomed.

use std::sync::Arc;

use image::RgbaImage;
use resvg::{tiny_skia, usvg};

use super::{Decoded, Frame};

/// Small icons get rasterized up to this long side; big drawings keep their size.
const MIN_LONG_SIDE: f32 = 2048.0;
const MAX_LONG_SIDE: f32 = 8192.0;

pub fn decode(data: &[u8]) -> Result<Decoded, String> {
    let mut opt = usvg::Options::default();
    // Loading system fonts costs ~100ms, so only do it when there is text to shape.
    if memchr_text(data) {
        opt.fontdb_mut().load_system_fonts();
    }
    let tree = usvg::Tree::from_data(data, &opt).map_err(|e| format!("Invalid SVG: {e}"))?;
    let size = tree.size();
    let (vw, vh) = (size.width(), size.height());
    let long = vw.max(vh);
    if !(long > 0.0) {
        return Err("SVG has zero size.".into());
    }
    let scale = (MIN_LONG_SIDE / long).max(1.0).min(MAX_LONG_SIDE / long);
    let (w, h) = (((vw * scale).ceil() as u32).max(1), ((vh * scale).ceil() as u32).max(1));
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("Couldn't allocate SVG canvas.")?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());

    let mut img = RgbaImage::new(w, h);
    for (src, dst) in pixmap.pixels().iter().zip(img.pixels_mut()) {
        let c = src.demultiply();
        dst.0 = [c.red(), c.green(), c.blue(), c.alpha()];
    }
    let mut out = Decoded::new("SVG", "svg", "vector");
    out.notes.push(format!(
        "Vector image {}×{}; rasterized at {w}×{h}.",
        vw.round() as u32,
        vh.round() as u32
    ));
    out.frames.push(Frame { image: Arc::new(img), delay_ms: 0 });
    Ok(out)
}

fn memchr_text(data: &[u8]) -> bool {
    data.windows(5).any(|w| w == b"<text")
}
