//! Image decoding. Formats are detected by content (magic bytes), never trusted
//! from the extension alone, and every decoder runs behind `catch_unwind` so a
//! hostile or broken file produces an error card instead of a crash.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::{ImageFormat, RgbaImage};

pub mod aseprite;
pub mod magick;
pub mod native;
pub mod svg;

/// Refuse anything bigger than this many pixels per frame (~1 gigapixel, 4 GB RGBA).
pub const MAX_PIXELS: u64 = 1 << 30;

#[derive(Clone)]
pub struct Frame {
    pub image: Arc<RgbaImage>,
    pub delay_ms: u32,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Tag {
    pub name: String,
    pub from: u32,
    pub to: u32,
}

#[derive(Clone)]
pub struct Decoded {
    pub path: PathBuf,
    /// Human-readable container format, e.g. "PNG", "Aseprite", "HEIC".
    pub format: String,
    /// Which backend produced the pixels.
    pub decoder: &'static str,
    /// Source pixel layout, e.g. "Rgba8", "Rgb16", "Rgba32F".
    pub color: String,
    pub frames: Vec<Frame>,
    pub tags: Vec<Tag>,
    /// Linear float pixels for HDR sources, so exposure can be re-applied.
    pub hdr: Option<Arc<image::Rgba32FImage>>,
    pub file_size: u64,
    pub notes: Vec<String>,
    /// SVG source and its intrinsic size, so the viewer can re-rasterize when zooming in.
    pub vector: Option<(Arc<Vec<u8>>, [f32; 2])>,
}

impl Decoded {
    pub fn new(format: impl Into<String>, decoder: &'static str, color: impl Into<String>) -> Self {
        Self {
            path: PathBuf::new(),
            format: format.into(),
            decoder,
            color: color.into(),
            frames: Vec::new(),
            tags: Vec::new(),
            hdr: None,
            file_size: 0,
            notes: Vec::new(),
            vector: None,
        }
    }

    pub fn width(&self) -> u32 {
        self.frames.first().map_or(0, |f| f.image.width())
    }

    pub fn height(&self) -> u32 {
        self.frames.first().map_or(0, |f| f.image.height())
    }

    /// Multiple frames with timing; multi-page files (TIFF pages, ICO sizes) have zero delays.
    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1 && self.frames.iter().any(|f| f.delay_ms > 0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Native(ImageFormat),
    Svg,
    Aseprite,
    /// Handed to ImageMagick; the label is the detected format.
    Magick(&'static str),
}

const RAW_EXTS: &[&str] = &[
    "dng", "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "orf", "rw2", "raf", "pef", "srw",
    "x3f", "3fr", "erf", "kdc", "mos", "mrw", "raw", "iiq",
];

pub fn ext_lower(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Detect the format from the first few KB of the file, falling back to the
/// extension only for formats that have no magic number (TGA, camera RAW).
pub fn sniff(head: &[u8], path: &Path) -> Kind {
    let ext = ext_lower(path);
    let starts = |m: &[u8]| head.starts_with(m);
    let at = |off: usize, m: &[u8]| head.len() >= off + m.len() && &head[off..off + m.len()] == m;

    // Camera RAW files are TIFF containers whose first IFD is a thumbnail; only libraw gets them right.
    if RAW_EXTS.contains(&ext.as_str()) {
        return Kind::Magick("Camera RAW");
    }
    if starts(b"\x89PNG\r\n\x1a\n") {
        return Kind::Native(ImageFormat::Png);
    }
    if starts(&[0xFF, 0xD8, 0xFF]) {
        return Kind::Native(ImageFormat::Jpeg);
    }
    if starts(b"GIF87a") || starts(b"GIF89a") {
        return Kind::Native(ImageFormat::Gif);
    }
    if starts(b"RIFF") && at(8, b"WEBP") {
        return Kind::Native(ImageFormat::WebP);
    }
    if starts(b"qoif") {
        return Kind::Native(ImageFormat::Qoi);
    }
    if starts(b"DDS ") {
        return Kind::Native(ImageFormat::Dds);
    }
    if starts(&[0x76, 0x2F, 0x31, 0x01]) {
        return Kind::Native(ImageFormat::OpenExr);
    }
    if starts(b"#?RADIANCE") || starts(b"#?RGBE") {
        return Kind::Native(ImageFormat::Hdr);
    }
    if starts(b"farbfeld") {
        return Kind::Native(ImageFormat::Farbfeld);
    }
    if starts(b"8BPS") {
        return Kind::Magick("PSD");
    }
    if starts(b"gimp xcf") {
        return Kind::Magick("XCF");
    }
    if starts(&[0xFF, 0x0A]) || starts(&[0, 0, 0, 0x0C, b'J', b'X', b'L', b' ']) {
        return Kind::Magick("JPEG XL");
    }
    if starts(&[0, 0, 0, 0x0C, b'j', b'P', b' ', b' ']) || starts(&[0xFF, 0x4F, 0xFF, 0x51]) {
        return Kind::Magick("JPEG 2000");
    }
    if at(4, b"ftyp") && head.len() >= 12 {
        let brand = &head[8..12];
        return match brand {
            b"avif" | b"avis" => Kind::Magick("AVIF"),
            _ => {
                // Scan compatible brands too: some AVIFs use 'mif1' as the major brand.
                let box_len = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
                let brands = &head[8..box_len.min(head.len())];
                if brands.chunks(4).any(|b| b == b"avif" || b == b"avis") {
                    Kind::Magick("AVIF")
                } else {
                    Kind::Magick("HEIC")
                }
            }
        };
    }
    if starts(b"II*\0") || starts(b"MM\0*") {
        return Kind::Native(ImageFormat::Tiff);
    }
    if starts(b"BM") {
        return Kind::Native(ImageFormat::Bmp);
    }
    // TGA has no magic number and its header can look like ICO/CUR, so trust the extension first.
    if matches!(ext.as_str(), "tga" | "icb" | "vda" | "vst") {
        return Kind::Native(ImageFormat::Tga);
    }
    if (starts(&[0, 0, 1, 0]) || starts(&[0, 0, 2, 0])) && head.len() >= 6 && head[4] | head[5] != 0 {
        return Kind::Native(ImageFormat::Ico);
    }
    if head.len() >= 6 && head[4] == 0xE0 && head[5] == 0xA5 {
        return Kind::Aseprite;
    }
    if head.len() >= 2 && head[0] == b'P' && (b'1'..=b'7').contains(&head[1]) {
        return Kind::Native(ImageFormat::Pnm);
    }
    if starts(&[0x1F, 0x8B]) && ext == "svgz" {
        return Kind::Svg;
    }
    if looks_like_svg(head) {
        return Kind::Svg;
    }
    match ext.as_str() {
        "ase" | "aseprite" => Kind::Aseprite,
        _ => Kind::Magick("unknown"),
    }
}

fn looks_like_svg(head: &[u8]) -> bool {
    let text = String::from_utf8_lossy(&head[..head.len().min(4096)]);
    let t = text.trim_start_matches('\u{feff}').trim_start();
    (t.starts_with('<') && t.contains("<svg")) || t.starts_with("<svg")
}

/// Decode a file into display-ready RGBA frames.
pub fn decode(path: &Path) -> Result<Decoded, String> {
    let meta = std::fs::metadata(path).map_err(|e| io_msg(&e))?;
    if meta.is_dir() {
        return Err("This is a folder, not an image.".into());
    }
    if meta.len() == 0 {
        return Err("Empty file (0 bytes).".into());
    }
    let data = std::fs::read(path).map_err(|e| io_msg(&e))?;
    let kind = sniff(&data[..data.len().min(4096)], path);

    let result = std::panic::catch_unwind(|| decode_kind(&data, path, kind))
        .unwrap_or_else(|_| Err("The decoder crashed on this file (it is probably corrupt).".into()));

    let mut dec = match (result, kind) {
        (Ok(d), _) => d,
        // Anything native can't handle (CMYK JPEG, exotic TIFF, ...) gets a second chance.
        (Err(e), Kind::Native(_) | Kind::Aseprite | Kind::Svg) if magick::available() => {
            match magick::decode(path, None) {
                Ok(mut d) => {
                    d.notes.push(format!("Built-in decoder failed ({e}); decoded with ImageMagick."));
                    d
                }
                Err(_) => return Err(e),
            }
        }
        (Err(e), _) => return Err(e),
    };

    // The built-in TIFF decoder only reads the first page; hand multi-page files to magick.
    if kind == Kind::Native(ImageFormat::Tiff) && dec.decoder == "native" && magick::available() {
        let pages = tiff_page_count(&data);
        if pages > 1 {
            if let Ok(mut d) = magick::decode(path, Some("TIFF")) {
                d.notes.push(format!("{pages} pages."));
                dec = d;
            }
        }
    }

    let ext = ext_lower(path);
    if let Kind::Native(fmt) = kind {
        if !ext.is_empty() && !fmt.extensions_str().contains(&ext.as_str()) && !["apng", "cur", "jfif", "jpe", "dib"].contains(&ext.as_str())
        {
            dec.notes.push(format!("Extension .{ext} doesn't match the actual content ({}).", dec.format));
        }
    }
    dec.path = path.to_path_buf();
    dec.file_size = meta.len();
    if dec.frames.is_empty() {
        return Err("The file contains no image frames.".into());
    }
    Ok(dec)
}

fn decode_kind(data: &[u8], path: &Path, kind: Kind) -> Result<Decoded, String> {
    match kind {
        Kind::Native(fmt) => native::decode(data, fmt),
        Kind::Svg => svg::decode(data),
        Kind::Aseprite => aseprite::decode(data),
        Kind::Magick(label) => {
            if !magick::available() {
                return Err(format!(
                    "{} files need ImageMagick, which wasn't found on PATH. Install it with: winget install ImageMagick.Q16-HDRI",
                    if label == "unknown" { "These" } else { label }
                ));
            }
            magick::decode(path, Some(label))
        }
    }
}

/// Count pages (IFDs) in a classic TIFF without decoding pixels.
pub fn tiff_page_count(data: &[u8]) -> u32 {
    let le = match data.get(..4) {
        Some(b"II* ") => true,
        Some(b"MM *") => false,
        _ => return 0,
    };
    let u16_at = |o: usize| data.get(o..o + 2).map(|b| if le { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) });
    let u32_at = |o: usize| {
        data.get(o..o + 4)
            .map(|b| if le { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) })
    };
    let mut pages = 0;
    let mut off = u32_at(4).unwrap_or(0) as usize;
    let mut seen = std::collections::HashSet::new();
    while off != 0 && pages < 10_000 && seen.insert(off) {
        let Some(n) = u16_at(off) else { break };
        pages += 1;
        off = u32_at(off + 2 + n as usize * 12).unwrap_or(0) as usize;
    }
    pages
}

fn io_msg(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "File not found.".into(),
        std::io::ErrorKind::PermissionDenied => "Access denied (the file may be locked by another program).".into(),
        _ => format!("Couldn't read the file: {e}"),
    }
}

/// Map linear float pixels to sRGB 8-bit with an exposure adjustment in stops.
pub fn tonemap(hdr: &image::Rgba32FImage, exposure: f32) -> RgbaImage {
    const N: usize = 4096;
    let lut: Vec<u8> = (0..N)
        .map(|i| {
            let v = i as f32 / (N - 1) as f32;
            let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
            (s * 255.0 + 0.5) as u8
        })
        .collect();
    let gain = 2f32.powf(exposure);
    let (w, h) = hdr.dimensions();
    let mut out = RgbaImage::new(w, h);
    for (src, dst) in hdr.pixels().zip(out.pixels_mut()) {
        let m = |c: f32| lut[((c * gain).clamp(0.0, 1.0) * (N - 1) as f32) as usize];
        dst.0 = [m(src.0[0]), m(src.0[1]), m(src.0[2]), (src.0[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8];
    }
    out
}

/// Extensions Glint registers for and lists when browsing a folder.
pub const EXTENSIONS: &[&str] = &[
    "png", "apng", "jpg", "jpeg", "jpe", "jfif", "gif", "webp", "bmp", "dib", "ico", "cur", "tga", "dds",
    "qoi", "tif", "tiff", "exr", "hdr", "pbm", "pgm", "ppm", "pnm", "pam", "ff", "svg", "svgz", "ase",
    "aseprite", "avif", "heic", "heif", "jxl", "psd", "psb", "xcf", "jp2", "j2k", "pcx", "dng", "cr2",
    "cr3", "nef", "arw", "orf", "rw2", "raf",
];

pub fn is_image_path(path: &Path) -> bool {
    EXTENSIONS.contains(&ext_lower(path).as_str())
}
