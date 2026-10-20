//! ImageMagick fallback for everything the built-in decoders don't cover
//! (HEIC, AVIF, JPEG XL, PSD, camera RAW, CMYK JPEG, ...).
//!
//! The file is streamed through stdin rather than passed by path, because
//! magick interprets `[...]` in filenames as frame selectors.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, OnceLock};

use image::RgbaImage;

use super::{Decoded, Frame, ext_lower};

const MAX_PAGES: u32 = 256;

pub fn exe() -> Option<&'static Path> {
    static EXE: OnceLock<Option<PathBuf>> = OnceLock::new();
    EXE.get_or_init(find_exe).as_deref()
}

pub fn available() -> bool {
    exe().is_some()
}

fn find_exe() -> Option<PathBuf> {
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let p = dir.join("magick.exe");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let pf = std::env::var_os("ProgramFiles").map(PathBuf::from)?;
    std::fs::read_dir(&pf)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("ImageMagick"))
        .map(|e| e.path().join("magick.exe"))
        .find(|p| p.is_file())
}

/// A `magick` command with no console window popping up from the GUI.
pub fn command() -> Option<Command> {
    let mut cmd = Command::new(exe()?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    Some(cmd)
}

/// Formats whose content magick can't sniff from stdin need an explicit coder prefix.
fn coder_prefix(path: &Path) -> &'static str {
    match ext_lower(path).as_str() {
        "tga" => "tga:",
        "dng" => "dng:",
        "cr2" => "cr2:",
        "cr3" => "cr3:",
        "nef" => "nef:",
        "arw" => "arw:",
        "orf" => "orf:",
        "rw2" => "rw2:",
        "raf" => "raf:",
        "pef" => "pef:",
        "srw" => "srw:",
        "pcx" => "pcx:",
        "dds" => "dds:",
        "ico" => "ico:",
        _ => "",
    }
}

pub fn decode(path: &Path, label: Option<&'static str>) -> Result<Decoded, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    // PSD/PSB page 0 is the flattened composite; the rest are individual layers.
    let pages = match label {
        Some("PSD") | Some("Camera RAW") => "[0]".to_string(),
        _ => format!("[0-{}]", MAX_PAGES - 1),
    };
    let input = format!("{}-{pages}", coder_prefix(path));
    let mut child = command()
        .ok_or("ImageMagick not found.")?
        .args([&input, "-auto-orient", "-colorspace", "sRGB", "-alpha", "on", "-depth", "8", "PAM:-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't run ImageMagick: {e}"))?;

    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
    });
    let mut stderr = child.stderr.take().unwrap();
    let err_reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let mut out = Vec::new();
    child.stdout.take().unwrap().read_to_end(&mut out).map_err(|e| e.to_string())?;
    let status = child.wait().map_err(|e| e.to_string())?;
    let _ = writer.join();
    let stderr = err_reader.join().unwrap_or_default();

    let frames = parse_pam_stream(&out)?;
    if frames.is_empty() {
        let first = stderr.lines().next().unwrap_or("no output").trim();
        let first = first.trim_start_matches("magick: ").trim_start_matches("magick.exe: ");
        return Err(if status.success() { "ImageMagick produced no image.".into() } else { format!("ImageMagick: {first}") });
    }

    let fmt = match label {
        Some("unknown") | None => "Image".to_string(),
        Some(l) => l.to_string(),
    };
    let mut dec = Decoded::new(fmt, "magick", "Rgba8 (via ImageMagick)");
    dec.frames = frames.into_iter().map(|image| Frame { image: Arc::new(image), delay_ms: 0 }).collect();
    if dec.frames.len() > 1 {
        dec.notes.push(format!("{} pages/frames.", dec.frames.len()));
    }
    Ok(dec)
}

/// Parse one or more concatenated PAM (P7) images.
pub fn parse_pam_stream(mut data: &[u8]) -> Result<Vec<RgbaImage>, String> {
    let mut images = Vec::new();
    while data.len() > 3 && data.starts_with(b"P7") {
        let end = find(data, b"ENDHDR\n").ok_or("Malformed PAM header from ImageMagick.")?;
        let header = std::str::from_utf8(&data[..end]).map_err(|_| "Bad PAM header")?;
        let (mut w, mut h, mut depth, mut maxval) = (0usize, 0usize, 0usize, 255u32);
        for line in header.lines() {
            let mut it = line.split_whitespace();
            match (it.next(), it.next()) {
                (Some("WIDTH"), Some(v)) => w = v.parse().unwrap_or(0),
                (Some("HEIGHT"), Some(v)) => h = v.parse().unwrap_or(0),
                (Some("DEPTH"), Some(v)) => depth = v.parse().unwrap_or(0),
                (Some("MAXVAL"), Some(v)) => maxval = v.parse().unwrap_or(255),
                _ => {}
            }
        }
        if w == 0 || h == 0 || !(1..=4).contains(&depth) || maxval != 255 {
            return Err(format!("Unexpected PAM layout {w}x{h} depth {depth} maxval {maxval}."));
        }
        let body = &data[end + 7..];
        let len = w * h * depth;
        if body.len() < len {
            return Err("Truncated pixel data from ImageMagick.".into());
        }
        let px = &body[..len];
        let rgba: Vec<u8> = match depth {
            4 => px.to_vec(),
            3 => px.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
            2 => px.chunks_exact(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
            _ => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        };
        images.push(RgbaImage::from_raw(w as u32, h as u32, rgba).ok_or("Bad PAM buffer")?);
        data = &body[len..];
        while data.first().is_some_and(|b| b.is_ascii_whitespace()) {
            data = &data[1..];
        }
    }
    Ok(images)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}
