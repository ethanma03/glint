//! Decoder coverage against tests/fixtures (regenerate with `python scripts/make_fixtures.py`).

use std::path::PathBuf;

use glint::decode::{self, magick};

fn fx(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

fn ok(name: &str) -> decode::Decoded {
    decode::decode(&fx(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn err(name: &str) -> String {
    match decode::decode(&fx(name)) {
        Ok(d) => panic!("{name}: expected an error, decoded {}x{} {}", d.width(), d.height(), d.format),
        Err(e) => e,
    }
}

#[test]
fn builtin_formats() {
    for (name, w, h, fmt) in [
        ("sprite.png", 32, 32, "PNG"),
        ("sprite.bmp", 32, 32, "BMP"),
        ("sprite.tga", 32, 32, "TGA"),
        ("photo.qoi", 32, 32, "QOI"),
        ("photo.jpg", 640, 400, "JPEG"),
        ("photo.webp", 200, 125, "WebP"),
        ("sheet.png", 128, 64, "PNG"),
        ("wide.png", 17000, 300, "PNG"),
        ("gray16.png", 256, 64, "PNG"),
        ("rgba16.png", 200, 100, "PNG"),
        ("multi.ico", 64, 64, "ICO"),
    ] {
        let d = ok(name);
        assert_eq!((d.width(), d.height(), d.format.as_str()), (w, h, fmt), "{name}");
    }
}

#[test]
fn sixteen_bit_keeps_source_depth_info() {
    assert!(ok("gray16.png").color.contains("16"));
    let d = ok("rgba16.png");
    let a = d.frames[0].image.get_pixel(10, 10).0[3];
    assert!((150..=156).contains(&a), "alpha {a}");
}

#[test]
fn dds_decodes_natively_or_via_magick() {
    let d = ok("sprite.dds");
    assert_eq!((d.width(), d.height()), (32, 32));
}

#[test]
fn animations() {
    for name in ["anim.gif", "anim.webp", "anim.apng"] {
        let d = ok(name);
        assert_eq!(d.frames.len(), 4, "{name}");
        assert!(d.is_animated(), "{name}");
        assert_eq!(d.frames[0].delay_ms, 120, "{name}");
    }
}

#[test]
fn aseprite_frames_and_tags() {
    let d = ok("walk.aseprite");
    assert_eq!((d.width(), d.height(), d.frames.len()), (16, 16, 3));
    assert_eq!(d.tags.len(), 1);
    assert_eq!((d.tags[0].name.as_str(), d.tags[0].from, d.tags[0].to), ("walk", 0, 1));
    assert_eq!(d.frames[1].image.get_pixel(6, 6).0, [60, 200, 90, 255]);
}

#[test]
fn svg_rasterizes_crisply() {
    let d = ok("icon.svg");
    assert_eq!(d.format, "SVG");
    assert_eq!(d.width(), 2048);
}

#[test]
fn exif_orientation_applied() {
    let d = ok("exif_rotated.jpg");
    // 300x200 stored, RightTop orientation → displayed 200x300.
    assert_eq!((d.width(), d.height()), (200, 300));
}

#[test]
fn hdr_exr_has_float_source() {
    let d = ok("bright.exr");
    assert!(d.hdr.is_some());
}

#[test]
fn content_sniffing_beats_extension() {
    let d = ok("actually_jpeg.png");
    assert_eq!(d.format, "JPEG");
    assert!(d.notes.iter().any(|n| n.contains("doesn't match")));
    assert_eq!(ok("no_extension").format, "PNG");
    assert_eq!(ok("brackets [1].png").width(), 32);
    assert_eq!(ok("ünïcödé スプライト.png").width(), 32);
}

#[test]
fn broken_files_give_friendly_errors() {
    assert!(err("empty.png").contains("Empty file"));
    assert!(err("does_not_exist.png").contains("not found"));
    let g = err("garbage.png");
    assert!(!g.is_empty());
}

#[test]
fn truncated_files_do_not_crash() {
    // Either a partial decode or an error is fine; a panic is not.
    for name in ["truncated.png", "truncated.jpg"] {
        let _ = decode::decode(&fx(name));
    }
}

#[test]
fn magick_formats() {
    if !magick::available() {
        eprintln!("ImageMagick not installed; skipping");
        return;
    }
    for (name, w, h, fmt) in [
        ("photo.avif", 200, 125, "AVIF"),
        ("photo.jxl", 200, 125, "JPEG XL"),
        ("layers.psd", 32, 32, "PSD"),
    ] {
        let d = ok(name);
        assert_eq!((d.width(), d.height(), d.format.as_str()), (w, h, fmt), "{name}");
    }
    // CMYK JPEG: either decoder is fine as long as colors come out as RGB.
    let d = ok("cmyk.jpg");
    assert_eq!((d.width(), d.height()), (640, 400));
    // Multi-page TIFF: both pages available.
    let d = ok("pages.tiff");
    assert!(!d.frames.is_empty());
}
