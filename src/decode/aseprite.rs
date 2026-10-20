//! Aseprite (.ase/.aseprite): visible layers flattened per frame, plus tags.

use std::io::Cursor;
use std::sync::Arc;

use image::RgbaImage;

use super::{Decoded, Frame, Tag};

pub fn decode(data: &[u8]) -> Result<Decoded, String> {
    let ase = asefile::AsepriteFile::read(Cursor::new(data)).map_err(|e| format!("Invalid Aseprite file: {e}"))?;
    let color = if ase.is_indexed_color() { "Indexed8" } else { "Rgba8" };
    let mut out = Decoded::new("Aseprite", "aseprite", color);
    for i in 0..ase.num_frames() {
        let frame = ase.frame(i);
        // asefile builds against an older `image`; move the raw buffer across versions.
        let img = frame.image();
        let (w, h) = (img.width(), img.height());
        let image = RgbaImage::from_raw(w, h, img.into_raw()).ok_or("Bad Aseprite frame buffer.")?;
        out.frames.push(Frame { image: Arc::new(image), delay_ms: frame.duration() });
    }
    for i in 0..ase.num_tags() {
        let t = ase.tag(i);
        out.tags.push(Tag { name: t.name().to_string(), from: t.from_frame(), to: t.to_frame() });
    }
    out.notes.push(format!("{} layers, {} frames.", ase.num_layers(), ase.num_frames()));
    if out.frames.len() == 1 {
        out.frames[0].delay_ms = 0;
    }
    Ok(out)
}
