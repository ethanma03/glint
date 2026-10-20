//! `glint` — command-line front end. Opens images in the (single) viewer window,
//! remote-controls it, and runs headless image operations.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};

use glint::analysis::{SheetGrid, parse_pair, parse_size};
use glint::ipc::{self, ClientError};
use glint::ops::{self, Output};

#[derive(Parser)]
#[command(
    name = "glint",
    version,
    about = "Glint — fast image viewer for sprites and screenshots",
    args_conflicts_with_subcommands = true,
    after_help = "Run `glint <command> --help` for details. `glint ctl help` lists viewer commands."
)]
struct Cli {
    /// Images to open (reuses the running window).
    files: Vec<PathBuf>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open images in the viewer.
    Open { files: Vec<PathBuf> },
    /// Describe an image: format, size, frames, colors, alpha bounds.
    Info {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Compare two images: print metrics and (unless --no-show) open compare mode.
    Compare {
        a: PathBuf,
        b: PathBuf,
        /// diff | side | onion | swipe
        #[arg(long, default_value = "diff")]
        mode: String,
        #[arg(long)]
        json: bool,
        /// Don't open the viewer, just print metrics.
        #[arg(long)]
        no_show: bool,
        /// Write a diff visualization PNG.
        #[arg(long)]
        diff_out: Option<PathBuf>,
        /// Animation frame to compare.
        #[arg(long, default_value_t = 0)]
        frame: usize,
        /// Exit with code 1 if the images differ.
        #[arg(long)]
        fail_on_diff: bool,
    },
    /// Send a command to the running viewer (e.g. `glint ctl zoom 400`). `glint ctl help` lists them.
    Ctl {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        argv: Vec<String>,
    },
    /// Print the viewer's state as JSON (file, zoom, visible rect, frame, cursor pixel, ...).
    State,
    /// Save exactly what the viewer shows to a PNG (HUD hidden unless --ui).
    Snap {
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Include the info bar and overlays.
        #[arg(long)]
        ui: bool,
    },
    /// Edit an image through ImageMagick: crop x,y,w,h | rotate DEG | flip h|v | scale 2x|50%|WxH |
    /// trim | pad WxH [gravity] | key #hex [fuzz%] | flatten [#hex] | colors N | grayscale | strip | convert EXT
    Edit {
        op: String,
        #[arg(allow_negative_numbers = true)]
        args: Vec<String>,
        /// Input file (default: the image open in the viewer).
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Output file. Without -o or --in-place the result is previewed from a temp file.
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Overwrite the input (backed up; see `glint undo`).
        #[arg(long)]
        in_place: bool,
        /// Use smooth (Lanczos) instead of nearest-neighbour resampling for scale.
        #[arg(long)]
        smooth: bool,
        /// Don't show the result in the viewer.
        #[arg(long)]
        no_show: bool,
    },
    /// Run arbitrary magick arguments on an image: `glint magick -o out.png -- -modulate 100,120`.
    Magick {
        #[arg(short, long)]
        input: Option<PathBuf>,
        #[arg(short, long)]
        out: Option<PathBuf>,
        #[arg(long)]
        in_place: bool,
        #[arg(long)]
        no_show: bool,
        #[arg(last = true, required = true)]
        args: Vec<String>,
    },
    /// Restore the file changed by the last --in-place edit.
    Undo,
    /// Slice or inspect a sprite sheet.
    Sheet {
        file: PathBuf,
        /// Cell size in pixels, e.g. 32x32 or 5x7.
        #[arg(long, conflicts_with_all = ["cells", "detect"])]
        grid: Option<String>,
        /// Number of cells as COLUMNSxROWS, e.g. 5x5 (cell size is computed).
        #[arg(long, conflicts_with = "detect")]
        cells: Option<String>,
        /// Detect the grid from the transparent gaps between sprites.
        #[arg(long)]
        detect: bool,
        #[arg(long, default_value = "0,0")]
        offset: String,
        #[arg(long, default_value = "0,0")]
        gap: String,
        /// Write each non-empty cell as a PNG into this folder.
        #[arg(long)]
        export: Option<PathBuf>,
        #[arg(long)]
        keep_empty: bool,
        /// Show the grid overlay in the viewer.
        #[arg(long)]
        show: bool,
        #[arg(long)]
        json: bool,
    },
    /// List supported formats.
    Formats,
    /// Close the viewer.
    Quit,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            if let Some(ClientError::NotRunning) = e.downcast_ref::<ClientError>() {
                eprintln!("glint: no viewer window is running (start one with `glint <file>`)");
                return ExitCode::from(2);
            }
            eprintln!("glint: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn abs(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

fn s(p: &Path) -> String {
    abs(p).display().to_string()
}

fn print(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

/// Send a viewer command; error if the viewer reports failure.
fn ctl(argv: Vec<String>) -> Result<Value> {
    let r = ipc::send(&argv).map_err(|e| match e {
        ClientError::NotRunning => anyhow::Error::new(ClientError::NotRunning),
        e => anyhow!("{e}"),
    })?;
    if r.ok { Ok(r.result) } else { bail!("{}", r.error) }
}

/// Make sure a viewer is running, starting `glintw.exe` detached if needed.
fn ensure_viewer(initial: &[String]) -> Result<bool> {
    if ipc::is_running() {
        return Ok(false);
    }
    let exe = std::env::current_exe()?.with_file_name("glintw.exe");
    if !exe.exists() {
        bail!("glintw.exe not found next to {}", exe.display());
    }
    glint::win::spawn_detached(&exe, initial)
        .with_context(|| format!("starting {}", exe.display()))?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        if ipc::is_running() {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    bail!("the viewer didn't start within 10s")
}

fn open_in_viewer(files: &[PathBuf]) -> Result<()> {
    for f in files {
        if !f.exists() {
            bail!("not found: {}", f.display());
        }
    }
    let paths: Vec<String> = files.iter().map(|f| s(f)).collect();
    if !ensure_viewer(&paths)? {
        let mut argv = vec!["open".to_string()];
        argv.extend(paths);
        ctl(argv)?;
    }
    wait_idle()
}

/// Block until the viewer has finished decoding, so follow-up `ctl` commands
/// (zoom, frame, ...) apply to the new image rather than being reset by it.
fn wait_idle() -> Result<()> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        if ctl(vec!["state".into()])?["busy"] != Value::Bool(true) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

/// Image currently shown in the viewer, for edits without -i.
fn current_file() -> Result<PathBuf> {
    let st = ctl(vec!["state".into()])?;
    st["file"].as_str().map(PathBuf::from).ok_or_else(|| anyhow!("the viewer has no image open; pass -i <file>"))
}

fn output_mode(out: Option<PathBuf>, in_place: bool) -> Result<Output> {
    Ok(match (out, in_place) {
        (Some(_), true) => bail!("use either -o or --in-place, not both"),
        (Some(p), false) => Output::To(abs(&p)),
        (None, true) => Output::InPlace,
        (None, false) => Output::Preview,
    })
}

fn finish_edit(written: &Path, mode: &Output, no_show: bool) -> Result<()> {
    let shown = if no_show {
        false
    } else if matches!(mode, Output::InPlace) {
        // The viewer reloads changed files by itself; just make sure it shows this one.
        match ctl(vec!["state".into()]) {
            // Reload explicitly (instead of waiting for the file watcher) so the next command sees the result.
            Ok(st) if st["file"].as_str().map(PathBuf::from).as_deref() == Some(written) => {
                ctl(vec!["reload".into()]).is_ok() && wait_idle().is_ok()
            }
            Ok(_) => ctl(vec!["open".into(), s(written)]).is_ok(),
            Err(_) => false,
        }
    } else if matches!(mode, Output::Preview) {
        open_in_viewer(&[written.to_path_buf()]).is_ok()
    } else {
        ipc::is_running() && ctl(vec!["open".into(), s(written)]).is_ok()
    };
    let (w, h) = image::image_dimensions(written).map(|(w, h)| (Some(w), Some(h))).unwrap_or((None, None));
    print(&json!({ "output": written, "width": w, "height": h, "shown": shown,
        "preview": matches!(mode, Output::Preview) }));
    Ok(())
}

fn run(cli: Cli) -> Result<ExitCode> {
    let cmd = match cli.cmd {
        Some(c) => c,
        None if cli.files.is_empty() => {
            ensure_viewer(&[])?;
            ctl(vec!["focus".into()])?;
            return Ok(ExitCode::SUCCESS);
        }
        None => Cmd::Open { files: cli.files },
    };
    match cmd {
        Cmd::Open { files } => {
            if files.is_empty() {
                ensure_viewer(&[])?;
            } else {
                open_in_viewer(&files)?;
            }
        }
        Cmd::Info { file, json } => {
            let v = ops::info(&file)?;
            if json {
                print(&v);
            } else {
                print_info(&v);
            }
        }
        Cmd::Compare { a, b, mode, json, no_show, diff_out, frame, fail_on_diff } => {
            let m = ops::compare(&a, &b, frame, diff_out.as_deref())?;
            if json {
                print(&serde_json::to_value(&m)?);
            } else if m.identical {
                println!("identical ({}x{})", m.a_size[0], m.a_size[1]);
            } else {
                println!(
                    "{} of {} pixels differ ({:.4}%), max channel diff {}, RMSE {:.3}, PSNR {}",
                    m.differing_pixels,
                    m.total_pixels,
                    m.differing_percent,
                    m.max_channel_diff,
                    m.rmse,
                    m.psnr_db.map_or("inf".into(), |p| format!("{p:.2} dB"))
                );
                if let Some([x, y, w, h]) = m.diff_bbox {
                    println!("changed region: {w}x{h} at {x},{y}");
                }
                if m.size_mismatch {
                    println!("sizes differ: {}x{} vs {}x{}", m.a_size[0], m.a_size[1], m.b_size[0], m.b_size[1]);
                }
            }
            if !no_show {
                ensure_viewer(&[])?;
                ctl(vec!["compare".into(), s(&a), s(&b), mode])?;
                wait_idle()?;
            }
            if fail_on_diff && !m.identical {
                return Ok(ExitCode::from(1));
            }
        }
        Cmd::Ctl { argv } => {
            let argv = absolutize_ctl(argv);
            let loads = matches!(argv[0].as_str(), "open" | "goto" | "next" | "prev" | "first" | "last" | "compare" | "reload");
            let v = ctl(argv)?;
            if loads {
                wait_idle()?;
            }
            if v != Value::Bool(true) {
                print(&v);
            }
        }
        Cmd::State => print(&ctl(vec!["state".into()])?),
        Cmd::Snap { out, ui } => {
            let out = abs(&out.unwrap_or_else(|| std::env::temp_dir().join("glint").join("snap.png")));
            let mut argv = vec!["snap".to_string(), out.display().to_string()];
            if ui {
                argv.push("--ui".into());
            }
            print(&ctl(argv)?);
        }
        Cmd::Edit { op, args, input, out, in_place, smooth, no_show } => {
            let input = match input {
                Some(i) => abs(&i),
                None => current_file()?,
            };
            let (margs, ext) = ops::edit_args(&op, &args, smooth)?;
            let mode = output_mode(out, in_place)?;
            let written = ops::run_magick(&input, &margs, &mode, ext.as_deref())?;
            finish_edit(&written, &mode, no_show)?;
        }
        Cmd::Magick { input, out, in_place, no_show, args } => {
            let input = match input {
                Some(i) => abs(&i),
                None => current_file()?,
            };
            let mode = output_mode(out, in_place)?;
            let written = ops::run_magick(&input, &args, &mode, None)?;
            finish_edit(&written, &mode, no_show)?;
        }
        Cmd::Undo => {
            let p = ops::undo()?;
            println!("restored {}", p.display());
        }
        Cmd::Sheet { file, grid, cells, detect, offset, gap, export, keep_empty, show, json } => {
            let offset = parse_pair(&offset).ok_or_else(|| anyhow!("bad --offset"))?;
            let gap = parse_pair(&gap).ok_or_else(|| anyhow!("bad --gap"))?;
            let g = match (grid, cells, detect) {
                (Some(grid), _, _) => SheetGrid {
                    cell: parse_size(&grid).ok_or_else(|| anyhow!("bad --grid '{grid}' (e.g. 32x32)"))?,
                    offset,
                    gap,
                },
                (_, Some(cells), _) => {
                    let n = parse_size(&cells).ok_or_else(|| anyhow!("bad --cells '{cells}' (e.g. 5x5)"))?;
                    let (w, h) = glint::decode::decode(&file).map(|d| (d.width(), d.height())).map_err(|e| anyhow!(e))?;
                    SheetGrid::from_counts(w, h, n, offset, gap).ok_or_else(|| anyhow!("{cells} cells don't fit {w}x{h}"))?.0
                }
                (_, _, true) => {
                    let d = glint::decode::decode(&file).map_err(|e| anyhow!(e))?;
                    glint::analysis::detect_grid(&d.frames[0].image)
                        .ok_or_else(|| anyhow!("couldn't detect a regular grid; pass --grid WxH or --cells CxR"))?
                }
                _ => bail!("give one of --grid WxH, --cells COLSxROWS, or --detect"),
            };
            let v = ops::sheet(&file, &ops::SheetOpts { grid: g, export: export.map(|e| abs(&e)), keep_empty })?;
            if json {
                print(&v);
            } else {
                println!(
                    "{}x{} cells of {}x{}: {} non-empty of {}",
                    v["columns"], v["rows"], g.cell[0], g.cell[1], v["cells_non_empty"], v["cells_total"]
                );
                if export_written(&v) > 0 {
                    println!("exported {} PNGs", export_written(&v));
                }
            }
            if show {
                open_in_viewer(&[file.clone()])?;
                ctl(vec![
                    "sheet".into(),
                    format!("{}x{}", g.cell[0], g.cell[1]),
                    "offset".into(),
                    format!("{},{}", g.offset[0], g.offset[1]),
                    "gap".into(),
                    format!("{},{}", g.gap[0], g.gap[1]),
                ])?;
            }
        }
        Cmd::Formats => {
            for (name, exts, how) in ops::formats() {
                println!("{name:<28} {exts:<34} {how}");
            }
            println!(
                "\nImageMagick: {}",
                glint::decode::magick::exe().map_or("not found".into(), |p| p.display().to_string())
            );
        }
        Cmd::Quit => match ipc::send(&["quit".into()]) {
            Ok(_) | Err(ClientError::NotRunning) => {}
            Err(e) => bail!("{e}"),
        },
    }
    Ok(ExitCode::SUCCESS)
}

fn export_written(v: &Value) -> usize {
    v["cells"].as_array().map_or(0, |c| c.iter().filter(|c| c.get("file").is_some()).count())
}

/// Make file arguments of path-taking ctl commands absolute (the viewer has a different cwd).
fn absolutize_ctl(mut argv: Vec<String>) -> Vec<String> {
    let path_args: &[usize] = match argv.first().map(String::as_str) {
        Some("open") => &[],
        Some("compare") => &[1, 2],
        Some("goto" | "compare-add") => &[1],
        _ => return argv,
    };
    let all = argv.first().map(String::as_str) == Some("open");
    for i in 1..argv.len() {
        let is_path = all || path_args.contains(&i);
        let p = Path::new(&argv[i]);
        if is_path && (p.exists() || p.extension().is_some()) && argv[i].parse::<usize>().is_err() {
            argv[i] = s(p);
        }
    }
    argv
}

fn print_info(v: &Value) {
    let line = |k: &str, val: String| println!("{k:>14}  {val}");
    line("file", v["path"].as_str().unwrap_or_default().to_string());
    line("format", format!("{} ({}, {})", v["format"].as_str().unwrap_or(""), v["decoder"].as_str().unwrap_or(""), v["color"].as_str().unwrap_or("")));
    line("size", format!("{} x {}", v["width"], v["height"]));
    let frames = v["frames"].as_u64().unwrap_or(1);
    if frames > 1 {
        let total: u64 = v["frame_delays_ms"].as_array().map_or(0, |a| a.iter().filter_map(Value::as_u64).sum());
        line("frames", format!("{frames}{}", if total > 0 { format!(" ({total} ms total)") } else { String::new() }));
    }
    if let Some(tags) = v["tags"].as_array().filter(|t| !t.is_empty()) {
        let t: Vec<String> = tags.iter().map(|t| format!("{} [{}-{}]", t["name"].as_str().unwrap_or(""), t["from"], t["to"])).collect();
        line("tags", t.join(", "));
    }
    line("file size", glint_human(v["file_size"].as_u64().unwrap_or(0)));
    line("colors", v["unique_colors"].to_string());
    if v["has_alpha"] == true {
        line("alpha", format!("{} transparent, {} partial", v["transparent_pixels"], v["semi_transparent_pixels"]));
        if let Some(b) = v["opaque_bbox"].as_array() {
            line("content bbox", format!("{}x{} at {},{}", b[2], b[3], b[0], b[1]));
        }
    } else {
        line("alpha", "opaque".into());
    }
    for n in v["notes"].as_array().into_iter().flatten() {
        line("note", n.as_str().unwrap_or("").to_string());
    }
}

fn glint_human(n: u64) -> String {
    let mut v = n as f64;
    for u in ["B", "KB", "MB", "GB"] {
        if v < 1024.0 || u == "GB" {
            return if u == "B" { format!("{n} B") } else { format!("{v:.1} {u}") };
        }
        v /= 1024.0;
    }
    unreachable!()
}
