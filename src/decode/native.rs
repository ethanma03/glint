//! Formats handled in-process by the `image` crate.

use std::io::Cursor;
use std::sync::Arc;

use image::codecs::{gif::GifDecoder, png::PngDecoder, webp::WebPDecoder};
use image::{AnimationDecoder, DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};

use super::{Decoded, Frame, MAX_PIXELS};

const MAX_FRAMES: usize = 5000;
/// Stop collecting animation frames once they'd take more than this much RAM.
const MAX_ANIM_BYTES: u64 = 3 << 30;

pub fn decode(data: &[u8], fmt: ImageFormat) -> Result<Decoded, String> {
    let name = format_name(fmt);
    match fmt {
        ImageFormat::Gif => {
            let dec = GifDecoder::new(Cursor::new(data)).map_err(msg)?;
            check_dims(dec.dimensions())?;
            return animated(name, "Indexed8", dec.into_frames());
        }
        ImageFormat::Png => {
            let dec = PngDecoder::new(Cursor::new(data)).map_err(msg)?;
            if dec.is_apng().map_err(msg)? {
                check_dims(dec.dimensions())?;
                let color = format!("{:?}", dec.color_type());
                return animated("APNG", color, dec.apng().map_err(msg)?.into_frames());
            }
        }
        ImageFormat::WebP => {
            let dec = WebPDecoder::new(Cursor::new(data)).map_err(msg)?;
            if dec.has_animation() {
                check_dims(dec.dimensions())?;
                return animated("WebP (animated)", "Rgba8", dec.into_frames());
            }
        }
        _ => {}
    }
    still(data, fmt, name)
}

fn still(data: &[u8], fmt: ImageFormat, name: &str) -> Result<Decoded, String> {
    let mut reader = ImageReader::with_format(Cursor::new(data), fmt);
    reader.limits(Limits::no_limits());
    let mut dec = reader.into_decoder().map_err(msg)?;
    check_dims(dec.dimensions())?;
    let orientation = dec.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(dec).map_err(msg)?;

    let mut out = Decoded::new(name, "native", format!("{:?}", img.color()));
    if orientation != image::metadata::Orientation::NoTransforms {
        img.apply_orientation(orientation);
        out.notes.push(format!("EXIF orientation applied ({orientation:?})."));
    }
    let image = match img {
        DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_) => {
            let hdr = img.to_rgba32f();
            let ldr = super::tonemap(&hdr, 0.0);
            out.hdr = Some(Arc::new(hdr));
            out.notes.push("HDR source: use E / Shift+E to adjust exposure.".into());
            ldr
        }
        _ => img.to_rgba8(),
    };
    out.frames.push(Frame { image: Arc::new(image), delay_ms: 0 });
    Ok(out)
}

fn animated(name: &str, color: impl Into<String>, frames: image::Frames<'_>) -> Result<Decoded, String> {
    let mut out = Decoded::new(name, "native", color);
    let mut bytes = 0u64;
    for frame in frames.take(MAX_FRAMES) {
        let frame = match frame {
            Ok(f) => f,
            // A truncated animation still shows the frames that decoded.
            Err(e) if !out.frames.is_empty() => {
                out.notes.push(format!("Animation truncated after {} frames: {e}", out.frames.len()));
                break;
            }
            Err(e) => return Err(msg(e)),
        };
        let (n, d) = frame.delay().numer_denom_ms();
        let mut delay = if d == 0 { 0 } else { n / d };
        // Browsers treat ≤10ms GIF delays as 100ms; match what artists expect to see.
        if delay <= 10 {
            delay = 100;
        }
        let image = frame.into_buffer();
        bytes += image.as_raw().len() as u64;
        out.frames.push(Frame { image: Arc::new(image), delay_ms: delay });
        if bytes > MAX_ANIM_BYTES {
            out.notes.push(format!("Only the first {} frames were loaded (memory limit).", out.frames.len()));
            break;
        }
    }
    if out.frames.len() == 1 {
        out.frames[0].delay_ms = 0;
    }
    Ok(out)
}

fn check_dims((w, h): (u32, u32)) -> Result<(), String> {
    if w == 0 || h == 0 {
        return Err(format!("Invalid image dimensions {w}×{h}."));
    }
    if w as u64 * h as u64 > MAX_PIXELS {
        return Err(format!("Image is too large to display ({w}×{h})."));
    }
    Ok(())
}

pub fn format_name(fmt: ImageFormat) -> &'static str {
    match fmt {
        ImageFormat::Png => "PNG",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::Gif => "GIF",
        ImageFormat::WebP => "WebP",
        ImageFormat::Pnm => "PNM",
        ImageFormat::Tiff => "TIFF",
        ImageFormat::Tga => "TGA",
        ImageFormat::Dds => "DDS",
        ImageFormat::Bmp => "BMP",
        ImageFormat::Ico => "ICO",
        ImageFormat::Hdr => "Radiance HDR",
        ImageFormat::OpenExr => "OpenEXR",
        ImageFormat::Farbfeld => "Farbfeld",
        ImageFormat::Avif => "AVIF",
        ImageFormat::Qoi => "QOI",
        _ => "Image",
    }
}

fn msg(e: image::ImageError) -> String {
    match e {
        image::ImageError::Unsupported(u) => format!("Unsupported variant: {u}"),
        image::ImageError::Limits(l) => format!("Too large: {l}"),
        other => {
            let s = other.to_string();
            if s.to_lowercase().contains("eof") || s.contains("end of file") {
                format!("The file appears truncated ({s}).")
            } else {
                s
            }
        }
    }
}
