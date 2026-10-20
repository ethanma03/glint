//! Headless operations behind the CLI: info, compare, edits via ImageMagick, undo, sheet slicing.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::analysis::{self, SheetGrid};
use crate::decode::{self, magick};

pub fn info(path: &Path) -> Result<Value> {
    let dec = decode::decode(path).map_err(|e| anyhow!(e))?;
    let first = &dec.frames[0].image;
    let stats = analysis::stats(first);
    Ok(json!({
        "path": dec.path,
        "format": dec.format,
        "decoder": dec.decoder,
        "color": dec.color,
        "width": dec.width(),
        "height": dec.height(),
        "frames": dec.frames.len(),
        "animated": dec.is_animated(),
        "frame_delays_ms": dec.frames.iter().map(|f| f.delay_ms).collect::<Vec<_>>(),
        "frame_sizes": dec.frames.iter().map(|f| [f.image.width(), f.image.height()]).collect::<Vec<_>>(),
        "tags": dec.tags,
        "hdr": dec.hdr.is_some(),
        "file_size": dec.file_size,
        "unique_colors": stats.unique_colors,
        "has_alpha": stats.transparent_pixels > 0,
        "transparent_pixels": stats.transparent_pixels,
        "semi_transparent_pixels": stats.semi_transparent_pixels,
        "opaque_bbox": stats.opaque_bbox,
        "notes": dec.notes,
    }))
}

pub fn compare(a: &Path, b: &Path, frame: usize, diff_out: Option<&Path>) -> Result<analysis::DiffMetrics> {
    let da = decode::decode(a).map_err(|e| anyhow!("{}: {e}", a.display()))?;
    let db = decode::decode(b).map_err(|e| anyhow!("{}: {e}", b.display()))?;
    let fa = &da.frames.get(frame).ok_or_else(|| anyhow!("{} has no frame {frame}", a.display()))?.image;
    let fb = &db.frames.get(frame).ok_or_else(|| anyhow!("{} has no frame {frame}", b.display()))?.image;
    let m = analysis::diff(fa, fb);
    if let Some(out) = diff_out {
        analysis::diff_image(fa, fb).save(out).with_context(|| format!("writing {}", out.display()))?;
    }
    Ok(m)
}

/// Translate a named edit into ImageMagick arguments. Returns (args, new extension).
pub fn edit_args(op: &str, args: &[String], smooth: bool) -> Result<(Vec<String>, Option<String>)> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let arg = |i: usize, what: &str| args.get(i).cloned().ok_or_else(|| anyhow!("{op}: missing {what}"));
    let filter = if smooth { "Lanczos" } else { "Point" };
    Ok(match op {
        "crop" => {
            let v: Vec<u32> = arg(0, "x,y,w,h")?.split([',', ' ']).filter_map(|t| t.trim().parse().ok()).collect();
            let [x, y, w, h] = v[..] else { bail!("crop: expected x,y,w,h") };
            (s(&["-crop", &format!("{w}x{h}+{x}+{y}"), "+repage"]), None)
        }
        "rotate" => (s(&["-background", "none", "-rotate", &arg(0, "degrees")?, "+repage"]), None),
        "flip" => match arg(0, "h|v")?.as_str() {
            "h" | "horizontal" | "x" => (s(&["-flop"]), None),
            "v" | "vertical" | "y" => (s(&["-flip"]), None),
            o => bail!("flip: expected h or v, got {o}"),
        },
        "scale" | "resize" => {
            let g = arg(0, "factor (2x, 50%, WxH, W)")?;
            let geom = match g.strip_suffix('x') {
                Some(f) if !f.contains('x') => format!("{}%", f.parse::<f64>().context("bad factor")? * 100.0),
                _ => g.clone(),
            };
            (s(&["-filter", filter, "-resize", &geom]), None)
        }
        "trim" => (s(&["-trim", "+repage"]), None),
        "pad" | "extent" => {
            let size = arg(0, "WxH")?;
            let gravity = args.get(1).cloned().unwrap_or_else(|| "center".into());
            (s(&["-background", "none", "-gravity", &gravity, "-extent", &size]), None)
        }
        "key" | "alpha-key" => {
            let color = arg(0, "#rrggbb")?;
            let fuzz = args.get(1).cloned().unwrap_or_else(|| "0%".into());
            (s(&["-alpha", "set", "-fuzz", &fuzz, "-transparent", &color]), None)
        }
        "flatten" => {
            let color = args.first().cloned().unwrap_or_else(|| "white".into());
            (s(&["-background", &color, "-flatten"]), None)
        }
        "colors" | "quantize" => (s(&["+dither", "-colors", &arg(0, "count")?]), None),
        "grayscale" => (s(&["-colorspace", "Gray"]), None),
        "strip" => (s(&["-strip"]), None),
        "convert" => (vec![], Some(arg(0, "format (png, webp, ...)")?.trim_start_matches('.').to_lowercase())),
        o => bail!(
            "unknown edit '{o}'. Available: crop, rotate, flip, scale, trim, pad, key, flatten, colors, grayscale, strip, convert"
        ),
    })
}

#[derive(Debug, Clone)]
pub enum Output {
    /// Write to a temp file and show it in the viewer; the original is untouched.
    Preview,
    To(PathBuf),
    /// Replace the input (a backup is kept for `glint undo`).
    InPlace,
}

fn glint_dir(sub: &str) -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("Glint").join(sub)
}

fn stamp() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{}{:03}", t.as_secs(), t.subsec_millis())
}

/// Run `magick <input> <args> <output>`; returns the written path.
pub fn run_magick(input: &Path, args: &[String], output: &Output, new_ext: Option<&str>) -> Result<PathBuf> {
    if !input.exists() {
        bail!("not found: {}", input.display());
    }
    let ext = new_ext
        .map(str::to_string)
        .or_else(|| input.extension().map(|e| e.to_string_lossy().to_lowercase()))
        .unwrap_or_else(|| "png".into());
    let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "image".into());
    let target = match output {
        Output::To(p) => p.clone(),
        Output::Preview => {
            let dir = std::env::temp_dir().join("glint").join("preview");
            std::fs::create_dir_all(&dir)?;
            dir.join(format!("{stem}-{}.{ext}", stamp()))
        }
        Output::InPlace => match new_ext {
            // Converting "in place" writes a sibling with the new extension.
            Some(e) => input.with_extension(e),
            None => input.to_path_buf(),
        },
    };
    let replacing = target.exists() && same_file(&target, input);
    // Write to a temp sibling first so a failed command never truncates the original.
    let tmp = target.with_file_name(format!(".glint-tmp-{}.{ext}", stamp()));
    let mut cmd = magick::command().ok_or_else(|| anyhow!("ImageMagick (magick.exe) not found on PATH"))?;
    cmd.arg(input).args(args).arg(&tmp);
    let out = cmd.output().context("running magick")?;
    if !out.status.success() || !tmp.exists() {
        let _ = std::fs::remove_file(&tmp);
        bail!("magick failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    if replacing || (matches!(output, Output::InPlace) && target.exists()) {
        backup(&target)?;
    }
    std::fs::rename(&tmp, &target)
        .or_else(|_| std::fs::copy(&tmp, &target).map(|_| ()).and_then(|_| std::fs::remove_file(&tmp)))
        .with_context(|| format!("writing {}", target.display()))?;
    Ok(target)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct UndoEntry {
    original: PathBuf,
    backup: PathBuf,
    time: String,
}

fn journal_path() -> PathBuf {
    glint_dir("undo").join("journal.json")
}

fn load_journal() -> Vec<UndoEntry> {
    std::fs::read(journal_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_journal(j: &[UndoEntry]) -> Result<()> {
    std::fs::create_dir_all(glint_dir("undo"))?;
    std::fs::write(journal_path(), serde_json::to_vec_pretty(j)?)?;
    Ok(())
}

fn backup(path: &Path) -> Result<()> {
    let dir = glint_dir("undo");
    std::fs::create_dir_all(&dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let bak = dir.join(format!("{}-{name}", stamp()));
    std::fs::copy(path, &bak).with_context(|| format!("backing up {}", path.display()))?;
    let mut j = load_journal();
    j.push(UndoEntry { original: path.to_path_buf(), backup: bak, time: stamp() });
    // Keep the last 50 backups.
    while j.len() > 50 {
        let old = j.remove(0);
        let _ = std::fs::remove_file(old.backup);
    }
    save_journal(&j)
}

/// Restore the most recent in-place edit. Returns the restored file.
pub fn undo() -> Result<PathBuf> {
    let mut j = load_journal();
    let e = j.pop().ok_or_else(|| anyhow!("nothing to undo"))?;
    std::fs::copy(&e.backup, &e.original).with_context(|| format!("restoring {}", e.original.display()))?;
    let _ = std::fs::remove_file(&e.backup);
    save_journal(&j)?;
    Ok(e.original)
}

pub struct SheetOpts {
    pub grid: SheetGrid,
    pub export: Option<PathBuf>,
    pub keep_empty: bool,
}

pub fn sheet(path: &Path, o: &SheetOpts) -> Result<Value> {
    let dec = decode::decode(path).map_err(|e| anyhow!(e))?;
    let img = &dec.frames[0].image;
    let g = o.grid;
    let (cols, rows) = (g.columns(img.width()), g.rows(img.height()));
    if cols == 0 || rows == 0 {
        bail!("grid {}x{} doesn't fit in {}x{}", g.cell[0], g.cell[1], img.width(), img.height());
    }
    if let Some(dir) = &o.export {
        std::fs::create_dir_all(dir)?;
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "cell".into());
    let digits = ((cols * rows) as f32).log10().floor() as usize + 1;
    let mut cells = Vec::new();
    for i in 0..cols * rows {
        let [x, y, w, h] = g.cell_rect(i, img.width());
        let sub = image::imageops::crop_imm(img.as_ref(), x, y, w, h).to_image();
        let st = analysis::stats(&sub);
        let empty = st.opaque_bbox.is_none();
        let mut entry = json!({
            "index": i, "row": i / cols, "col": i % cols, "rect": [x, y, w, h],
            "empty": empty, "content_bbox": st.opaque_bbox, "unique_colors": st.unique_colors,
        });
        if let Some(dir) = &o.export {
            if !empty || o.keep_empty {
                let file = dir.join(format!("{stem}_{i:0digits$}.png"));
                sub.save(&file).with_context(|| format!("writing {}", file.display()))?;
                entry["file"] = json!(file);
            }
        }
        cells.push(entry);
    }
    let non_empty = cells.iter().filter(|c| c["empty"] == false).count();
    Ok(json!({
        "path": path, "width": img.width(), "height": img.height(),
        "grid": g, "columns": cols, "rows": rows, "cells_total": cols * rows, "cells_non_empty": non_empty,
        "cells": cells,
    }))
}

/// Supported formats, for `glint formats`.
pub fn formats() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("PNG / APNG", "png apng", "built-in (animated APNG supported)"),
        ("JPEG", "jpg jpeg jfif", "built-in, EXIF rotation; CMYK via magick"),
        ("GIF", "gif", "built-in, animated"),
        ("WebP", "webp", "built-in, animated"),
        ("BMP / ICO / CUR", "bmp dib ico cur", "built-in"),
        ("TGA / DDS / QOI", "tga dds qoi", "built-in"),
        ("TIFF", "tif tiff", "built-in; multi-page/exotic via magick"),
        ("OpenEXR / Radiance HDR", "exr hdr", "built-in, exposure control"),
        ("PNM / PAM / Farbfeld", "pbm pgm ppm pnm pam ff", "built-in"),
        ("SVG", "svg svgz", "built-in (resvg)"),
        ("Aseprite", "ase aseprite", "built-in, frames + tags"),
        ("AVIF / HEIC / JPEG XL", "avif heic heif jxl", "ImageMagick"),
        ("Photoshop / GIMP", "psd psb xcf", "ImageMagick (flattened composite)"),
        ("Camera RAW", "dng cr2 cr3 nef arw orf rw2 raf", "ImageMagick (libraw)"),
        ("Anything else magick reads", "*", "ImageMagick fallback, detected by content"),
    ]
}
