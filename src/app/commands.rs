//! The command language shared by `glint ctl`, the command palette and keybindings.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use super::{Bg, CmpMode, GlintApp, HudMode, Sheet, absolute};
use crate::analysis::{SheetGrid, hex, parse_pair, parse_size};

/// (usage, description) — shown by `glint ctl help` and searched by the palette.
pub const COMMANDS: &[(&str, &str)] = &[
    ("open <file>...", "Open files (several = browse just those)"),
    ("next | prev | first | last", "Navigate the folder"),
    ("goto <index|file>", "Jump to a file in the folder (0-based index)"),
    ("zoom <fit|in|out|N%|Nx>", "Set zoom; 100 = one screen pixel per image pixel"),
    ("center <x> <y>", "Center the view on an image pixel"),
    ("grid <on|off|toggle>", "Pixel grid (visible from 600%)"),
    ("sheet <WxH> [offset X,Y] [gap X,Y] [play A-B] [fps N]", "Sprite-sheet overlay and cell animation preview"),
    ("sheet off", "Hide the sprite-sheet overlay"),
    ("bg <checker|light-checker|black|gray|white|magenta|#rrggbb|next>", "Background behind transparency"),
    ("smooth <on|off|toggle>", "Smooth (bilinear) magnification instead of crisp pixels"),
    ("frame <N|next|prev>", "Show an animation frame / page (0-based)"),
    ("play [on|off|toggle]", "Play or pause animation"),
    ("tag <name|off>", "Loop an Aseprite tag"),
    ("speed <x>", "Animation speed multiplier"),
    ("compare <a> <b> [diff|side|onion|swipe]", "Compare two images"),
    ("compare off", "Leave compare mode"),
    ("mode <diff|side|onion|swipe|next>", "Change compare mode"),
    ("split <0..1>", "Onion opacity / swipe position"),
    ("swap", "Swap A and B in compare mode"),
    ("exposure <stops|+N|-N>", "Exposure for HDR images"),
    ("reload", "Reload from disk"),
    ("fullscreen [on|off|toggle]", "Fullscreen"),
    ("hud <auto|always|hidden|toggle>", "Bottom info bar visibility"),
    ("info [on|off|toggle]", "Details panel"),
    ("copy <image|path|hex>", "Copy to the clipboard"),
    ("reveal", "Show the file in Explorer"),
    ("state", "Viewer state as JSON"),
    ("snap <out.png> [--ui]", "Save exactly what the window shows"),
    ("focus", "Bring the window to the front"),
    ("quit", "Close the viewer"),
];

fn toggle(arg: Option<&str>, cur: bool) -> Result<bool, String> {
    match arg {
        None | Some("toggle") => Ok(!cur),
        Some("on" | "true" | "1" | "yes") => Ok(true),
        Some("off" | "false" | "0" | "no") => Ok(false),
        Some(o) => Err(format!("expected on/off/toggle, got '{o}'")),
    }
}

fn resolve(p: &str, cwd: Option<&Path>) -> PathBuf {
    let p = PathBuf::from(p);
    match cwd {
        Some(c) if p.is_relative() => c.join(p),
        _ => absolute(&p),
    }
}

pub fn parse_snap(args: &[String], cwd: Option<&Path>) -> Result<(PathBuf, bool), String> {
    let mut path = None;
    let mut clean = true;
    for a in args {
        match a.as_str() {
            "--ui" => clean = false,
            "--clean" => clean = true,
            p => path = Some(resolve(p, cwd)),
        }
    }
    let path = path.unwrap_or_else(|| std::env::temp_dir().join("glint").join("snap.png"));
    Ok((path, clean))
}

pub fn parse_bg(s: &str) -> Option<Bg> {
    Some(match s {
        "checker" | "dark-checker" | "checkerboard" => Bg::DarkChecker,
        "light-checker" => Bg::LightChecker,
        "black" => Bg::Black,
        "gray" | "grey" => Bg::Gray,
        "white" => Bg::White,
        "magenta" | "pink" => Bg::Magenta,
        h if h.starts_with('#') && h.len() == 7 => {
            let v = u32::from_str_radix(&h[1..], 16).ok()?;
            Bg::Custom([(v >> 16) as u8, (v >> 8) as u8, v as u8])
        }
        _ => return None,
    })
}

/// Split palette input like a shell: `open "C:\my file.png"`.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for ch in s.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(cur);
    }
    out
}

impl GlintApp {
    pub fn exec(&mut self, ctx: &egui::Context, argv: &[String], cwd: Option<&Path>) -> Result<Value, String> {
        let Some(cmd) = argv.first() else { return Err("empty command".into()) };
        let args: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
        let a0 = args.first().copied();
        let ok = || Ok(json!(true));
        match cmd.as_str() {
            "ping" => Ok(json!({ "pid": std::process::id() })),
            "help" if cwd.is_none() => {
                self.help = true;
                ok()
            }
            "help" => Ok(json!(COMMANDS.iter().map(|(u, d)| json!({"usage": u, "description": d})).collect::<Vec<_>>())),
            "palette" => {
                self.palette.open = true;
                self.palette.just_opened = true;
                self.palette.text.clear();
                self.palette.selected = 0;
                ok()
            }
            "open" => {
                if args.is_empty() {
                    return Err("usage: open <file>...".into());
                }
                let paths: Vec<PathBuf> = args.iter().map(|p| resolve(p, cwd)).collect();
                if let Some(missing) = paths.iter().find(|p| !p.exists()) {
                    return Err(format!("not found: {}", missing.display()));
                }
                self.open_list(paths);
                ok()
            }
            "next" => {
                self.step(1);
                ok()
            }
            "prev" => {
                self.step(-1);
                ok()
            }
            "first" => {
                self.goto_index(0);
                ok()
            }
            "last" => {
                let n = self.files.len();
                self.goto_index(n.saturating_sub(1));
                ok()
            }
            "goto" => {
                let a = a0.ok_or("usage: goto <index|file>")?;
                if let Ok(i) = a.parse::<usize>() {
                    if i >= self.files.len() {
                        return Err(format!("index out of range (0..{})", self.files.len()));
                    }
                    self.goto_index(i);
                } else {
                    let p = resolve(a, cwd);
                    self.open(&p);
                }
                ok()
            }
            "zoom" | "fit" => {
                let a = if cmd == "fit" { "fit" } else { a0.ok_or("usage: zoom <fit|in|out|N%|Nx>")? };
                let r = self.view.rect;
                match a {
                    "fit" => self.view.fit = true,
                    "in" => {
                        let z = self.view.step_zoom(1);
                        self.view.zoom_to(z, r.center(), r, true);
                    }
                    "out" => {
                        let z = self.view.step_zoom(-1);
                        self.view.zoom_to(z, r.center(), r, true);
                    }
                    s => {
                        let z = if let Some(x) = s.strip_suffix('x') {
                            x.parse::<f32>().map_err(|_| format!("bad zoom '{s}'"))?
                        } else {
                            s.trim_end_matches('%').parse::<f32>().map_err(|_| format!("bad zoom '{s}'"))? / 100.0
                        };
                        if !(z > 0.0) {
                            return Err("zoom must be positive".into());
                        }
                        let animate = cwd.is_none();
                        self.view.zoom_to(z, r.center(), r, animate);
                    }
                }
                ok()
            }
            "center" | "pan" => {
                let (x, y) = match args.as_slice() {
                    [xy] => {
                        let [x, y] = parse_pair(xy).ok_or("usage: center <x> <y>")?;
                        (x as f32, y as f32)
                    }
                    [x, y] => (x.parse().map_err(|_| "bad x")?, y.parse().map_err(|_| "bad y")?),
                    _ => return Err("usage: center <x> <y>".into()),
                };
                self.view.center_on(egui::vec2(x, y));
                ok()
            }
            "grid" => {
                self.settings.grid = toggle(a0, self.settings.grid)?;
                if cwd.is_none() {
                    let msg = if self.settings.grid { "Pixel grid on (visible from 600%)" } else { "Pixel grid off" };
                    self.toast(msg);
                }
                ok()
            }
            "sheet" => self.cmd_sheet(&args),
            "bg" => {
                let a = a0.ok_or("usage: bg <name|#rrggbb|next>")?;
                self.settings.bg = if a == "next" {
                    let i = Bg::CYCLE.iter().position(|b| *b == self.settings.bg).map_or(0, |i| i + 1);
                    Bg::CYCLE[i % Bg::CYCLE.len()]
                } else {
                    parse_bg(a).ok_or_else(|| format!("unknown background '{a}'"))?
                };
                if cwd.is_none() {
                    self.toast(format!("Background: {}", self.settings.bg.name()));
                }
                ok()
            }
            "smooth" => {
                self.settings.smooth = toggle(a0, self.settings.smooth)?;
                if cwd.is_none() {
                    self.toast(if self.settings.smooth { "Smooth scaling" } else { "Crisp pixels" });
                }
                ok()
            }
            "frame" => {
                let n = self.doc.as_ref().map_or(0, |d| d.dec.frames.len());
                if n == 0 {
                    return Err("no image".into());
                }
                let cur = self.anim.frame;
                self.anim.frame = match a0.ok_or("usage: frame <N|next|prev>")? {
                    "next" => (cur + 1) % n,
                    "prev" => (cur + n - 1) % n,
                    s => {
                        let i: usize = s.parse().map_err(|_| format!("bad frame '{s}'"))?;
                        if i >= n {
                            return Err(format!("frame out of range (0..{n})"));
                        }
                        i
                    }
                };
                self.anim.playing = false;
                self.anim.accum_ms = 0.0;
                ok()
            }
            "play" | "pause" => {
                let animated = self.doc.as_ref().is_some_and(|d| d.dec.frames.len() > 1);
                if !animated && cmd == "play" && a0.is_none() {
                    return ok();
                }
                self.anim.playing = if cmd == "pause" { false } else { toggle(a0, self.anim.playing)? };
                ok()
            }
            "tag" => {
                let a = a0.ok_or("usage: tag <name|off>")?;
                if a == "off" {
                    self.anim.range = None;
                    return ok();
                }
                let doc = self.doc.as_ref().ok_or("no image")?;
                let t = doc.dec.tags.iter().find(|t| t.name.eq_ignore_ascii_case(a)).ok_or_else(|| format!("no tag '{a}'"))?;
                self.anim.range = Some((t.from as usize, t.to as usize));
                self.anim.frame = t.from as usize;
                self.anim.playing = true;
                ok()
            }
            "speed" => {
                let s: f32 = a0.ok_or("usage: speed <x>")?.trim_end_matches('x').parse().map_err(|_| "bad speed")?;
                self.anim.speed = s.clamp(0.05, 16.0);
                ok()
            }
            "compare" => match args.as_slice() {
                ["off"] => {
                    self.compare = None;
                    self.pending_compare = None;
                    self.view.fit = true;
                    ok()
                }
                [a, b, rest @ ..] => {
                    let mode = rest.first().map_or(Some(CmpMode::Swipe), |m| CmpMode::parse(m)).ok_or("unknown mode")?;
                    let (a, b) = (resolve(a, cwd), resolve(b, cwd));
                    for p in [&a, &b] {
                        if !p.exists() {
                            return Err(format!("not found: {}", p.display()));
                        }
                    }
                    self.start_compare(a, b, mode);
                    ok()
                }
                _ => Err("usage: compare <a> <b> [diff|side|onion|swipe] | compare off".into()),
            },
            "compare-add" => {
                let p = resolve(a0.ok_or("usage: compare-add <file>")?, cwd);
                let now = self.last_time;
                self.compare_add(p, now);
                ok()
            }
            "mode" => {
                let c = self.compare.as_mut().ok_or("not comparing")?;
                c.mode = match a0.ok_or("usage: mode <diff|side|onion|swipe|next>")? {
                    "next" => {
                        let i = CmpMode::ALL.iter().position(|m| *m == c.mode).unwrap_or(0);
                        CmpMode::ALL[(i + 1) % 4]
                    }
                    m => CmpMode::parse(m).ok_or_else(|| format!("unknown mode '{m}'"))?,
                };
                let label = c.mode.label();
                self.view.fit = true;
                if cwd.is_none() {
                    self.toast(label);
                }
                ok()
            }
            "split" => {
                let c = self.compare.as_mut().ok_or("not comparing")?;
                c.split = a0.ok_or("usage: split <0..1>")?.parse::<f32>().map_err(|_| "bad split")?.clamp(0.0, 1.0);
                ok()
            }
            "swap" => {
                if let Some(c) = &mut self.compare {
                    let (a, b, m) = (c.b.path.clone(), c.a.path.clone(), c.mode);
                    self.start_compare(a, b, m);
                }
                ok()
            }
            "exposure" => {
                let a = a0.ok_or("usage: exposure <stops>")?;
                let doc = self.doc.as_mut().ok_or("no image")?;
                let hdr = doc.dec.hdr.clone().ok_or("exposure only applies to HDR images (EXR/HDR)")?;
                let v: f32 = a.trim_start_matches('+').parse().map_err(|_| "bad exposure")?;
                doc.exposure = if a.starts_with('+') || a.starts_with('-') { doc.exposure + v } else { v };
                let img = crate::decode::tonemap(&hdr, doc.exposure);
                doc.layer.replace_frame(0, Arc::new(img));
                let e = doc.exposure;
                if cwd.is_none() {
                    self.toast(format!("Exposure {e:+.1} EV"));
                }
                ok()
            }
            "reload" => {
                self.reload();
                ok()
            }
            "fullscreen" => {
                let cur = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                let on = toggle(a0, cur)?;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
                ok()
            }
            "hud" => {
                self.settings.hud = match a0 {
                    Some("auto") => HudMode::Auto,
                    Some("always" | "on") => HudMode::Always,
                    Some("hidden" | "off") => HudMode::Hidden,
                    None | Some("toggle") => match self.settings.hud {
                        HudMode::Hidden => HudMode::Auto,
                        HudMode::Auto => HudMode::Always,
                        HudMode::Always => HudMode::Hidden,
                    },
                    Some(o) => return Err(format!("unknown hud mode '{o}'")),
                };
                if cwd.is_none() {
                    self.toast(format!("Info bar: {:?}", self.settings.hud));
                }
                ok()
            }
            "info" => {
                self.settings.info = toggle(a0, self.settings.info)?;
                ok()
            }
            "copy" => self.cmd_copy(a0.unwrap_or("image")),
            "reveal" => {
                let p = self.current.clone().ok_or("no file")?;
                crate::win::reveal_in_explorer(&p);
                ok()
            }
            "delete" => {
                if self.current.is_some() && self.compare.is_none() {
                    self.confirm_delete = self.current.clone();
                }
                ok()
            }
            "escape" => {
                if self.compare.is_some() {
                    self.compare = None;
                    self.view.fit = true;
                } else if self.sheet.is_some() {
                    self.sheet = None;
                } else if ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                }
                ok()
            }
            "state" => Ok(self.state_json()),
            "focus" => ok(),
            "quit" | "exit" => {
                self.quit = true;
                ok()
            }
            other => Err(format!("unknown command '{other}' (try: glint ctl help)")),
        }
    }

    fn cmd_sheet(&mut self, args: &[&str]) -> Result<Value, String> {
        let usage = "usage: sheet <WxH> [offset X,Y] [gap X,Y] [play A-B] [fps N] | sheet off";
        match args.first().copied() {
            None => return Err(usage.into()),
            Some("off") => {
                self.sheet = None;
                return Ok(json!(true));
            }
            _ => {}
        }
        let cell = parse_size(args[0]).ok_or(usage)?;
        let mut grid = SheetGrid { cell, offset: [0, 0], gap: [0, 0] };
        let mut play = None;
        let mut fps = 10.0;
        let mut it = args[1..].iter();
        while let Some(k) = it.next() {
            let v = it.next().ok_or(usage)?;
            match k.trim_start_matches("--") {
                "offset" => grid.offset = parse_pair(v).ok_or("bad offset")?,
                "gap" | "spacing" => grid.gap = parse_pair(v).ok_or("bad gap")?,
                "play" => {
                    let (a, b) = v.split_once('-').unwrap_or((v, v));
                    play = Some([a.parse().map_err(|_| "bad play range")?, b.parse().map_err(|_| "bad play range")?]);
                }
                "fps" => fps = v.parse().map_err(|_| "bad fps")?,
                _ => return Err(usage.into()),
            }
        }
        self.anim.sheet_cell = play.map_or(0, |p: [u32; 2]| p[0]);
        self.anim.sheet_accum = 0.0;
        self.sheet = Some(Sheet { grid, play, fps });
        let (cols, rows) = self.doc.as_ref().map_or((0, 0), |d| (grid.columns(d.dec.width()), grid.rows(d.dec.height())));
        Ok(json!({ "columns": cols, "rows": rows, "cells": cols * rows }))
    }

    fn cmd_copy(&mut self, what: &str) -> Result<Value, String> {
        let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        match what {
            "image" => {
                let doc = self.doc.as_ref().ok_or("no image")?;
                let img = doc.layer.frame(self.anim.frame);
                cb.set_image(arboard::ImageData {
                    width: img.width() as usize,
                    height: img.height() as usize,
                    bytes: std::borrow::Cow::Borrowed(img.as_raw()),
                })
                .map_err(|e| e.to_string())?;
                self.toast("Image copied");
            }
            "path" => {
                let p = self.current.clone().ok_or("no file")?;
                cb.set_text(p.display().to_string()).map_err(|e| e.to_string())?;
                self.toast("Path copied");
            }
            "hex" => {
                let px = self.hover_rgba().ok_or("hover over a pixel first")?;
                let h = hex(px);
                cb.set_text(h.clone()).map_err(|e| e.to_string())?;
                self.toast(format!("Copied {h}"));
            }
            o => return Err(format!("copy what? '{o}' (image|path|hex)")),
        }
        Ok(json!(true))
    }

    pub fn hover_rgba(&self) -> Option<[u8; 4]> {
        let p = self.hover_px?;
        let doc = self.doc.as_ref()?;
        let img = doc.layer.frame(self.anim.frame);
        let (x, y) = (p.x.floor(), p.y.floor());
        (x >= 0.0 && y >= 0.0 && (x as u32) < img.width() && (y as u32) < img.height())
            .then(|| img.get_pixel(x as u32, y as u32).0)
    }

    pub fn state_json(&self) -> Value {
        let v = &self.view;
        let r = v.rect;
        let tl = v.to_image(r, r.min);
        let br = v.to_image(r, r.max);
        let doc = self.doc.as_ref().map(|d| {
            json!({
                "path": d.dec.path,
                "format": d.dec.format,
                "decoder": d.dec.decoder,
                "color": d.dec.color,
                "width": d.dec.width(),
                "height": d.dec.height(),
                "frames": d.dec.frames.len(),
                "frame_delays_ms": d.dec.frames.iter().map(|f| f.delay_ms).collect::<Vec<_>>(),
                "tags": d.dec.tags,
                "file_size": d.dec.file_size,
                "notes": d.dec.notes,
                "exposure": d.exposure,
            })
        });
        let cmp = self.compare.as_ref().map(|c| {
            json!({ "a": c.a.path, "b": c.b.path, "mode": c.mode, "split": c.split, "metrics": c.metrics })
        });
        json!({
            "file": self.current,
            "index": self.index,
            "count": self.files.len(),
            "image": doc,
            "error": self.error.as_ref().map(|(p, e)| json!({"path": p, "message": e})),
            "loading": self.current.as_ref().is_some_and(|p| self.loader.is_loading(p)),
            "busy": self.is_busy(),
            "zoom_percent": (v.zoom * 1000.0).round() / 10.0,
            "fit": v.fit,
            "center": [v.center.x, v.center.y],
            "visible_rect": [tl.x.max(0.0), tl.y.max(0.0), br.x - tl.x, br.y - tl.y],
            "view_size_px": [r.width() * v.ppp, r.height() * v.ppp],
            "frame": self.anim.frame,
            "playing": self.anim.playing,
            "speed": self.anim.speed,
            "grid": self.settings.grid,
            "smooth": self.settings.smooth,
            "bg": self.settings.bg.name(),
            "sheet": self.sheet,
            "compare": cmp,
            "cursor_px": self.hover_px.map(|p| [p.x.floor(), p.y.floor()]),
            "cursor_rgba": self.hover_rgba().map(hex),
        })
    }
}
